use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

use ts_aot_core::{LocalId, TypeId};
use ts_aot_ir_hir::{HirParam, HirStmt};

static NEXT_BODY_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct ClosureBody {
    pub(crate) params: Vec<HirParam>,
    pub(crate) body: Vec<HirStmt>,
}

#[derive(Clone, Copy)]
struct LocalEntry {
    id: LocalId,
    ty: TypeId,
}

struct PredeclaredEntry {
    ty: TypeId,
    allocated: Cell<Option<LocalId>>,
}

pub(crate) struct NarrowingCheckpoint {
    bindings: HashMap<String, LocalEntry>,
    records: HashMap<String, Option<LocalEntry>>,
}

pub(crate) struct BodyScope {
    body_id: u32,
    next_local: Cell<u32>,
    scopes: Vec<HashMap<String, LocalEntry>>,
    predeclared: Vec<HashMap<String, PredeclaredEntry>>,
    narrowings: Vec<HashMap<String, Option<LocalEntry>>>,
    closure_bodies: Vec<HashMap<LocalId, Vec<ClosureBody>>>,
    poisoned_closure_bodies: Vec<HashSet<LocalId>>,
}

impl BodyScope {
    pub(crate) fn new(param_count: u32) -> Self {
        Self {
            body_id: NEXT_BODY_ID.fetch_add(1, Ordering::Relaxed),
            next_local: Cell::new(param_count),
            scopes: vec![HashMap::new()],
            predeclared: vec![HashMap::new()],
            narrowings: vec![HashMap::new()],
            closure_bodies: vec![HashMap::new()],
            poisoned_closure_bodies: vec![HashSet::new()],
        }
    }

    pub(crate) fn body_id(&self) -> u32 {
        self.body_id
    }

    pub(crate) fn depth(&self) -> usize {
        self.scopes.len()
    }

    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
        self.predeclared.push(HashMap::new());
        self.narrowings.push(HashMap::new());
        self.closure_bodies.push(HashMap::new());
        self.poisoned_closure_bodies.push(HashSet::new());
    }

    pub(crate) fn pop(&mut self) {
        self.scopes.pop();
        self.predeclared.pop();
        self.narrowings.pop();
        self.closure_bodies.pop();
        self.poisoned_closure_bodies.pop();
    }

    fn closure_body_frame(
        &mut self,
        id: LocalId,
    ) -> Option<&mut HashMap<LocalId, Vec<ClosureBody>>> {
        self.closure_bodies
            .iter_mut()
            .rev()
            .find(|frame| frame.contains_key(&id))
    }

    fn is_closure_body_poisoned(&self, id: LocalId) -> bool {
        self.poisoned_closure_bodies
            .iter()
            .any(|frame| frame.contains(&id))
    }

    fn binding_frame_index(&self, id: LocalId) -> Option<usize> {
        self.scopes
            .iter()
            .rposition(|frame| frame.values().any(|entry| entry.id == id))
    }

    fn push_closure_body(
        frame: &mut HashMap<LocalId, Vec<ClosureBody>>,
        id: LocalId,
        body: ClosureBody,
    ) {
        let bodies = frame.entry(id).or_default();
        if !bodies.contains(&body) {
            bodies.push(body);
        }
    }

    pub(crate) fn set_closure_body(&mut self, id: LocalId, body: ClosureBody) {
        if self.is_closure_body_poisoned(id) {
            return;
        }
        if let Some(frame) = self.closure_body_frame(id) {
            Self::push_closure_body(frame, id, body);
            return;
        }
        let index = self
            .binding_frame_index(id)
            .unwrap_or_else(|| self.closure_bodies.len().saturating_sub(1));
        if let Some(frame) = self.closure_bodies.get_mut(index) {
            Self::push_closure_body(frame, id, body);
        }
    }

    pub(crate) fn poison_closure_body(&mut self, id: LocalId) {
        if let Some(frame) = self.closure_body_frame(id) {
            frame.remove(&id);
        }
        let index = self
            .binding_frame_index(id)
            .unwrap_or_else(|| self.poisoned_closure_bodies.len().saturating_sub(1));
        if let Some(frame) = self.poisoned_closure_bodies.get_mut(index) {
            frame.insert(id);
        }
    }

    pub(crate) fn closure_bodies_of_local(&self, id: LocalId) -> Option<Vec<ClosureBody>> {
        if self.is_closure_body_poisoned(id) {
            return None;
        }
        self.closure_bodies
            .iter()
            .rev()
            .find_map(|frame| frame.get(&id).cloned())
    }

    pub(crate) fn visible_closure_bodies(&self) -> HashMap<LocalId, Vec<ClosureBody>> {
        let mut out: HashMap<LocalId, Vec<ClosureBody>> = HashMap::new();
        for frame in &self.closure_bodies {
            for (id, bodies) in frame {
                if self.is_closure_body_poisoned(*id) {
                    continue;
                }
                let entry = out.entry(*id).or_default();
                for body in bodies {
                    if !entry.contains(body) {
                        entry.push(body.clone());
                    }
                }
            }
        }
        out
    }

    pub(crate) fn narrow(&mut self, name: &str, id: LocalId, ty: TypeId) {
        let previous = self.scopes.last().and_then(|top| top.get(name).copied());
        if let Some(record) = self.narrowings.last_mut() {
            record.entry(name.to_string()).or_insert(previous);
        }
        self.insert(name, LocalEntry { id, ty });
    }

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

    pub(crate) fn visible_binding(&self, name: &str) -> Option<LocalId> {
        self.lookup_in_scopes(name).map(|entry| entry.id)
    }

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

    pub(crate) fn narrowing_checkpoint(&self) -> NarrowingCheckpoint {
        NarrowingCheckpoint {
            bindings: self.scopes.last().cloned().unwrap_or_default(),
            records: self.narrowings.last().cloned().unwrap_or_default(),
        }
    }

    pub(crate) fn discard_narrowings_since(&mut self, checkpoint: NarrowingCheckpoint) {
        let NarrowingCheckpoint {
            bindings,
            records: seen,
        } = checkpoint;
        let Some(records) = self.narrowings.last_mut() else {
            return;
        };
        let introduced: Vec<String> = records
            .keys()
            .filter(|name| !seen.contains_key(*name))
            .cloned()
            .collect();
        let mut restores: Vec<(String, Option<LocalEntry>)> = introduced
            .iter()
            .map(|name| (name.clone(), records.remove(name).flatten()))
            .collect();
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
