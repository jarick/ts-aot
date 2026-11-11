use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::thread;

use ts_aot_runtime::{
    __ts_aot_symbol_for, __ts_aot_symbol_key_for, __ts_aot_symbol_new, __ts_aot_symbol_new_desc,
    Ctx, Host, JsString, run_in_arena,
};

fn with_ctx<R>(f: impl for<'a> FnOnce(&Ctx<'a>) -> R) -> R {
    struct H;
    impl Host for H {}
    run_in_arena(&H, f)
}

fn key_for(sym: i64) -> Option<String> {
    with_ctx(|ctx| __ts_aot_symbol_key_for(ctx, sym).map(|s| s.to_string_lossy()))
}

#[test]
fn symbol_new_returns_unique_ids_for_repeated_calls() {
    let a = __ts_aot_symbol_new();
    let b = __ts_aot_symbol_new();
    assert_ne!(a, b, "two Symbol() calls must produce distinct ids");
}

#[test]
fn symbol_new_desc_returns_unique_ids() {
    let a = __ts_aot_symbol_new_desc(&JsString::from("foo"));
    let b = __ts_aot_symbol_new_desc(&JsString::from("foo"));
    assert_ne!(a, b, "two Symbol('foo') calls must produce distinct ids");
}

#[test]
fn symbol_for_returns_same_id_for_same_key() {
    let a = __ts_aot_symbol_for(&JsString::from("registered"));
    let b = __ts_aot_symbol_for(&JsString::from("registered"));
    assert_eq!(a, b, "Symbol.for(k) must be idempotent for the same key");
}

#[test]
fn symbol_for_returns_different_ids_for_different_keys() {
    let a = __ts_aot_symbol_for(&JsString::from("alpha"));
    let b = __ts_aot_symbol_for(&JsString::from("beta"));
    assert_ne!(
        a, b,
        "Symbol.for with different keys must produce different ids"
    );
}

#[test]
fn symbol_key_for_returns_registered_key() {
    let sym = __ts_aot_symbol_for(&JsString::from("lookup"));
    let key = key_for(sym);
    assert_eq!(key.as_deref(), Some("lookup"));
}

#[test]
fn symbol_key_for_returns_none_for_unregistered_symbol() {
    let fresh = __ts_aot_symbol_new();
    let key = key_for(fresh);
    assert_eq!(
        key, None,
        "keyFor on a non-registered symbol (e.g. fresh Symbol()) must return None per ECMAScript spec"
    );
}

#[test]
fn symbol_for_empty_key_is_registered_and_keyfor_returns_some_empty_string() {
    let sym = __ts_aot_symbol_for(&JsString::from(""));
    let key = key_for(sym);
    assert_eq!(
        key.as_deref(),
        Some(""),
        "Symbol.for(\"\") must register the empty key; keyFor must return Some(\"\") — \
         not None, not Some(anything-else) — to distinguish from a non-registered symbol"
    );
}

#[test]
fn symbol_for_empty_key_repeated_returns_same_id() {
    let a = __ts_aot_symbol_for(&JsString::from(""));
    let b = __ts_aot_symbol_for(&JsString::from(""));
    assert_eq!(
        a, b,
        "Symbol.for(\"\") repeated must be idempotent like any other registered key"
    );
}

#[test]
fn symbol_for_preserves_lone_surrogate_keys() {
    let units = [0xD800u16, 0x0041];
    let a = __ts_aot_symbol_for(&JsString::Raw(&units));
    let b = __ts_aot_symbol_for(&JsString::Raw(&units));
    assert_eq!(
        a, b,
        "Symbol.for must be idempotent for a lone-surrogate key"
    );
    let round_tripped = with_ctx(|ctx| {
        __ts_aot_symbol_key_for(ctx, a)
            .expect("a registered lone-surrogate id must resolve")
            .to_units()
    });
    assert_eq!(
        round_tripped,
        units.to_vec(),
        "keyFor must preserve the full UTF-16 code units of a lone-surrogate key, not collapse \
         them to U+FFFD via a lossy string conversion"
    );
    let other = __ts_aot_symbol_for(&JsString::Raw(&[0xD801u16]));
    assert_ne!(
        a, other,
        "distinct lone-surrogate keys must map to distinct ids"
    );
}

#[test]
fn symbol_for_concurrent_same_key_returns_same_id_and_consistent_reverse_map() {
    let key = JsString::from("concurrent-same-key-7");
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::with_capacity(8);
    for _ in 0..8 {
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            __ts_aot_symbol_for(&key)
        }));
    }
    let mut ids = HashSet::new();
    for h in handles {
        let id = h.join().expect("worker thread panicked");
        ids.insert(id);
    }
    assert_eq!(
        ids.len(),
        1,
        "8 concurrent Symbol.for(\"concurrent-same-key-7\") calls must all return the same id; \
         got {ids:?} (TOCTOU race: lookup-allocate-insert must be atomic under a single mutex)"
    );
    let only_id = *ids.iter().next().expect("set has one element");
    let resolved = key_for(only_id).expect("registered id must resolve");
    assert_eq!(
        resolved, "concurrent-same-key-7",
        "keyFor on the only id must return the registered key (no orphaned ids in reverse map)"
    );
    for other_id in 0..only_id {
        let other_key = key_for(other_id);
        assert_ne!(
            other_key.as_deref(),
            Some("concurrent-same-key-7"),
            "id {other_id} must not alias to the same reverse key (would mean duplicate insert into reverse map)"
        );
    }
}
