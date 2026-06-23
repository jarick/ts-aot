use crate::ids::TypeId;
use crate::interner::Interner;
use crate::ty::Type;

#[derive(Debug, Clone)]
pub struct TypeTable(Interner<Type, TypeId>);

impl Default for TypeTable {
    fn default() -> Self {
        Self(Interner::new())
    }
}

impl TypeTable {
    #[must_use]
    pub fn new() -> Self {
        Self(Interner::new())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn intern(&mut self, ty: &Type) -> TypeId {
        self.0.intern(ty)
    }

    #[must_use]
    pub fn resolve(&self, id: TypeId) -> Option<&Type> {
        self.0.resolve(id)
    }

    #[must_use]
    pub fn types(&self) -> &[Type] {
        self.0.values()
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
    fn resolve_returns_none_for_unbound_id() {
        let table = TypeTable::new();
        assert_eq!(table.resolve(TypeId::from_raw(99)), None);
    }
}
