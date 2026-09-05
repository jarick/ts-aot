use std::rc::Rc;

use ts_aot_runtime::{
    __ts_aot_weak_map_clear, __ts_aot_weak_map_delete, __ts_aot_weak_map_get,
    __ts_aot_weak_map_has, __ts_aot_weak_map_new, __ts_aot_weak_map_set,
};

fn pin() -> Rc<()> {
    Rc::new(())
}

fn key_of<T>(value: &T) -> *const () {
    std::ptr::from_ref::<T>(value).cast::<()>()
}

#[test]
fn weak_map_set_get_round_trip() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 1i64;
    let ptr = key_of(&k);
    assert_eq!(__ts_aot_weak_map_set(&m, &liveness, ptr, 100), 1);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), Some(100));
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 1);
}

#[test]
fn weak_map_overwrite_existing_key() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 7i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, 1);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, 2);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), Some(2));
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 1);
}

#[test]
fn weak_map_get_missing_key_returns_none() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 42i64;
    let ptr = key_of(&k);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), None);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 0);
}

#[test]
fn weak_map_delete_existing_key_returns_true() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 5i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, 99);
    assert_eq!(__ts_aot_weak_map_delete(&m, &liveness, ptr), 1);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 0);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), None);
}

#[test]
fn weak_map_delete_missing_key_returns_false() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 100i64;
    let ptr = key_of(&k);
    assert_eq!(__ts_aot_weak_map_delete(&m, &liveness, ptr), 0);
}

#[test]
fn weak_map_clear_empties_all_entries() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k1 = 1i64;
    let k2 = 2i64;
    let k3 = 3i64;
    let p1 = key_of(&k1);
    let p2 = key_of(&k2);
    let p3 = key_of(&k3);
    let _ = __ts_aot_weak_map_set(&m, &liveness, p1, 10);
    let _ = __ts_aot_weak_map_set(&m, &liveness, p2, 20);
    let _ = __ts_aot_weak_map_set(&m, &liveness, p3, 30);
    __ts_aot_weak_map_clear(&m);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, p1), 0);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, p2), 0);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, p3), 0);
}

#[test]
fn weak_map_independent_instances() {
    let m1: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let m2: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 1i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m1, &liveness, ptr, 100);
    assert_eq!(__ts_aot_weak_map_get(&m1, &liveness, ptr), Some(100));
    assert_eq!(__ts_aot_weak_map_get(&m2, &liveness, ptr), None);
}

#[test]
fn weak_map_purges_entries_when_liveness_drops() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    {
        let liveness = pin();
        let k = 1i64;
        let ptr = key_of(&k);
        let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, 100);
        assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), Some(100));
    }
    let liveness2 = pin();
    let k2 = 1i64;
    let ptr2 = key_of(&k2);
    assert_eq!(
        __ts_aot_weak_map_get(&m, &liveness2, ptr2),
        None,
        "transient entry must be purged once its liveness binding is dropped"
    );
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness2, ptr2), 0);
    let _ = __ts_aot_weak_map_set(&m, &liveness2, ptr2, 999);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness2, ptr2), Some(999));
}

#[test]
fn weak_map_stored_negative_one_is_distinct_from_absent() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 1i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, -1);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 1);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), Some(-1));
    assert_ne!(__ts_aot_weak_map_get(&m, &liveness, ptr), None);
    let other = 2i64;
    let other_ptr = key_of(&other);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, other_ptr), None);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, other_ptr), 0);
}

#[test]
fn weak_map_stored_i64_min_is_distinct_from_absent() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 1i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, i64::MIN);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 1);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, ptr), Some(i64::MIN));
    assert_ne!(__ts_aot_weak_map_get(&m, &liveness, ptr), None);
    let other = 2i64;
    let other_ptr = key_of(&other);
    assert_eq!(__ts_aot_weak_map_get(&m, &liveness, other_ptr), None);
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, other_ptr), 0);
}

#[test]
fn weak_map_supports_string_values() {
    let m: ts_aot_runtime::WeakMapHandle<String> = __ts_aot_weak_map_new();
    let liveness = pin();
    let k = 1i64;
    let ptr = key_of(&k);
    let _ = __ts_aot_weak_map_set(&m, &liveness, ptr, String::from("hello"));
    assert_eq!(__ts_aot_weak_map_has(&m, &liveness, ptr), 1);
    assert_eq!(
        __ts_aot_weak_map_get(&m, &liveness, ptr),
        Some(String::from("hello"))
    );
}

#[test]
fn weak_map_purges_dropped_key_on_address_reuse() {
    let m: ts_aot_runtime::WeakMapHandle<i64> = __ts_aot_weak_map_new();
    let k1 = 1i64;
    let p1 = key_of(&k1);
    {
        let liveness = pin();
        let _ = __ts_aot_weak_map_set(&m, &liveness, p1, 100);
    }
    let liveness2 = pin();
    assert_eq!(
        __ts_aot_weak_map_get(&m, &liveness2, p1),
        None,
        "after the first liveness scope ends, the stale entry under the original pointer p1 must be purged and unreachable from a fresh liveness binding"
    );
    let _ = __ts_aot_weak_map_set(&m, &liveness2, p1, 999);
    assert_eq!(
        __ts_aot_weak_map_get(&m, &liveness2, p1),
        Some(999),
        "reusing the original pointer p1 with a fresh liveness must register a fresh entry under that address, and that entry must be the only one present"
    );
}
