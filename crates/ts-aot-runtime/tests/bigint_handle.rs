use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use ts_aot_runtime::{
    __ts_aot_bigint_add, __ts_aot_bigint_div, __ts_aot_bigint_eq, __ts_aot_bigint_ge,
    __ts_aot_bigint_gt, __ts_aot_bigint_le, __ts_aot_bigint_lt, __ts_aot_bigint_mul,
    __ts_aot_bigint_neg, __ts_aot_bigint_neq, __ts_aot_bigint_new, __ts_aot_bigint_not,
    __ts_aot_bigint_rem, __ts_aot_bigint_sub, __ts_aot_bigint_to_number, __ts_aot_bigint_to_string,
    BigIntHandle,
};

fn b(s: &str) -> BigIntHandle {
    __ts_aot_bigint_new(s)
}

fn bi(s: &str) -> BigInt {
    s.parse::<BigInt>().expect("valid literal")
}

#[test]
fn bigint_new_preserves_small_value() {
    assert_eq!(*b("42").value(), bi("42"));
}

#[test]
fn bigint_new_preserves_large_value() {
    assert_eq!(
        *b("99999999999999999999").value(),
        bi("99999999999999999999")
    );
}

#[test]
fn bigint_new_accepts_zero() {
    assert_eq!(*b("0").value(), bi("0"));
}

#[test]
fn bigint_new_accepts_negative() {
    assert_eq!(*b("-7").value(), bi("-7"));
}

#[test]
fn bigint_new_accepts_decimal_with_many_digits() {
    assert_eq!(*b("255").value(), bi("255"));
}

#[test]
fn bigint_handle_is_cloneable() {
    let h = b("123");
    let h2 = h.clone();
    assert_eq!(*h.value(), *h2.value());
}

#[test]
fn bigint_add_combines_two_handles() {
    let r = __ts_aot_bigint_add(&b("40"), &b("2"));
    assert_eq!(*r.value(), bi("42"));
}

#[test]
fn bigint_add_with_negative_yields_difference() {
    let r = __ts_aot_bigint_add(&b("10"), &b("-3"));
    assert_eq!(*r.value(), bi("7"));
}

#[test]
fn bigint_sub_yields_difference() {
    let r = __ts_aot_bigint_sub(&b("10"), &b("3"));
    assert_eq!(*r.value(), bi("7"));
}

#[test]
fn bigint_mul_multiplies() {
    let r = __ts_aot_bigint_mul(&b("6"), &b("7"));
    assert_eq!(*r.value(), bi("42"));
}

#[test]
fn bigint_div_rounds_toward_zero() {
    let r = __ts_aot_bigint_div(&b("7"), &b("2"));
    assert_eq!(*r.value(), bi("3"));
}

#[test]
fn bigint_div_negative_dividend_rounds_toward_zero() {
    let r = __ts_aot_bigint_div(&b("-7"), &b("2"));
    assert_eq!(*r.value(), bi("-3"));
}

#[test]
fn bigint_rem_yields_remainder() {
    let r = __ts_aot_bigint_rem(&b("7"), &b("3"));
    assert_eq!(*r.value(), bi("1"));
}

#[test]
fn bigint_div_by_zero_yields_zero() {
    let r = __ts_aot_bigint_div(&b("42"), &b("0"));
    assert_eq!(*r.value(), bi("0"));
}

#[test]
fn bigint_rem_by_zero_yields_zero() {
    let r = __ts_aot_bigint_rem(&b("42"), &b("0"));
    assert_eq!(*r.value(), bi("0"));
}

#[test]
fn bigint_neg_flips_sign() {
    let r = __ts_aot_bigint_neg(&b("42"));
    assert_eq!(*r.value(), bi("-42"));
    let r2 = __ts_aot_bigint_neg(&b("-42"));
    assert_eq!(*r2.value(), bi("42"));
}

#[test]
fn bigint_not_bitwise_complement() {
    let r = __ts_aot_bigint_not(&b("0"));
    assert_eq!(*r.value(), BigInt::from(-1));
}

#[test]
fn bigint_lt_compares() {
    assert!(__ts_aot_bigint_lt(&b("1"), &b("2")));
    assert!(!__ts_aot_bigint_lt(&b("2"), &b("2")));
    assert!(!__ts_aot_bigint_lt(&b("3"), &b("2")));
}

#[test]
fn bigint_le_compares() {
    assert!(__ts_aot_bigint_le(&b("2"), &b("2")));
    assert!(__ts_aot_bigint_le(&b("1"), &b("2")));
    assert!(!__ts_aot_bigint_le(&b("3"), &b("2")));
}

#[test]
fn bigint_gt_compares() {
    assert!(__ts_aot_bigint_gt(&b("3"), &b("2")));
    assert!(!__ts_aot_bigint_gt(&b("2"), &b("2")));
    assert!(!__ts_aot_bigint_gt(&b("1"), &b("2")));
}

#[test]
fn bigint_ge_compares() {
    assert!(__ts_aot_bigint_ge(&b("2"), &b("2")));
    assert!(__ts_aot_bigint_ge(&b("3"), &b("2")));
    assert!(!__ts_aot_bigint_ge(&b("1"), &b("2")));
}

#[test]
fn bigint_eq_compares() {
    assert!(__ts_aot_bigint_eq(&b("42"), &b("42")));
    assert!(!__ts_aot_bigint_eq(&b("42"), &b("43")));
    assert!(!__ts_aot_bigint_eq(&b("42"), &b("-42")));
}

#[test]
fn bigint_neq_compares() {
    assert!(!__ts_aot_bigint_neq(&b("42"), &b("42")));
    assert!(__ts_aot_bigint_neq(&b("42"), &b("43")));
}

#[test]
fn bigint_to_string_renders_decimal() {
    let s = __ts_aot_bigint_to_string(&b("123456789012345678901234567890"));
    assert_eq!(s, "123456789012345678901234567890");
}

#[test]
fn bigint_to_string_renders_negative() {
    let s = __ts_aot_bigint_to_string(&b("-42"));
    assert_eq!(s, "-42");
}

#[test]
fn bigint_to_number_within_f64_precision() {
    let n = __ts_aot_bigint_to_number(&b("42"));
    assert!((n - 42.0).abs() < f64::EPSILON);
}

#[test]
fn bigint_to_number_for_large_value_loses_precision_but_returns_f64() {
    let n = __ts_aot_bigint_to_number(&b("99999999999999999999"));
    assert!(n.is_finite());
}

#[test]
fn bigint_is_zero_and_is_negative_via_value() {
    let zero = bi("0");
    let one = bi("1");
    let neg_one = bi("-1");
    assert!(zero.is_zero());
    assert!(!zero.is_negative());
    assert!(!one.is_negative());
    assert!(!one.is_zero());
    assert!(neg_one.is_negative());
    assert!(!neg_one.is_zero());
}
