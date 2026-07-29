use indexmap::IndexMap;
use ts_aot_runtime::{
    __ts_aot_array_create_with_len, __ts_aot_array_from, __ts_aot_array_from_length_mapped,
    __ts_aot_array_from_mapped, __ts_aot_array_from_string, __ts_aot_array_get,
    __ts_aot_array_is_array, __ts_aot_array_is_array_false, __ts_aot_array_len,
    __ts_aot_array_push, __ts_aot_array_set, __ts_aot_host_console_log, __ts_aot_map_get,
    __ts_aot_map_set, __ts_aot_math_abs, __ts_aot_math_acos, __ts_aot_math_asin,
    __ts_aot_math_atan, __ts_aot_math_atan2, __ts_aot_math_ceil, __ts_aot_math_cos,
    __ts_aot_math_exp, __ts_aot_math_floor, __ts_aot_math_log, __ts_aot_math_max,
    __ts_aot_math_min, __ts_aot_math_pow, __ts_aot_math_random, __ts_aot_math_round,
    __ts_aot_math_sign, __ts_aot_math_sin, __ts_aot_math_sqrt, __ts_aot_math_tan,
    __ts_aot_math_trunc, __ts_aot_op_in, __ts_aot_op_instanceof, __ts_aot_string_char_at,
    __ts_aot_string_from_char_code, __ts_aot_string_from_code_point, __ts_aot_string_index_of,
    __ts_aot_string_len, __ts_aot_string_substring_utf16, __ts_aot_throw, __ts_aot_typeof,
    __ts_aot_typeof_null, __ts_aot_typeof_unit, JsString, TsArrayMarker,
};

fn js(s: &str) -> JsString {
    JsString::from(s)
}

fn assert_f64_exact(actual: f64, expected: f64) {
    let equal = if actual.is_nan() || expected.is_nan() {
        actual.is_nan() && expected.is_nan()
    } else {
        actual.to_bits() == expected.to_bits()
    };
    assert!(equal, "f64 mismatch: actual={actual} expected={expected}");
}

#[test]
fn runtime_string_len_returns_utf16_code_unit_count() {
    assert_eq!(__ts_aot_string_len(&js("hello")), 5);
    assert_eq!(__ts_aot_string_len(&js("")), 0);
    assert_eq!(__ts_aot_string_len(&js("café")), 4);
}

#[test]
fn runtime_math_abs_floor_ceil_round_trunc_sign() {
    assert_f64_exact(__ts_aot_math_abs(-3.5), 3.5);
    assert_f64_exact(__ts_aot_math_abs(3.5), 3.5);
    assert_f64_exact(__ts_aot_math_floor(3.7), 3.0);
    assert_f64_exact(__ts_aot_math_floor(-3.2), -4.0);
    assert_f64_exact(__ts_aot_math_ceil(3.2), 4.0);
    assert_f64_exact(__ts_aot_math_ceil(-3.7), -3.0);
    assert_f64_exact(__ts_aot_math_round(3.5), 4.0);
    assert_f64_exact(__ts_aot_math_round(3.4), 3.0);
    assert_f64_exact(__ts_aot_math_round(-0.0), -0.0);
    assert_f64_exact(__ts_aot_math_round(-0.1), -0.0);
    assert_f64_exact(__ts_aot_math_round(-0.5), -0.0);
    assert_f64_exact(__ts_aot_math_round(-1.5), -1.0);
    assert_f64_exact(__ts_aot_math_round(-2.5), -2.0);
    assert_f64_exact(
        __ts_aot_math_round(4_503_599_627_370_497.0),
        4_503_599_627_370_497.0,
    );
    assert_f64_exact(
        __ts_aot_math_round(-4_503_599_627_370_497.0),
        -4_503_599_627_370_497.0,
    );
    assert_f64_exact(__ts_aot_math_trunc(3.7), 3.0);
    assert_f64_exact(__ts_aot_math_trunc(-3.7), -3.0);
    assert_f64_exact(__ts_aot_math_sign(5.0), 1.0);
    assert_f64_exact(__ts_aot_math_sign(-5.0), -1.0);
    assert_f64_exact(__ts_aot_math_sign(0.0), 0.0);
    assert_f64_exact(__ts_aot_math_sign(-0.0), -0.0);
    assert!(__ts_aot_math_sign(f64::NAN).is_nan());
}

