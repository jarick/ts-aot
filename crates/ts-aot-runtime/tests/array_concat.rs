use ts_aot_runtime::{__ts_aot_array_concat, __ts_aot_array_hole};

#[test]
fn array_concat_merges_two_vectors() {
    let a: Vec<i64> = vec![1, 2, 3];
    let b: Vec<i64> = vec![4, 5, 6];
    let out = __ts_aot_array_concat(vec![a, b]);
    assert_eq!(out, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn array_concat_with_empty_vector_preserves_other_parts() {
    let a: Vec<i64> = vec![1, 2, 3];
    let b: Vec<i64> = Vec::new();
    let c: Vec<i64> = vec![4, 5];
    let out = __ts_aot_array_concat(vec![a, b, c]);
    assert_eq!(out, vec![1, 2, 3, 4, 5]);
}

#[test]
fn array_concat_with_all_empty_returns_empty() {
    let out = __ts_aot_array_concat::<i64>(vec![Vec::new(), Vec::new()]);
    assert!(out.is_empty());
}

#[test]
fn array_concat_preserves_source_order() {
    let a: Vec<i64> = vec![1];
    let b: Vec<i64> = vec![2];
    let c: Vec<i64> = vec![3];
    let out = __ts_aot_array_concat(vec![a, b, c]);
    assert_eq!(out, vec![1, 2, 3]);
}

#[test]
fn array_hole_returns_single_default_value() {
    let out = __ts_aot_array_hole::<i64>();
    assert_eq!(out, vec![0]);
}

#[test]
fn array_hole_for_f64_returns_zero() {
    let out = __ts_aot_array_hole::<f64>();
    assert_eq!(out, vec![0.0]);
}

#[test]
fn array_hole_for_string_returns_empty_string() {
    let out = __ts_aot_array_hole::<String>();
    assert_eq!(out, vec![String::new()]);
}
