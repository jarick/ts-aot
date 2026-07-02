use std::collections::HashMap;

use crate::ids::TypeId;
use crate::ty::Type;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    types: Vec<Type>,
    index: HashMap<Type, TypeId>,
}

impl TypeTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn intern(&mut self, ty: &Type) -> TypeId {
        if let Some(&id) = self.index.get(ty) {
            return id;
        }
        let id = TypeId::from_raw(u32::try_from(self.types.len()).expect("type table overflow"));
        self.types.push(ty.clone());
        self.index.insert(ty.clone(), id);
        id
    }

    #[must_use]
    pub fn resolve(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.raw() as usize)
    }

    #[must_use]
    pub fn types(&self) -> &[Type] {
        &self.types
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::Type;

    #[test]
    fn empty_table_has_no_types() {
        let table = TypeTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert!(table.types().is_empty());
    }

    #[test]
    fn intern_distinguishes_unrelated_variants() {
        let mut table = TypeTable::new();
        let a = table.intern(&Type::I32);
        let b = table.intern(&Type::String);
        assert_ne!(a, b);
        assert_eq!(table.resolve(a), Some(&Type::I32));
        assert_eq!(table.resolve(b), Some(&Type::String));
    }

    #[test]
    fn intern_dedupes_equal_types() {
        let mut table = TypeTable::new();
        let a = table.intern(&Type::I32);
        let b = table.intern(&Type::I32);
        assert_eq!(a, b);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn resolve_returns_none_for_unbound_id() {
        let table = TypeTable::new();
        assert_eq!(table.resolve(TypeId::from_raw(99)), None);
    }
}