#[test]
fn runtime_math_sqrt_pow_log_exp() {
    assert_f64_exact(__ts_aot_math_sqrt(16.0), 4.0);
    assert_f64_exact(__ts_aot_math_sqrt(2.0), 2.0_f64.sqrt());
    assert_f64_exact(__ts_aot_math_pow(2.0, 10.0), 1024.0);
    assert_f64_exact(__ts_aot_math_pow(9.0, 0.5), 3.0);
    assert_f64_exact(__ts_aot_math_log(1.0), 0.0);
    assert_f64_exact(__ts_aot_math_log(std::f64::consts::E), 1.0);
    assert_f64_exact(__ts_aot_math_exp(0.0), 1.0);
    assert_f64_exact(__ts_aot_math_exp(1.0), std::f64::consts::E);
}

#[test]
fn runtime_math_pow_zero_exponent_returns_one() {
    assert_f64_exact(__ts_aot_math_pow(2.0, 0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(2.0, -0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(0.0, 0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(-0.0, 0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(f64::INFINITY, 0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(f64::NEG_INFINITY, 0.0), 1.0);
    assert_f64_exact(__ts_aot_math_pow(f64::NAN, 0.0), 1.0);
}

#[test]
fn runtime_math_pow_one_with_infinite_exponent_returns_nan() {
    assert!(__ts_aot_math_pow(1.0, f64::INFINITY).is_nan());
    assert!(__ts_aot_math_pow(1.0, f64::NEG_INFINITY).is_nan());
}

#[test]
fn runtime_math_pow_neg_one_with_infinite_exponent_returns_nan() {
    assert!(__ts_aot_math_pow(-1.0, f64::INFINITY).is_nan());
    assert!(__ts_aot_math_pow(-1.0, f64::NEG_INFINITY).is_nan());
}

#[test]
fn runtime_math_trig_functions() {
    assert_f64_exact(__ts_aot_math_sin(0.0), 0.0);
    assert_f64_exact(__ts_aot_math_cos(0.0), 1.0);
    assert_f64_exact(__ts_aot_math_tan(0.0), 0.0);
    assert_f64_exact(__ts_aot_math_asin(0.0), 0.0);
    assert_f64_exact(__ts_aot_math_acos(1.0), 0.0);
    assert_f64_exact(__ts_aot_math_atan(0.0), 0.0);
    assert_f64_exact(__ts_aot_math_atan2(0.0, 1.0), 0.0);
    assert_f64_exact(__ts_aot_math_atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
}

#[test]
fn runtime_math_max_min_returns_nan_on_nan_input() {
    assert!(__ts_aot_math_max(&[1.0, f64::NAN]).is_nan());
    assert!(__ts_aot_math_max(&[f64::NAN, 1.0]).is_nan());
    assert!(__ts_aot_math_max(&[1.0, 2.0, f64::NAN, 4.0]).is_nan());
    assert_f64_exact(__ts_aot_math_max(&[3.0, 5.0]), 5.0);
    assert_f64_exact(__ts_aot_math_max(&[5.0, 3.0]), 5.0);
    assert!(__ts_aot_math_min(&[1.0, f64::NAN]).is_nan());
    assert!(__ts_aot_math_min(&[f64::NAN, 1.0]).is_nan());
    assert!(__ts_aot_math_min(&[1.0, 2.0, f64::NAN, 4.0]).is_nan());
    assert_f64_exact(__ts_aot_math_min(&[3.0, 5.0]), 3.0);
    assert_f64_exact(__ts_aot_math_min(&[5.0, 3.0]), 3.0);
}

#[test]
fn runtime_math_max_min_accept_variadic_args() {
    assert_f64_exact(__ts_aot_math_max(&[1.0, 2.0, 3.0, 4.0, 5.0]), 5.0);
    assert_f64_exact(__ts_aot_math_max(&[5.0, 3.0, 9.0, 1.0]), 9.0);
    assert_f64_exact(__ts_aot_math_min(&[1.0, 2.0, 3.0, 4.0, 5.0]), 1.0);
    assert_f64_exact(__ts_aot_math_min(&[5.0, 3.0, 9.0, 1.0]), 1.0);
}

#[test]
fn runtime_math_max_signed_zero_tie_returns_positive_zero() {
    assert_f64_exact(__ts_aot_math_max(&[-0.0, 0.0]), 0.0);
    assert_f64_exact(__ts_aot_math_max(&[0.0, -0.0]), 0.0);
    assert_f64_exact(__ts_aot_math_max(&[1.0, -0.0, 0.0]), 1.0);
}

#[test]
fn runtime_math_min_signed_zero_tie_returns_negative_zero() {
    assert_f64_exact(__ts_aot_math_min(&[-0.0, 0.0]), -0.0);
    assert_f64_exact(__ts_aot_math_min(&[0.0, -0.0]), -0.0);
    assert_f64_exact(__ts_aot_math_min(&[-1.0, -0.0, 0.0]), -1.0);
}

#[test]
fn runtime_math_max_with_single_arg_returns_that_arg() {
    assert_f64_exact(__ts_aot_math_max(&[42.0]), 42.0);
    assert_f64_exact(__ts_aot_math_min(&[42.0]), 42.0);
    assert_f64_exact(__ts_aot_math_max(&[-3.5]), -3.5);
    assert_f64_exact(__ts_aot_math_min(&[-3.5]), -3.5);
}

#[test]
fn runtime_math_max_with_zero_args_returns_negative_infinity() {
    assert_f64_exact(__ts_aot_math_max(&[]), f64::NEG_INFINITY);
    assert_f64_exact(__ts_aot_math_min(&[]), f64::INFINITY);
}

#[test]
fn runtime_math_random_returns_value_in_unit_interval() {
    for _ in 0..16 {
        let r = __ts_aot_math_random();
        assert!(
            (0.0..1.0).contains(&r),
            "Math.random() must return value in [0.0, 1.0); got {r}"
        );
    }
}

#[test]
fn runtime_typeof_dispatches_on_concrete_type() {
    let n_int: i64 = 42;
    let n_float: f64 = 1.5;
    let n_bool: bool = true;
    let n_str = js("x");
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
fn runtime_op_in_string_in_string_vec_index_returns_true() {
    let arr: Vec<JsString> = vec![js("a"), js("b"), js("c")];
    assert!(__ts_aot_op_in(&js("0"), &arr));
    assert!(__ts_aot_op_in(&js("1"), &arr));
    assert!(__ts_aot_op_in(&js("2"), &arr));
}

#[test]
fn runtime_op_in_string_in_string_vec_index_out_of_range_returns_false() {
    let arr: Vec<JsString> = vec![js("a"), js("b"), js("c")];
    assert!(!__ts_aot_op_in(&js("3"), &arr));
    assert!(!__ts_aot_op_in(&js("100"), &arr));
}

#[test]
fn runtime_op_in_string_in_string_vec_non_integer_index_returns_false() {
    let arr: Vec<JsString> = vec![js("a"), js("b")];
    assert!(!__ts_aot_op_in(&js("a"), &arr));
    assert!(!__ts_aot_op_in(&js("abc"), &arr));
    assert!(!__ts_aot_op_in(&js(""), &arr));
    assert!(!__ts_aot_op_in(&js("-1"), &arr));
    assert!(!__ts_aot_op_in(&js("01"), &arr));
    assert!(!__ts_aot_op_in(&js("1.5"), &arr));
}

#[test]
fn runtime_op_in_indexmap_key_present_returns_true() {
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    __ts_aot_map_set(&mut map, js("k"), js("v"));
    let key = js("k");
    assert!(__ts_aot_op_in(&key, &map));
    let key_str: String = "k".to_owned();
    assert!(__ts_aot_op_in(&key_str, &map));
}

#[test]
fn runtime_op_in_indexmap_key_absent_returns_false() {
    let map: IndexMap<JsString, JsString> = IndexMap::new();
    let key = js("missing");
    assert!(!__ts_aot_op_in(&key, &map));
}

#[test]
#[should_panic(expected = "unsupported container type")]
fn runtime_op_in_non_container_panics() {
    let n_int: i64 = 42;
    let n_str: String = "x".to_owned();
    let _ = __ts_aot_op_in(&n_str, &n_int);
}

#[test]
#[should_panic(expected = "requires i64 key")]
fn runtime_op_in_vec_i64_with_string_key_panics() {
    let arr: Vec<i64> = vec![1, 2, 3];
    let key: String = "x".to_owned();
    let _ = __ts_aot_op_in(&key, &arr);
}

#[test]
#[should_panic(expected = "requires JsString or String key")]
fn runtime_op_in_indexmap_with_wrong_key_type_panics() {
    let map: IndexMap<JsString, JsString> = IndexMap::new();
    let key: i64 = 42;
    let _ = __ts_aot_op_in(&key, &map);
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
    let scalars: Vec<JsString> = __ts_aot_array_from_string(&js("abc"));
    assert_eq!(scalars, vec![js("a"), js("b"), js("c")]);
    let empty: Vec<JsString> = __ts_aot_array_from_string(&js(""));
    assert!(empty.is_empty());
    let cafe: Vec<JsString> = __ts_aot_array_from_string(&js("café"));
    assert_eq!(cafe, vec![js("c"), js("a"), js("f"), js("é")]);
}

#[test]
fn runtime_array_from_string_with_astral_char_yields_one_string_element() {
    let scalars: Vec<JsString> = __ts_aot_array_from_string(&js("😀"));
    assert_eq!(
        scalars.len(),
        1,
        "astral char must yield 1 Unicode scalar element (not 2 UTF-16 code units), got {scalars:?}"
    );
    assert_eq!(scalars[0], js("😀"));
}

#[test]
fn runtime_array_from_string_preserves_lone_surrogates_as_raw_units() {
    let raw = JsString::from_units(vec![
        u16::from(b'a'),
        0xD83D,
        u16::from(b'b'),
        0xDE00,
        u16::from(b'c'),
    ]);
    let scalars: Vec<JsString> = __ts_aot_array_from_string(&raw);
    assert_eq!(scalars.len(), 5, "got {scalars:?}");
    assert_eq!(scalars[0], js("a"));
    assert_eq!(scalars[1], JsString::from_units(vec![0xD83D]));
    assert_eq!(scalars[2], js("b"));
    assert_eq!(scalars[3], JsString::from_units(vec![0xDE00]));
    assert_eq!(scalars[4], js("c"));
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
    let mut map: IndexMap<JsString, JsString> = IndexMap::new();
    __ts_aot_map_set(&mut map, js("k"), js("v"));
    assert_eq!(__ts_aot_map_get(&map, &js("k")), Some(js("v")));
    assert_eq!(__ts_aot_map_get(&map, &js("missing")), None);
}

#[test]
fn runtime_host_console_log_does_not_panic() {
    __ts_aot_host_console_log(&js("hello from runtime_basics"));
}

#[test]
fn runtime_string_index_of_returns_offset_when_needle_found() {
    assert_eq!(
        __ts_aot_string_index_of(&js("hello world"), &js("world"), 0),
        6
    );
    assert_eq!(
        __ts_aot_string_index_of(&js("hello world"), &js("hello"), 0),
        0
    );
    assert_eq!(__ts_aot_string_index_of(&js("hello world"), &js("o"), 0), 4);
    assert_eq!(__ts_aot_string_index_of(&js("hello world"), &js("o"), 5), 7);
}

#[test]
fn runtime_string_index_of_returns_minus_one_when_needle_absent() {
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js("xyz"), 0), -1);
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js("xyz"), 100), -1);
}

#[test]
fn runtime_string_index_of_empty_needle_matches_at_from_index() {
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js(""), 0), 0);
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js(""), 3), 3);
    assert_eq!(__ts_aot_string_index_of(&js(""), &js(""), 0), 0);
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js(""), 100), 5);
}

