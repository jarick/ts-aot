use std::cell::Cell;
use std::collections::HashMap;

use ts_aot_core::{LocalId, TypeId};

#[derive(Clone, Copy)]
struct LocalEntry {
    id: LocalId,
    ty: TypeId,
}

struct PredeclaredEntry {
    ty: TypeId,
    allocated: Cell<Option<LocalId>>,
}

/// The state [`BodyScope::narrowing_checkpoint`] returns and
/// [`BodyScope::discard_narrowings_since`] puts back.
pub(crate) struct NarrowingCheckpoint {
    /// The bindings the frame had when the checkpoint was taken.
    bindings: HashMap<String, LocalEntry>,
    /// The frame's narrowing records at that point.
    records: HashMap<String, Option<LocalEntry>>,
}

pub(crate) struct BodyScope {
    next_local: Cell<u32>,
    scopes: Vec<HashMap<String, LocalEntry>>,
    predeclared: Vec<HashMap<String, PredeclaredEntry>>,
    /// Flow-sensitive narrowings of this frame: the entry a variable had before
    /// `narrow` replaced it (`None` when it was not in the frame), so `widen`
    /// and `pop` can restore exactly the state that was there.
    narrowings: Vec<HashMap<String, Option<LocalEntry>>>,
}

impl BodyScope {
    pub(crate) fn new(param_count: u32) -> Self {
        Self {
            next_local: Cell::new(param_count),
            scopes: vec![HashMap::new()],
            predeclared: vec![HashMap::new()],
            narrowings: vec![HashMap::new()],
        }
    }

    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
        self.predeclared.push(HashMap::new());
        self.narrowings.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        self.scopes.pop();
        self.predeclared.pop();
        self.narrowings.pop();
    }

    /// Rebinds a variable to a narrowed local (`id`/`ty`) for the rest of this
    /// frame: an assignment guarantees the narrowed type. The entry that was
    /// visible before is remembered so [`BodyScope::widen`] can restore it.
    pub(crate) fn narrow(&mut self, name: &str, id: LocalId, ty: TypeId) {
        let previous = self.scopes.last().and_then(|top| top.get(name).copied());
        if let Some(record) = self.narrowings.last_mut() {
            record.entry(name.to_string()).or_insert(previous);
        }
        self.insert(name, LocalEntry { id, ty });
    }

    /// Drops a narrowing recorded by [`BodyScope::narrow`], restoring the entry
    /// that was visible before it (used when a later assignment may be nullish).
    ///
    /// The record lives in the frame where the narrowing was introduced, which
    /// is not necessarily the frame being walked now: an assignment inside an
    /// `if`, a loop or a `try` body must cancel a narrowing established outside
    /// it, otherwise the rest of the function keeps reading the stale narrowed
    /// copy while the variable's own storage may have been set back to a nullish
    /// value.
    ///
    /// The frame is the one the name *resolves* to - the innermost that declares
    /// it - and the search stops there. A declaration in a nested block shadows
    /// the outer variable, so a write to the inner one must not cancel the outer
    /// narrowing; the inner binding has no record, which is what makes the
    /// assignment a no-op here.
    pub(crate) fn widen(&mut self, name: &str) {
        let Some(index) = self
            .scopes
            .iter()
            .rposition(|frame| frame.contains_key(name))
        else {
            return;
        };
        let Some(record) = self.narrowings.get_mut(index) else {
            return;
        };
        // No record for this binding: it was not created by `narrow` (a plain
        // declaration, or the variable's own storage), so there is nothing to
        // widen and the frames outside hold a *different* variable.
        let Some(previous) = record.remove(name) else {
            return;
        };
        let Some(frame) = self.scopes.get_mut(index) else {
            return;
        };
        match previous {
            Some(entry) => {
                frame.insert(name.to_string(), entry);
            }
            None => {
                frame.remove(name);
            }
        }
    }

    /// The local the name resolves to in the frames that are pushed now, without
    /// allocating one for a predeclared name (which [`BodyScope::lookup`] does).
    ///
    /// [`BodyScope::widen_before_loop`] resolves the names a loop writes to their
    /// binding *before* the body is walked, so the widening cannot be redirected
    /// by a declaration inside the loop.
    pub(crate) fn visible_binding(&self, name: &str) -> Option<LocalId> {
        self.lookup_in_scopes(name).map(|entry| entry.id)
    }

    /// Drops the narrowing bound to `id` (the local [`BodyScope::narrow`]
    /// created), restoring the entry that was visible before it.
    ///
    /// The narrowing is identified by the local it is bound to rather than by
    /// the name: two frames may narrow the *same* name (a shadowing declaration
    /// in a nested block), and [`BodyScope::widen`] would restore the innermost
    /// one, leaving the outer narrowing in effect for the rest of the function
    /// even though its variable's storage changed.
    pub(crate) fn widen_id(&mut self, id: LocalId) {
        for index in (0..self.narrowings.len()).rev() {
            let Some(scope) = self.scopes.get(index) else {
                continue;
            };
            let Some(name) = scope
                .iter()
                .find_map(|(name, entry)| (entry.id == id).then(|| name.clone()))
            else {
                continue;
            };
            // No recorded narrowing for this name: the binding was not created
            // by `narrow`, so leave it alone.
            let Some(previous) = self.narrowings[index].remove(&name) else {
                return;
            };
            let Some(scope) = self.scopes.get_mut(index) else {
                return;
            };
            match previous {
                Some(entry) => {
                    scope.insert(name, entry);
                }
                None => {
                    scope.remove(&name);
                }
            }
            return;
        }
    }

    /// The narrowing state of the current frame, for a construct whose cases
    /// share one frame (a `switch`): [`BodyScope::discard_narrowings_since`]
    /// puts a case's changes back before the next one is walked.
    ///
    /// Both halves are needed. The *records* tell which narrowings the case
    /// introduced (their record is new) and which it merely re-narrowed (the
    /// record already existed, so `narrow`'s `or_insert` kept the old one); the
    /// *bindings* are the entries those records restore, which is what the
    /// re-narrowed names would otherwise keep from the case.
    pub(crate) fn narrowing_checkpoint(&self) -> NarrowingCheckpoint {
        NarrowingCheckpoint {
            bindings: self.scopes.last().cloned().unwrap_or_default(),
            records: self.narrowings.last().cloned().unwrap_or_default(),
        }
    }

    /// Discards every narrowing the current frame established since `checkpoint`,
    /// restoring the entry each of them replaced.
    ///
    /// A name recorded *before* the checkpoint keeps its record unless the
    /// construct widened it (a nullable assignment), and its binding is put back
    /// to what the checkpoint saw - a re-narrowing of the same name would
    /// otherwise leave the next case pointing at a `_narrowed_N` whose `let` was
    /// emitted in the previous case's arm. Declarations the case made are not
    /// touched: all cases share the switch's block scope.
    pub(crate) fn discard_narrowings_since(&mut self, checkpoint: NarrowingCheckpoint) {
        // Destructured so the checkpoint is consumed: it describes one position in
        // the walk and is not meant to be reused.
        let NarrowingCheckpoint {
            bindings,
            records: seen,
        } = checkpoint;
        let Some(records) = self.narrowings.last_mut() else {
            return;
        };
        // The narrowings this construct introduced, with the entry each replaced.
        let introduced: Vec<String> = records
            .keys()
            .filter(|name| !seen.contains_key(*name))
            .cloned()
            .collect();
        let mut restores: Vec<(String, Option<LocalEntry>)> = introduced
            .iter()
            .map(|name| (name.clone(), records.remove(name).flatten()))
            .collect();
        // Names that were already narrowed when the checkpoint was taken: the
        // construct may have re-narrowed one, which `narrow` records without a new
        // record, so the binding is restored from the checkpoint instead. A name
        // whose record is gone was *widened* by the construct; that write is real
        // and has to keep holding.
        for name in seen.keys() {
            if !records.contains_key(name) {
                continue;
            }
            restores.push((name.clone(), bindings.get(name).copied()));
        }
        let Some(frame) = self.scopes.last_mut() else {
            return;
        };
        for (name, entry) in restores {
            match entry {
                Some(entry) => {
                    frame.insert(name, entry);
                }
                None => {
                    frame.remove(&name);
                }
            }
        }
    }

    fn insert(&mut self, name: &str, entry: LocalEntry) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), entry);
        }
    }

    fn alloc_id(&self) -> LocalId {
        let raw = self.next_local.get();
        self.next_local.set(raw.saturating_add(1));
        LocalId::from_raw(raw)
    }

    fn alloc_for_predeclared(&self, pre: &PredeclaredEntry) -> LocalId {
        if let Some(id) = pre.allocated.get() {
            return id;
        }
        let id = self.alloc_id();
        pre.allocated.set(Some(id));
        id
    }

    pub(crate) fn declare(&mut self, name: &str, ty: TypeId) -> LocalId {
        if let Some(top) = self.scopes.last()
            && let Some(entry) = top.get(name)
        {
            return entry.id;
        }
        if let Some(top_pre) = self.predeclared.last_mut()
            && let Some(pre) = top_pre.remove(name)
        {
            let id = self.alloc_for_predeclared(&pre);
            self.insert(name, LocalEntry { id, ty });
            return id;
        }
        let id = self.alloc_id();
        self.insert(name, LocalEntry { id, ty });
        id
    }

    pub(crate) fn predeclare(&mut self, name: &str, ty: TypeId) {
        if let Some(top) = self.scopes.last()
            && top.contains_key(name)
        {
            return;
        }
        if let Some(top_pre) = self.predeclared.last_mut()
            && !top_pre.contains_key(name)
        {
            top_pre.insert(
                name.to_string(),
                PredeclaredEntry {
                    ty,
                    allocated: Cell::new(None),
                },
            );
        }
    }

    pub(crate) fn declare_param(&mut self, name: &str, id: LocalId, ty: TypeId) {
        if let Some(top_pre) = self.predeclared.last_mut() {
            top_pre.remove(name);
        }
        self.insert(name, LocalEntry { id, ty });
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<(LocalId, TypeId)> {
        if let Some(entry) = self.lookup_in_scopes(name) {
            return Some((entry.id, entry.ty));
        }
        for frame in self.predeclared.iter().rev() {
            if let Some(pre) = frame.get(name) {
                let id = self.alloc_for_predeclared(pre);
                return Some((id, pre.ty));
            }
        }
        None
    }

    fn lookup_in_scopes(&self, name: &str) -> Option<LocalEntry> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    pub(crate) fn peek_next(&self) -> u32 {
        self.next_local.get()
    }

    pub(crate) fn set_next(&self, value: u32) {
        self.next_local.set(self.next_local.get().max(value));
    }

    pub(crate) fn type_of_visible(&self, name: &str) -> Option<TypeId> {
        if let Some(entry) = self.lookup_in_scopes(name) {
            return Some(entry.ty);
        }
        for frame in self.predeclared.iter().rev() {
            if let Some(pre) = frame.get(name) {
                return Some(pre.ty);
            }
        }
        None
    }

    pub(crate) fn names(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for scope in &self.scopes {
            for key in scope.keys() {
                if !seen.iter().any(|s| s == key) {
                    seen.push(key.clone());
                }
            }
        }
        for frame in &self.predeclared {
            for key in frame.keys() {
                if !seen.iter().any(|s| s == key) {
                    seen.push(key.clone());
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(raw: u32) -> TypeId {
        TypeId::from_raw(raw)
    }

    fn id(raw: u32) -> LocalId {
        LocalId::from_raw(raw)
    }

    /// A case that re-narrows a name the checkpoint already saw adds no *new*
    /// record (`narrow` keeps the first one with `or_insert`), so discarding the
    /// introduced records alone would leave the frame pointing at the case's
    /// `_narrowed_N` - whose `let` was emitted in that case's arm.
    #[test]
    fn discarding_since_a_checkpoint_restores_a_re_narrowed_name() {
        let mut scope = BodyScope::new(0);
        scope.declare("x", ty(1));
        scope.push();
        scope.narrow("x", id(10), ty(2));
        let checkpoint = scope.narrowing_checkpoint();
        scope.narrow("x", id(11), ty(2));
        scope.discard_narrowings_since(checkpoint);
        assert_eq!(
            scope.lookup("x"),
            Some((id(10), ty(2))),
            "the binding the checkpoint saw has to come back"
        );
    }

    /// A narrowing the case introduced is dropped, restoring the entry it replaced.
    #[test]
    fn discarding_since_a_checkpoint_drops_an_introduced_narrowing() {
        let mut scope = BodyScope::new(0);
        let storage = scope.declare("x", ty(1));
        scope.push();
        let checkpoint = scope.narrowing_checkpoint();
        scope.narrow("x", id(10), ty(2));
        scope.discard_narrowings_since(checkpoint);
        assert_eq!(
            scope.lookup("x"),
            Some((storage, ty(1))),
            "the narrowed copy must not survive into the next case"
        );
    }

    /// A nullable write inside the case widened the narrowing; that write is real
    /// and has to keep holding, so discarding must not re-narrow the name.
    #[test]
    fn discarding_since_a_checkpoint_keeps_a_widening() {
        let mut scope = BodyScope::new(0);
        let storage = scope.declare("x", ty(1));
        scope.push();
        scope.narrow("x", id(10), ty(2));
        let checkpoint = scope.narrowing_checkpoint();
        scope.widen("x");
        scope.discard_narrowings_since(checkpoint);
        assert_eq!(
            scope.lookup("x"),
            Some((storage, ty(1))),
            "a nullable write inside the case stays visible after it"
        );
    }

    /// A declaration in a nested frame shadows the outer variable, so a write the
    /// inner code makes must stop at *that* binding: cancelling the outer
    /// narrowing would leave the rest of the function reading the outer storage
    /// with a stale type (a spurious "optional value in a non-optional position").
    #[test]
    fn widening_stops_at_a_shadowing_declaration() {
        let mut scope = BodyScope::new(0);
        scope.declare("x", ty(1));
        scope.narrow("x", id(10), ty(2));
        scope.push();
        let shadow = scope.declare("x", ty(3));
        scope.widen("x");
        assert_eq!(
            scope.lookup("x"),
            Some((shadow, ty(3))),
            "the shadowing binding has no narrowing to withdraw"
        );
        scope.pop();
        assert_eq!(
            scope.lookup("x"),
            Some((id(10), ty(2))),
            "the outer narrowing belongs to a different variable and has to survive"
        );
    }

    /// Two frames may narrow the *same* name. Resolving the name to its binding
    /// and widening by that local is what withdraws exactly the narrowing of the
    /// variable a loop writes.
    #[test]
    fn widening_a_resolved_binding_keeps_the_other_frames_narrowing() {
        let mut scope = BodyScope::new(0);
        let storage = scope.declare("x", ty(1));
        scope.narrow("x", id(10), ty(2));
        scope.push();
        scope.narrow("x", id(11), ty(3));
        assert_eq!(
            scope.visible_binding("x"),
            Some(id(11)),
            "the inner frame's narrowing is what the name resolves to"
        );
        scope.widen_id(id(11));
        assert_eq!(
            scope.lookup("x"),
            Some((id(10), ty(2))),
            "only the inner narrowing goes"
        );
        scope.pop();
        assert_eq!(
            scope.lookup("x"),
            Some((id(10), ty(2))),
            "the outer frame keeps its own narrowing"
        );
        scope.widen_id(id(10));
        assert_eq!(scope.lookup("x"), Some((storage, ty(1))));
    }
}
