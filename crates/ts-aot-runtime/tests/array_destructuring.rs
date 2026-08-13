use ts_aot_runtime::__ts_aot_array_get_or_default;

fn assert_f64_exact(actual: f64, expected: f64) {
    let equal = if actual.is_nan() || expected.is_nan() {
        actual.is_nan() && expected.is_nan()
    } else {
        actual.to_bits() == expected.to_bits()
    };
    assert!(equal, "f64 mismatch: actual={actual} expected={expected}");
}

#[test]
fn array_get_or_default_returns_element_at_index() {
    let arr: Vec<i64> = vec![10, 20, 30, 40];
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 0), 10);
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 1), 20);
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 2), 30);
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 3), 40);
}

#[test]
fn array_get_or_default_returns_default_for_out_of_range_index() {
    let arr: Vec<i64> = vec![10, 20, 30, 40];
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 4), 0);
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 100), 0);
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, -1), 0);
}

#[test]
fn array_get_or_default_is_generic_over_f64() {
    let arr: Vec<f64> = vec![1.5, 2.5, 3.5];
    assert_f64_exact(__ts_aot_array_get_or_default::<f64>(&arr, 0), 1.5);
    assert_f64_exact(__ts_aot_array_get_or_default::<f64>(&arr, 1), 2.5);
    assert_f64_exact(__ts_aot_array_get_or_default::<f64>(&arr, 99), 0.0);
}

#[test]
fn array_get_or_default_empty_array_returns_default() {
    let arr: Vec<i64> = Vec::new();
    assert_eq!(__ts_aot_array_get_or_default::<i64>(&arr, 0), 0);
}