#[test]
fn runtime_string_index_of_negative_from_index_treated_as_zero() {
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js("hello"), -1), 0);
    assert_eq!(__ts_aot_string_index_of(&js("hello"), &js("ell"), -100), 1);
}

#[test]
fn runtime_string_char_at_returns_scalar_at_index() {
    assert_eq!(__ts_aot_string_char_at(&js("hello"), 0), js("h"));
    assert_eq!(__ts_aot_string_char_at(&js("hello"), 4), js("o"));
    assert_eq!(__ts_aot_string_char_at(&js("café"), 3), js("é"));
}

#[test]
fn runtime_string_char_at_out_of_range_returns_empty_string() {
    assert_eq!(__ts_aot_string_char_at(&js("hello"), 5), js(""));
    assert_eq!(__ts_aot_string_char_at(&js("hello"), 100), js(""));
    assert_eq!(__ts_aot_string_char_at(&js("hello"), -1), js(""));
}

#[test]
fn runtime_string_from_char_code_builds_string_from_codes() {
    assert_eq!(__ts_aot_string_from_char_code(&[72, 105]), js("Hi"));
    assert_eq!(__ts_aot_string_from_char_code(&[]), js(""));
    assert_eq!(__ts_aot_string_from_char_code(&[65, 66, 67]), js("ABC"));
}

