use proptest::prelude::*;
use ts2zig_core::{StructId, SymbolId, Type, TypeId, TypeTable};

#[test]
fn intern_returns_same_id_for_same_type() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::I32);
    let b = table.intern(&Type::I32);
    assert_eq!(a, b);
}

#[test]
fn intern_distinguishes_distinct_primitive_types() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::I32);
    let b = table.intern(&Type::I64);
    assert_ne!(a, b);
}

#[test]
fn resolve_returns_interned_type() {
    let mut table = TypeTable::new();
    let id = table.intern(&Type::String);
    assert_eq!(table.resolve(id), Some(&Type::String));
}

#[test]
fn intern_assigns_sequential_ids() {
    let mut table = TypeTable::new();
    assert_eq!(table.intern(&Type::I32), TypeId::from_raw(0));
    assert_eq!(table.intern(&Type::I64), TypeId::from_raw(1));
    assert_eq!(table.intern(&Type::Bool), TypeId::from_raw(2));
}

#[test]
fn resolve_returns_none_for_invalid_id() {
    let table = TypeTable::new();
    assert_eq!(table.resolve(TypeId::from_raw(42)), None);
}

#[test]
fn empty_table_has_no_types() {
    let table = TypeTable::new();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
}

#[test]
fn optional_dedups_by_inner() {
    let mut table = TypeTable::new();
    let inner = table.intern(&Type::I32);
    let a = table.intern(&Type::Optional { inner });
    let b = table.intern(&Type::Optional { inner });
    assert_eq!(a, b);
}

#[test]
fn optional_distinguishes_by_inner() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::Optional {
        inner: TypeId::from_raw(1),
    });
    let b = table.intern(&Type::Optional {
        inner: TypeId::from_raw(2),
    });
    assert_ne!(a, b);
}

#[test]
fn array_dedups_by_element() {
    let mut table = TypeTable::new();
    let element = table.intern(&Type::Bool);
    let a = table.intern(&Type::Array { element });
    let b = table.intern(&Type::Array { element });
    assert_eq!(a, b);
}

#[test]
fn fn_dedups_by_full_signature() {
    let mut table = TypeTable::new();
    let p1 = table.intern(&Type::I32);
    let p2 = table.intern(&Type::I64);
    let ret = table.intern(&Type::Bool);
    let a = table.intern(&Type::Fn {
        params: vec![p1, p2],
        ret,
        err: None,
    });
    let b = table.intern(&Type::Fn {
        params: vec![p1, p2],
        ret,
        err: None,
    });
    assert_eq!(a, b);
    assert_eq!(table.len(), 4);
}

#[test]
fn fn_distinguishes_by_param_order() {
    let mut table = TypeTable::new();
    let p1 = table.intern(&Type::I32);
    let p2 = table.intern(&Type::I64);
    let ret = table.intern(&Type::Bool);
    let a = table.intern(&Type::Fn {
        params: vec![p1, p2],
        ret,
        err: None,
    });
    let b = table.intern(&Type::Fn {
        params: vec![p2, p1],
        ret,
        err: None,
    });
    assert_ne!(a, b);
}

#[test]
fn result_dedups_by_components() {
    let mut table = TypeTable::new();
    let ok = table.intern(&Type::String);
    let err = table.intern(&Type::Named {
        symbol: SymbolId::from_raw(0),
    });
    let a = table.intern(&Type::Result { ok, err });
    let b = table.intern(&Type::Result { ok, err });
    assert_eq!(a, b);
}

#[test]
fn struct_distinguishes_by_id() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::Struct {
        id: StructId::from_raw(0),
    });
    let b = table.intern(&Type::Struct {
        id: StructId::from_raw(1),
    });
    assert_ne!(a, b);
}

#[test]
fn named_dedups_by_symbol() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::Named {
        symbol: SymbolId::from_raw(7),
    });
    let b = table.intern(&Type::Named {
        symbol: SymbolId::from_raw(7),
    });
    assert_eq!(a, b);
}

#[test]
fn error_variant_is_singleton() {
    let mut table = TypeTable::new();
    let a = table.intern(&Type::Error);
    let b = table.intern(&Type::Error);
    assert_eq!(a, b);
    assert_eq!(table.len(), 1);
}

#[test]
fn promise_dedups_by_ok_and_err() {
    let mut table = TypeTable::new();
    let ok = table.intern(&Type::I32);
    let err = table.intern(&Type::String);
    let a = table.intern(&Type::Promise { ok, err: Some(err) });
    let b = table.intern(&Type::Promise { ok, err: Some(err) });
    assert_eq!(a, b);
    let c = table.intern(&Type::Promise { ok, err: None });
    assert_ne!(a, c);
}

#[test]
fn intern_with_dependent_types_dedups() {
    let mut table = TypeTable::new();
    let i32_id = table.intern(&Type::I32);
    let opt1 = table.intern(&Type::Optional { inner: i32_id });
    let opt2 = table.intern(&Type::Optional { inner: i32_id });
    assert_eq!(opt1, opt2);
    let arr = table.intern(&Type::Array { element: opt1 });
    let arr2 = table.intern(&Type::Array { element: opt2 });
    assert_eq!(arr, arr2);
    assert_eq!(table.len(), 3);
}

fn arb_simple_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::Void),
        Just(Type::Never),
        Just(Type::Bool),
        Just(Type::I8),
        Just(Type::I16),
        Just(Type::I32),
        Just(Type::I64),
        Just(Type::U8),
        Just(Type::U16),
        Just(Type::U32),
        Just(Type::U64),
        Just(Type::F32),
        Just(Type::F64),
        Just(Type::String),
        Just(Type::Null),
        Just(Type::Error),
        (0u32..64, 0u32..64).prop_map(|(inner, _)| Type::Optional {
            inner: TypeId::from_raw(inner)
        }),
        (0u32..64).prop_map(|id| Type::Struct {
            id: StructId::from_raw(id)
        }),
        (0u32..64).prop_map(|id| Type::Named {
            symbol: SymbolId::from_raw(id)
        }),
    ]
}

proptest! {
    #[test]
    fn intern_is_idempotent(ty in arb_simple_type()) {
        let mut table = TypeTable::new();
        let a = table.intern(&ty);
        let b = table.intern(&ty);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn resolve_roundtrips(ty in arb_simple_type()) {
        let mut table = TypeTable::new();
        let id = table.intern(&ty);
        prop_assert_eq!(table.resolve(id), Some(&ty));
    }

    #[test]
    fn ids_are_dense(ty in arb_simple_type()) {
        let mut table = TypeTable::new();
        let id = table.intern(&ty);
        prop_assert_eq!(id.raw() as usize, table.len() - 1);
    }

    #[test]
    fn distinct_primitive_variants_get_distinct_ids(
        a in arb_simple_type(),
        b in arb_simple_type(),
    ) {
        prop_assume!(a != b);
        let mut table = TypeTable::new();
        let id_a = table.intern(&a);
        let id_b = table.intern(&b);
        prop_assert_ne!(id_a, id_b);
    }
}
