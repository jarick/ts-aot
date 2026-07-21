use ts_aot_runtime::__ts_aot_bigint_new;

#[test]
fn bigint_handle_new_preserves_small_value() {
    let h = __ts_aot_bigint_new("42");
    assert_eq!(h.value(), "42");
}

#[test]
fn bigint_handle_new_preserves_large_value() {
    let h = __ts_aot_bigint_new("99999999999999999999");
    assert_eq!(h.value(), "99999999999999999999");
}

#[test]
fn bigint_handle_new_accepts_zero() {
    let h = __ts_aot_bigint_new("0");
    assert_eq!(h.value(), "0");
}

#[test]
fn bigint_handle_is_cloneable() {
    let h = __ts_aot_bigint_new("123");
    let _h2 = h.clone();
}