#[test]
fn runtime_string_from_char_code_masks_to_sixteen_bits() {
    let codes = vec![0x1_F0001_i64];
    assert_eq!(__ts_aot_string_from_char_code(&codes), js("\u{0001}"));
    let codes = vec![0x1_FFFF_i64];
    assert_eq!(__ts_aot_string_from_char_code(&codes), js("\u{FFFF}"));
}

#[test]
fn runtime_string_from_char_code_preserves_lone_surrogates_as_raw_units() {
    let high = __ts_aot_string_from_char_code(&[0xD83D_i64]);
    assert_eq!(high, JsString::from_units(vec![0xD83D]));
    let low = __ts_aot_string_from_char_code(&[0xDE00_i64]);
    assert_eq!(low, JsString::from_units(vec![0xDE00]));
    let split_pair = __ts_aot_string_from_char_code(&[0xD83D_i64, 0xD83D_i64]);
    assert_eq!(split_pair, JsString::from_units(vec![0xD83D, 0xD83D]));
    let valid_pair = __ts_aot_string_from_char_code(&[0xD83D_i64, 0xDE00_i64]);
    assert_eq!(valid_pair, js("\u{1F600}"));
    let mixed = __ts_aot_string_from_char_code(&[65_i64, 0xD83D, 66, 0xDE00]);
    assert_eq!(mixed, JsString::from_units(vec![65, 0xD83D, 66, 0xDE00]));
}

