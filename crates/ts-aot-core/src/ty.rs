use crate::ids::{Atom, GenericParamId, StructId, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    CopyValue,
    ValueStruct,
    GcRef,
    NullableGcRef,
    LinearMemoryValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Never,
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
    Null,
    Optional {
        inner: TypeId,
    },
    Struct {
        id: StructId,
    },
    Array {
        element: TypeId,
    },
    Fn {
        params: Vec<TypeId>,
        ret: TypeId,
        err: Option<TypeId>,
    },
    Promise {
        ok: TypeId,
        err: Option<TypeId>,
    },
    Result {
        ok: TypeId,
        err: TypeId,
    },
    Named {
        symbol: Atom,
    },
    GenericParam {
        id: GenericParamId,
    },
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{FunctionId, LocalId};

    #[test]
    fn primitive_variants_are_distinct() {
        assert_ne!(Type::I32, Type::I64);
        assert_ne!(Type::F32, Type::F64);
        assert_ne!(Type::String, Type::Null);
        assert_ne!(Type::Bool, Type::I32);
        assert_ne!(Type::Void, Type::Never);
        assert_ne!(Type::Error, Type::Void);
    }

    #[test]
    fn primitive_variants_hash_consistently() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Type::I32);
        set.insert(Type::I32);
        set.insert(Type::I64);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn optional_equality_depends_only_on_inner() {
        let a = Type::Optional {
            inner: TypeId::from_raw(7),
        };
        let b = Type::Optional {
            inner: TypeId::from_raw(7),
        };
        let c = Type::Optional {
            inner: TypeId::from_raw(8),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn struct_equality_depends_only_on_id() {
        let a = Type::Struct {
            id: StructId::from_raw(3),
        };
        let b = Type::Struct {
            id: StructId::from_raw(3),
        };
        let c = Type::Struct {
            id: StructId::from_raw(4),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fn_equality_considers_all_components() {
        let p1 = TypeId::from_raw(1);
        let p2 = TypeId::from_raw(2);
        let ret = TypeId::from_raw(3);
        let err = TypeId::from_raw(4);
        let base = Type::Fn {
            params: vec![p1, p2],
            ret,
            err: None,
        };
        assert_eq!(
            base.clone(),
            Type::Fn {
                params: vec![p1, p2],
                ret,
                err: None
            }
        );
        assert_ne!(
            base.clone(),
            Type::Fn {
                params: vec![p1, p2],
                ret,
                err: Some(err)
            }
        );
        assert_ne!(
            base,
            Type::Fn {
                params: vec![p2, p1],
                ret,
                err: None
            }
        );
    }

    #[test]
    fn result_distinguishes_ok_from_err() {
        let ty = Type::Result {
            ok: TypeId::from_raw(1),
            err: TypeId::from_raw(2),
        };
        assert_ne!(
            ty.clone(),
            Type::Result {
                ok: TypeId::from_raw(2),
                err: TypeId::from_raw(1)
            }
        );
    }

    #[test]
    fn named_equality_depends_only_on_symbol() {
        let a = Type::Named {
            symbol: Atom::from("Foo"),
        };
        let b = Type::Named {
            symbol: Atom::from("Foo"),
        };
        let c = Type::Named {
            symbol: Atom::from("Bar"),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn memory_kind_is_copy_and_distinct() {
        let kinds = [
            MemoryKind::CopyValue,
            MemoryKind::ValueStruct,
            MemoryKind::GcRef,
            MemoryKind::NullableGcRef,
            MemoryKind::LinearMemoryValue,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn type_carries_indirection_ids_without_compile_error() {
        let _ = Type::Array {
            element: TypeId::from_raw(0),
        };
        let _ = Type::Promise {
            ok: TypeId::from_raw(1),
            err: None,
        };
        let _ = Type::Optional {
            inner: TypeId::from_raw(2),
        };
        let _ = FunctionId::from_raw(0);
        let _ = LocalId::from_raw(0);
    }
}
