use proptest::prelude::*;
use ts2zig_core::{SymbolId, SymbolTable};

#[test]
fn intern_returns_same_id_for_same_name() {
    let mut table = SymbolTable::new();
    let a = table.intern("foo");
    let b = table.intern("foo");
    assert_eq!(a, b);
}

#[test]
fn intern_distinguishes_distinct_names() {
    let mut table = SymbolTable::new();
    let a = table.intern("foo");
    let b = table.intern("bar");
    assert_ne!(a, b);
}

#[test]
fn resolve_returns_interned_name() {
    let mut table = SymbolTable::new();
    let id = table.intern("foo");
    assert_eq!(table.resolve(id), Some("foo"));
}

#[test]
fn intern_assigns_sequential_ids() {
    let mut table = SymbolTable::new();
    assert_eq!(table.intern("a"), SymbolId::from_raw(0));
    assert_eq!(table.intern("b"), SymbolId::from_raw(1));
}

#[test]
fn resolve_returns_none_for_invalid_id() {
    let table = SymbolTable::new();
    assert_eq!(table.resolve(SymbolId::from_raw(42)), None);
}

#[test]
fn empty_table_has_no_symbols() {
    let table = SymbolTable::new();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
}

fn arb_identifier() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("x")),
        Just(String::from("_")),
        "[a-zA-Z_][a-zA-Z0-9_]{0,15}",
    ]
}

proptest! {
    #[test]
    fn symbol_intern_is_idempotent(name in arb_identifier()) {
        let mut table = SymbolTable::new();
        let a = table.intern(&name);
        let b = table.intern(&name);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn symbol_resolve_roundtrips(name in arb_identifier()) {
        let mut table = SymbolTable::new();
        let id = table.intern(&name);
        prop_assert_eq!(table.resolve(id), Some(name.as_str()));
    }

    #[test]
    fn distinct_names_get_distinct_ids(
        a in arb_identifier(),
        b in arb_identifier(),
    ) {
        prop_assume!(a != b);
        let mut table = SymbolTable::new();
        let id_a = table.intern(&a);
        let id_b = table.intern(&b);
        prop_assert_ne!(id_a, id_b);
    }
}