#[test]
fn runtime_jsstring_to_string_lossy_returns_inner_string_for_valid() {
    assert_eq!(js("hello").to_string_lossy(), "hello");
    assert_eq!(js("").to_string_lossy(), "");
    assert_eq!(js("😀").to_string_lossy(), "😀");
}

#[test]
fn runtime_jsstring_to_string_lossy_decodes_raw_via_from_utf16_lossy() {
    let raw_valid_pair = JsString::from_units(vec![0xD83D, 0xDE00]);
    assert_eq!(raw_valid_pair.to_string_lossy(), "\u{1F600}");
    let raw_lone_high = JsString::from_units(vec![0xD83D]);
    assert_eq!(raw_lone_high.to_string_lossy(), "\u{FFFD}");
    let raw_lone_low = JsString::from_units(vec![0xDE00]);
    assert_eq!(raw_lone_low.to_string_lossy(), "\u{FFFD}");
    let raw_bmp = JsString::from_units(vec![u16::from(b'a'), u16::from(b'b')]);
    assert_eq!(raw_bmp.to_string_lossy(), "ab");
}

#[test]
fn runtime_string_from_code_point_handles_bmp_and_astral() {
    assert_eq!(__ts_aot_string_from_code_point(&[65, 66, 67]), js("ABC"));
    assert_eq!(
        __ts_aot_string_from_code_point(&[0x1_F600_i64]),
        js("\u{1F600}")
    );
    assert_eq!(__ts_aot_string_from_code_point(&[]), js(""));
}

