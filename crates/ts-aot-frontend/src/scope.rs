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

pub(crate) struct BodyScope {
    next_local: Cell<u32>,
    scopes: Vec<HashMap<String, LocalEntry>>,
    predeclared: Vec<HashMap<String, PredeclaredEntry>>,
}

impl BodyScope {
    pub(crate) fn new(param_count: u32) -> Self {
        Self {
            next_local: Cell::new(param_count),
            scopes: vec![HashMap::new()],
            predeclared: vec![HashMap::new()],
        }
    }

    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
        self.predeclared.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        self.scopes.pop();
        self.predeclared.pop();
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
