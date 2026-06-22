use proptest::prelude::*;
use ts2zig_core::{StringId, StringTable};

#[test]
fn intern_returns_same_id_for_same_string() {
    let mut table = StringTable::new();
    let a = table.intern("hello");
    let b = table.intern("hello");
    assert_eq!(a, b);
}

#[test]
fn intern_distinguishes_distinct_strings() {
    let mut table = StringTable::new();
    let a = table.intern("hello");
    let b = table.intern("world");
    assert_ne!(a, b);
}

#[test]
fn resolve_returns_interned_string() {
    let mut table = StringTable::new();
    let id = table.intern("hello");
    assert_eq!(table.resolve(id), Some("hello"));
}

#[test]
fn resolve_handles_empty_string() {
    let mut table = StringTable::new();
    let id = table.intern("");
    assert_eq!(table.resolve(id), Some(""));
}

#[test]
fn intern_assigns_sequential_ids() {
    let mut table = StringTable::new();
    assert_eq!(table.intern("a"), StringId::from_raw(0));
    assert_eq!(table.intern("b"), StringId::from_raw(1));
    assert_eq!(table.intern("c"), StringId::from_raw(2));
}

#[test]
fn resolve_returns_none_for_invalid_id() {
    let table = StringTable::new();
    assert_eq!(table.resolve(StringId::from_raw(42)), None);
}

#[test]
fn empty_table_has_no_strings() {
    let table = StringTable::new();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
}

fn arb_string() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[a-zA-Z_][a-zA-Z0-9_]{0,15}",
        "\\PC{0,30}",
    ]
}

proptest! {
    #[test]
    fn string_intern_is_idempotent(s in arb_string()) {
        let mut table = StringTable::new();
        let a = table.intern(&s);
        let b = table.intern(&s);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn string_resolve_roundtrips(s in arb_string()) {
        let mut table = StringTable::new();
        let id = table.intern(&s);
        prop_assert_eq!(table.resolve(id), Some(s.as_str()));
    }

    #[test]
    fn string_ids_are_dense(s in arb_string()) {
        let mut table = StringTable::new();
        let id = table.intern(&s);
        prop_assert_eq!(id.raw() as usize, table.len() - 1);
    }
}
