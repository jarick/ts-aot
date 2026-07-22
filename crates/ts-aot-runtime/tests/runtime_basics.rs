use std::collections::HashMap;
use ts_aot_runtime::{
    __ts_aot_array_get, __ts_aot_array_len, __ts_aot_array_set, __ts_aot_host_console_log,
    __ts_aot_map_get, __ts_aot_map_set, __ts_aot_op_in, __ts_aot_op_instanceof,
    __ts_aot_string_len, __ts_aot_throw, __ts_aot_typeof, __ts_aot_typeof_null,
    __ts_aot_typeof_unit,
};

#[test]
fn runtime_string_len_returns_utf16_code_unit_count() {
    assert_eq!(__ts_aot_string_len("hello"), 5);
    assert_eq!(__ts_aot_string_len(""), 0);
    assert_eq!(__ts_aot_string_len("café"), 4);
}

#[test]
fn runtime_typeof_dispatches_on_concrete_type() {
    let n_int: i64 = 42;
    let n_float: f64 = 1.5;
    let n_bool: bool = true;
    let n_str: String = "x".to_owned();
    assert_eq!(__ts_aot_typeof(&n_int), "number");
    assert_eq!(__ts_aot_typeof(&n_float), "number");
    assert_eq!(__ts_aot_typeof(&n_bool), "boolean");
    assert_eq!(__ts_aot_typeof(&n_str), "string");
    let arr: Vec<i64> = vec![1, 2, 3];
    assert_eq!(__ts_aot_typeof(&arr), "object");
}

#[test]
fn runtime_typeof_unit_returns_undefined() {
    assert_eq!(__ts_aot_typeof_unit(), "undefined");
}

#[test]
fn runtime_typeof_null_returns_object() {
    assert_eq!(__ts_aot_typeof_null(), "object");
}

#[test]
fn runtime_op_in_array_index_in_range_returns_true() {
    let arr: Vec<i64> = vec![10, 20, 30];
    let idx: i64 = 1;
    assert!(__ts_aot_op_in(&idx, &arr));
}

#[test]
fn runtime_op_in_array_index_out_of_range_returns_false() {
    let arr: Vec<i64> = vec![10, 20, 30];
    let idx: i64 = 5;
    assert!(!__ts_aot_op_in(&idx, &arr));
}

#[test]
fn runtime_op_in_string_in_string_vec_member_returns_true() {
    let arr: Vec<String> = vec!["a".to_owned(), "b".to_owned()];
    let needle: String = "b".to_owned();
    assert!(__ts_aot_op_in(&needle, &arr));
}

#[test]
fn runtime_op_in_string_in_string_vec_non_member_returns_false() {
    let arr: Vec<String> = vec!["a".to_owned(), "b".to_owned()];
    let needle: String = "z".to_owned();
    assert!(!__ts_aot_op_in(&needle, &arr));
}

#[test]
fn runtime_op_in_hashmap_key_present_returns_true() {
    let mut map: HashMap<String, String> = HashMap::new();
    __ts_aot_map_set(&mut map, "k".to_owned(), "v".to_owned());
    let key: String = "k".to_owned();
    assert!(__ts_aot_op_in(&key, &map));
}

#[test]
fn runtime_op_in_hashmap_key_absent_returns_false() {
    let map: HashMap<String, String> = HashMap::new();
    let key: String = "missing".to_owned();
    assert!(!__ts_aot_op_in(&key, &map));
}

#[test]
fn runtime_op_in_non_container_returns_false() {
    let n_int: i64 = 42;
    let n_str: String = "x".to_owned();
    assert!(!__ts_aot_op_in(&n_str, &n_int));
}

#[test]
fn runtime_op_instanceof_matching_class_id_returns_true() {
    let n_int: i64 = 42;
    let target = 0xFFFF_FF03;
    assert!(__ts_aot_op_instanceof(&n_int, target));
}

#[test]
fn runtime_op_instanceof_different_class_id_returns_false() {
    let n_int: i64 = 42;
    let target = 0xFFFF_FF14;
    assert!(!__ts_aot_op_instanceof(&n_int, target));
}

#[test]
fn runtime_op_instanceof_primitive_value_never_matches_struct_id_zero() {
    let n_int: i64 = 42;
    let n_str: String = "x".to_owned();
    let n_bool: bool = true;
    assert!(!__ts_aot_op_instanceof(&n_int, 0));
    assert!(!__ts_aot_op_instanceof(&n_str, 0));
    assert!(!__ts_aot_op_instanceof(&n_bool, 0));
}

#[test]
fn runtime_op_instanceof_primitives_have_distinct_class_ids() {
    let n_int: i64 = 1;
    let n_str: String = "x".to_owned();
    let n_bool: bool = true;
    let n_id = 0xFFFF_FF03;
    let s_id = 0xFFFF_FF0E;
    let b_id = 0xFFFF_FF0C;
    assert!(__ts_aot_op_instanceof(&n_int, n_id));
    assert!(!__ts_aot_op_instanceof(&n_int, s_id));
    assert!(!__ts_aot_op_instanceof(&n_int, b_id));
    assert!(__ts_aot_op_instanceof(&n_str, s_id));
    assert!(!__ts_aot_op_instanceof(&n_str, n_id));
    assert!(__ts_aot_op_instanceof(&n_bool, b_id));
}

#[test]
fn runtime_throw_helper_panics_with_string_payload() {
    let result = std::panic::catch_unwind(|| __ts_aot_throw("oops".to_owned()));
    let err = result.expect_err("__ts_aot_throw must panic");
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("oops"),
        "panic payload must contain 'oops', got: {msg}"
    );
}

#[test]
fn runtime_array_get_set_and_len() {
    let mut arr: Vec<i64> = Vec::new();
    assert_eq!(__ts_aot_array_len(&arr), 0);
    arr.push(0);
    arr.push(0);
    arr.push(0);
    let wrote = __ts_aot_array_set(&mut arr, 1, 99);
    assert!(wrote);
    let got = __ts_aot_array_get(&arr, 1);
    assert_eq!(got, Some(99));
    assert_eq!(__ts_aot_array_len(&arr), 3);
}

#[test]
fn runtime_map_get_returns_stored_value() {
    let mut map: HashMap<String, String> = HashMap::new();
    __ts_aot_map_set(&mut map, "k".to_owned(), "v".to_owned());
    assert_eq!(__ts_aot_map_get(&map, "k").as_deref(), Some("v"));
    assert_eq!(__ts_aot_map_get(&map, "missing"), None);
}

#[test]
fn runtime_host_console_log_does_not_panic() {
    __ts_aot_host_console_log("hello from runtime_basics");
}
