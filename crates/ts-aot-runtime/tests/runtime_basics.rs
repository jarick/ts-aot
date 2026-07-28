use indexmap::IndexMap;
use ts_aot_runtime::{
    __ts_aot_array_create_with_len, __ts_aot_array_from, __ts_aot_array_from_length_mapped,
    __ts_aot_array_from_mapped, __ts_aot_array_from_string, __ts_aot_array_get,
    __ts_aot_array_is_array, __ts_aot_array_is_array_false, __ts_aot_array_len,
    __ts_aot_array_push, __ts_aot_array_set, __ts_aot_host_console_log, __ts_aot_map_get,
    __ts_aot_map_set, __ts_aot_op_in, __ts_aot_op_instanceof, __ts_aot_string_len, __ts_aot_throw,
    __ts_aot_typeof, __ts_aot_typeof_null, __ts_aot_typeof_unit, TsArrayMarker,
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
fn runtime_op_in_indexmap_key_present_returns_true() {
    let mut map: IndexMap<String, String> = IndexMap::new();
    __ts_aot_map_set(&mut map, "k".to_owned(), "v".to_owned());
    let key: String = "k".to_owned();
    assert!(__ts_aot_op_in(&key, &map));
}

#[test]
fn runtime_op_in_indexmap_key_absent_returns_false() {
    let map: IndexMap<String, String> = IndexMap::new();
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
fn runtime_array_is_array_returns_true_for_vec() {
    let arr: Vec<i64> = vec![1, 2, 3];
    assert!(
        __ts_aot_array_is_array(&arr),
        "Vec<i64> must be detected as array, got false"
    );
}

#[test]
fn runtime_array_is_array_false_helper_returns_false() {
    assert!(!__ts_aot_array_is_array_false());
}

#[test]
fn runtime_array_is_array_returns_true_for_vec_of_marker() {
    let arr: Vec<TsArrayMarker> = vec![TsArrayMarker, TsArrayMarker];
    assert!(
        __ts_aot_array_is_array(&arr),
        "Vec<TsArrayMarker> must be detected as array via the explicit marker"
    );
    let marker = TsArrayMarker;
    assert!(
        __ts_aot_array_is_array(&marker),
        "TsArrayMarker itself must be detected as array"
    );
}

#[test]
fn runtime_array_is_array_returns_true_for_nested_vec() {
    let nested: Vec<Vec<i64>> = vec![vec![1, 2], vec![3, 4]];
    assert!(
        __ts_aot_array_is_array(&nested),
        "Vec<Vec<i64>> must be detected as array (generic Vec<T> blanket impl)"
    );
}

#[test]
fn runtime_array_is_array_returns_false_for_non_vec() {
    assert!(!__ts_aot_array_is_array_false());
    let map: IndexMap<String, String> = IndexMap::new();
    let _ = map;
}

#[test]
fn runtime_array_create_with_len_zero_returns_empty_vec() {
    let v: Vec<i64> = __ts_aot_array_create_with_len(0);
    assert!(v.is_empty());
    assert_eq!(__ts_aot_array_len(&v), 0);
}

#[test]
fn runtime_array_create_with_len_fills_with_default() {
    let v: Vec<i64> = __ts_aot_array_create_with_len(3);
    assert_eq!(__ts_aot_array_len(&v), 3);
    assert_eq!(v, vec![0, 0, 0]);
    let s: Vec<String> = __ts_aot_array_create_with_len(2);
    assert_eq!(s, vec![String::new(), String::new()]);
}

#[test]
#[should_panic(expected = "out-of-range length")]
fn runtime_array_create_with_len_negative_panics() {
    let _v: Vec<i64> = __ts_aot_array_create_with_len(-1);
}

#[test]
#[should_panic(expected = "out-of-range length")]
fn runtime_array_create_with_len_exceeding_max_dense_len_panics() {
    let _v: Vec<i64> =
        __ts_aot_array_create_with_len(i64::from(ts_aot_runtime::MAX_DENSE_ARRAY_LEN) + 1);
}

#[test]
#[should_panic(expected = "out-of-range length")]
fn runtime_array_create_with_len_at_max_dense_len_boundary_panics() {
    let _v: Vec<i64> =
        __ts_aot_array_create_with_len(i64::from(ts_aot_runtime::MAX_DENSE_ARRAY_LEN));
}

#[test]
fn runtime_array_create_with_len_just_below_max_dense_len_succeeds() {
    let n = i64::from(ts_aot_runtime::MAX_DENSE_ARRAY_LEN) - 1;
    let v: Vec<u8> = __ts_aot_array_create_with_len(n);
    assert_eq!(i64::try_from(v.len()).unwrap(), n);
}

#[test]
fn runtime_array_push_appends_to_vec() {
    let mut v: Vec<i64> = Vec::new();
    __ts_aot_array_push(&mut v, 1);
    __ts_aot_array_push(&mut v, 2);
    __ts_aot_array_push(&mut v, 3);
    assert_eq!(v, vec![1, 2, 3]);
}

#[test]
fn runtime_array_from_clones_source_vec() {
    let src: Vec<i64> = vec![1, 2, 3];
    let dst: Vec<i64> = __ts_aot_array_from(&src);
    assert_eq!(dst, src);
    assert_eq!(__ts_aot_array_len(&src), 3);
    assert_eq!(__ts_aot_array_len(&dst), 3);
    let str_vec: Vec<String> = vec!["a".to_owned(), "b".to_owned()];
    let str_dst: Vec<String> = __ts_aot_array_from(&str_vec);
    assert_eq!(str_dst, vec!["a".to_owned(), "b".to_owned()]);
}

#[test]
fn runtime_array_from_string_returns_code_point_strings() {
    let scalars: Vec<String> = __ts_aot_array_from_string("abc");
    assert_eq!(
        scalars,
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
    );
    let empty: Vec<String> = __ts_aot_array_from_string("");
    assert!(empty.is_empty());
    let cafe: Vec<String> = __ts_aot_array_from_string("café");
    assert_eq!(
        cafe,
        vec![
            "c".to_owned(),
            "a".to_owned(),
            "f".to_owned(),
            "é".to_owned()
        ]
    );
}

#[test]
fn runtime_array_from_string_with_astral_char_yields_one_string_element() {
    let scalars: Vec<String> = __ts_aot_array_from_string("😀");
    assert_eq!(
        scalars.len(),
        1,
        "astral char must yield 1 Unicode scalar element (not 2 UTF-16 code units), got {scalars:?}"
    );
    assert_eq!(scalars[0], "😀");
}

#[test]
fn runtime_array_from_mapped_applies_function_to_each_element() {
    let src: Vec<i64> = vec![1, 2, 3];
    let doubled: Vec<i64> = __ts_aot_array_from_mapped(&src, |x, _i| x * 2);
    assert_eq!(doubled, vec![2, 4, 6]);
    let neg: Vec<i64> = __ts_aot_array_from_mapped(&src, |x, _i| -x);
    assert_eq!(neg, vec![-1, -2, -3]);
    let str_src: Vec<String> = vec!["a".to_owned(), "b".to_owned()];
    let upper: Vec<String> = __ts_aot_array_from_mapped(&str_src, |s, _i| s.to_uppercase());
    assert_eq!(upper, vec!["A".to_owned(), "B".to_owned()]);
}

#[test]
fn runtime_array_from_mapped_passes_zero_based_index_to_mapper() {
    let src: Vec<i64> = vec![10, 20, 30];
    let with_index: Vec<i64> = __ts_aot_array_from_mapped(&src, |x, i| x + i);
    assert_eq!(with_index, vec![10, 21, 32]);
    let only_index: Vec<i64> = __ts_aot_array_from_mapped(&src, |_x, i| i);
    assert_eq!(only_index, vec![0, 1, 2]);
    let empty: Vec<i64> = __ts_aot_array_from_mapped::<i64, i64, _>(&[], |_x, i| i * 10);
    assert!(empty.is_empty());
}

#[test]
fn runtime_array_from_length_mapped_calls_mapfn_with_index() {
    let result: Vec<i64> = __ts_aot_array_from_length_mapped::<i64, i64, _>(3, |_v, i| i * 2);
    assert_eq!(result, vec![0, 2, 4]);
    let empty: Vec<i64> = __ts_aot_array_from_length_mapped::<i64, i64, _>(0, |_v, _i| 99);
    assert!(empty.is_empty());
    let str_result: Vec<String> =
        __ts_aot_array_from_length_mapped::<(), String, _>(2, |_v, i| format!("idx{i}"));
    assert_eq!(str_result, vec!["idx0".to_owned(), "idx1".to_owned()]);
}

#[test]
#[should_panic(expected = "out-of-range length")]
fn runtime_array_from_length_mapped_negative_length_panics() {
    let _v: Vec<i64> = __ts_aot_array_from_length_mapped::<i64, i64, _>(-1, |_v, _i| 0);
}

#[test]
#[should_panic(expected = "out-of-range length")]
fn runtime_array_from_length_mapped_at_max_dense_len_boundary_panics() {
    let _v: Vec<i64> = __ts_aot_array_from_length_mapped::<i64, i64, _>(
        i64::from(ts_aot_runtime::MAX_DENSE_ARRAY_LEN),
        |_v, _i| 0,
    );
}

#[test]
fn runtime_array_from_mapped_accepts_fnmut_with_mutable_state() {
    let src: Vec<i64> = vec![10, 20, 30];
    let mut counter: i64 = 0;
    let result: Vec<i64> = __ts_aot_array_from_mapped(&src, |x, _i| {
        counter += x;
        x + counter
    });
    assert_eq!(
        result,
        vec![20, 50, 90],
        "FnMut closure must mutate captured state across calls (counter: 0->10->30->60, returns x+counter: 10+10=20, 20+30=50, 30+60=90)"
    );
}

#[test]
fn runtime_array_from_length_mapped_accepts_fnmut_with_mutable_state() {
    let mut counter: i64 = 0;
    let result: Vec<i64> = __ts_aot_array_from_length_mapped::<i64, i64, _>(3, |_v, _i| {
        counter += 1;
        counter
    });
    assert_eq!(
        result,
        vec![1, 2, 3],
        "FnMut closure must mutate captured state across calls (counter: 0->1->2->3)"
    );
}

#[test]
fn runtime_map_get_returns_stored_value() {
    let mut map: IndexMap<String, String> = IndexMap::new();
    __ts_aot_map_set(&mut map, "k".to_owned(), "v".to_owned());
    assert_eq!(__ts_aot_map_get(&map, "k").as_deref(), Some("v"));
    assert_eq!(__ts_aot_map_get(&map, "missing"), None);
}

#[test]
fn runtime_host_console_log_does_not_panic() {
    __ts_aot_host_console_log("hello from runtime_basics");
}
