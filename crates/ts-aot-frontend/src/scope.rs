use std::collections::HashMap;

use ts_aot_core::{LocalId, TypeId};

#[derive(Clone, Copy)]
struct LocalEntry {
    id: LocalId,
    ty: TypeId,
}

pub(crate) struct BodyScope {
    next_local: u32,
    scopes: Vec<HashMap<String, LocalEntry>>,
}

impl BodyScope {
    pub(crate) fn new(param_count: u32) -> Self {
        Self {
            next_local: param_count,
            scopes: vec![HashMap::new()],
        }
    }

    pub(crate) fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub(crate) fn pop(&mut self) {
        self.scopes.pop();
    }

    fn insert(&mut self, name: &str, entry: LocalEntry) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), entry);
        }
    }

    pub(crate) fn declare(&mut self, name: &str, ty: TypeId) -> LocalId {
        let id = LocalId::from_raw(self.next_local);
        self.next_local = self.next_local.saturating_add(1);
        self.insert(name, LocalEntry { id, ty });
        id
    }

    pub(crate) fn declare_param(&mut self, name: &str, id: LocalId, ty: TypeId) {
        self.insert(name, LocalEntry { id, ty });
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<(LocalId, TypeId)> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .map(|e| (e.id, e.ty))
    }
}