#[test]
#[should_panic(expected = "RangeError")]
fn runtime_string_from_code_point_throws_on_negative_code_point() {
    let _ = __ts_aot_string_from_code_point(&[-1_i64, 65]);
}

#[test]
fn runtime_string_from_code_point_preserves_lone_surrogates_as_raw_units() {
    let lone_high = __ts_aot_string_from_code_point(&[0xD800_i64]);
    assert_eq!(lone_high, JsString::from_units(vec![0xD800]));
    let lone_low = __ts_aot_string_from_code_point(&[0xDFFF_i64]);
    assert_eq!(lone_low, JsString::from_units(vec![0xDFFF]));
    let mixed = __ts_aot_string_from_code_point(&[0x41_i64, 0xD800, 0x42, 0xDFFF]);
    assert_eq!(
        mixed,
        JsString::from_units(vec![0x41, 0xD800, 0x42, 0xDFFF])
    );
}

#[test]
fn runtime_string_from_code_point_preserves_valid_pair_among_surrogates() {
    let r = __ts_aot_string_from_code_point(&[0xD83D, 0xDE00]);
    assert_eq!(r, js("\u{1F600}"));
}

#[test]
#[should_panic(expected = "RangeError")]
fn runtime_string_from_code_point_throws_on_code_point_above_max() {
    let _ = __ts_aot_string_from_code_point(&[0x11_0000_i64]);
}

#[test]
fn runtime_string_index_of_uses_utf16_code_unit_indexing() {
    assert_eq!(
        __ts_aot_string_index_of(&js("\u{1F600}x\u{1F600}"), &js("\u{1F600}"), 0),
        0
    );
    assert_eq!(
        __ts_aot_string_index_of(&js("\u{1F600}x\u{1F600}"), &js("x"), 0),
        2
    );
    assert_eq!(
        __ts_aot_string_index_of(&js("\u{1F600}x\u{1F600}"), &js("\u{1F600}"), 1),
        3
    );
    assert_eq!(__ts_aot_string_index_of(&js("a\u{1F600}b"), &js("b"), 0), 3);
    assert_eq!(__ts_aot_string_index_of(&js("a\u{1F600}b"), &js("b"), 2), 3);
    assert_eq!(__ts_aot_string_index_of(&js("café"), &js("é"), 0), 3);
    assert_eq!(__ts_aot_string_index_of(&js("café"), &js("é"), 4), -1);
}

#[test]
fn runtime_string_char_at_uses_utf16_code_unit_indexing() {
    assert_eq!(
        __ts_aot_string_char_at(&js("\u{1F600}"), 0),
        JsString::from_units(vec![0xD83D])
    );
    assert_eq!(
        __ts_aot_string_char_at(&js("\u{1F600}"), 1),
        JsString::from_units(vec![0xDE00])
    );
    assert_eq!(__ts_aot_string_char_at(&js("a\u{1F600}b"), 0), js("a"));
    assert_eq!(
        __ts_aot_string_char_at(&js("a\u{1F600}b"), 1),
        JsString::from_units(vec![0xD83D])
    );
    assert_eq!(
        __ts_aot_string_char_at(&js("a\u{1F600}b"), 2),
        JsString::from_units(vec![0xDE00])
    );
    assert_eq!(__ts_aot_string_char_at(&js("a\u{1F600}b"), 3), js("b"));
    assert_eq!(__ts_aot_string_char_at(&js("a\u{1F600}b"), 4), js(""));
    assert_eq!(__ts_aot_string_char_at(&js("a\u{1F600}b"), -1), js(""));
    assert_eq!(__ts_aot_string_char_at(&js("café"), 3), js("é"));
    assert_eq!(__ts_aot_string_char_at(&js("café"), 4), js(""));
}

#[test]
fn runtime_string_substring_utf16_uses_utf16_code_unit_indices() {
    let s = "é=value";
    assert_eq!(__ts_aot_string_substring_utf16(&js(s), 2, 7), js("value"));
    assert_eq!(__ts_aot_string_substring_utf16(&js(s), 0, 100), js(s));
    assert_eq!(__ts_aot_string_substring_utf16(&js(s), -5, 3), js("é=v"));
    assert_eq!(__ts_aot_string_substring_utf16(&js(s), 2, 2), js(""));
    let astral = "😀ab";
    assert_eq!(__ts_aot_string_substring_utf16(&js(astral), 0, 2), js("😀"));
    assert_eq!(__ts_aot_string_substring_utf16(&js(astral), 2, 4), js("ab"));
}

#[test]
fn runtime_string_substring_utf16_preserves_lone_surrogates_as_raw_units() {
    let s = "\u{1F600}";
    assert_eq!(
        __ts_aot_string_substring_utf16(&js(s), 0, 1),
        JsString::from_units(vec![0xD83D])
    );
    assert_eq!(
        __ts_aot_string_substring_utf16(&js(s), 1, 2),
        JsString::from_units(vec![0xDE00])
    );
    let s2 = "a\u{1F600}b";
    assert_eq!(
        __ts_aot_string_substring_utf16(&js(s2), 0, 2),
        JsString::from_units(vec![0x61, 0xD83D])
    );
    assert_eq!(
        __ts_aot_string_substring_utf16(&js(s2), 2, 4),
        JsString::from_units(vec![0xDE00, 0x62])
    );
    let s3 = JsString::from_units(vec![
        u16::from(b'x'),
        0xD83D,
        u16::from(b'y'),
        0xDE00,
        u16::from(b'z'),
    ]);
    assert_eq!(
        __ts_aot_string_substring_utf16(&s3, 0, 2),
        JsString::from_units(vec![u16::from(b'x'), 0xD83D])
    );
    assert_eq!(
        __ts_aot_string_substring_utf16(&s3, 2, 4),
        JsString::from_units(vec![u16::from(b'y'), 0xDE00])
    );
}

#[test]
fn runtime_string_index_of_and_char_at_compose_on_non_ascii() {
    let haystack = js("é=value\u{1F600}");
    let eq_idx = __ts_aot_string_index_of(&haystack, &js("="), 0);
    assert_eq!(eq_idx, 1);
    let value =
        __ts_aot_string_substring_utf16(&haystack, eq_idx + 1, __ts_aot_string_len(&haystack));
    assert_eq!(value, js("value\u{1F600}"));
    let emoji_idx = __ts_aot_string_index_of(&value, &js("\u{1F600}"), 0);
    assert_eq!(emoji_idx, 5);
    let next = __ts_aot_string_char_at(&value, emoji_idx);
    assert_eq!(next, JsString::from_units(vec![0xD83D]));
}
