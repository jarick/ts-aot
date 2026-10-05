use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use ts_aot_runtime::{
    __ts_aot_bigint_add, __ts_aot_bigint_div, __ts_aot_bigint_eq, __ts_aot_bigint_ge,
    __ts_aot_bigint_gt, __ts_aot_bigint_le, __ts_aot_bigint_lt, __ts_aot_bigint_mul,
    __ts_aot_bigint_neg, __ts_aot_bigint_neq, __ts_aot_bigint_new, __ts_aot_bigint_not,
    __ts_aot_bigint_rem, __ts_aot_bigint_shl, __ts_aot_bigint_shr, __ts_aot_bigint_sub,
    __ts_aot_bigint_to_number, __ts_aot_bigint_to_string, BigIntHandle, BigIntSyntaxError,
    parse_string_integer_literal,
};

fn b(s: &str) -> BigIntHandle {
    __ts_aot_bigint_new(s)
}

fn bi(s: &str) -> BigInt {
    s.parse::<BigInt>().expect("valid literal")
}

#[test]
fn parse_string_integer_literal_decimal_with_sign() {
    assert_eq!(parse_string_integer_literal("42").unwrap(), bi("42"));
    assert_eq!(parse_string_integer_literal("-42").unwrap(), bi("-42"));
    assert_eq!(parse_string_integer_literal("+42").unwrap(), bi("42"));
}

#[test]
fn parse_string_integer_literal_hex_prefix() {
    assert_eq!(parse_string_integer_literal("0x10").unwrap(), bi("16"));
    assert_eq!(parse_string_integer_literal("0X10").unwrap(), bi("16"));
    assert_eq!(parse_string_integer_literal("0xff").unwrap(), bi("255"));
}

#[test]
fn parse_string_integer_literal_octal_prefix() {
    assert_eq!(parse_string_integer_literal("0o17").unwrap(), bi("15"));
    assert_eq!(parse_string_integer_literal("0O17").unwrap(), bi("15"));
}

#[test]
fn parse_string_integer_literal_binary_prefix() {
    assert_eq!(parse_string_integer_literal("0b101").unwrap(), bi("5"));
    assert_eq!(parse_string_integer_literal("0B1010").unwrap(), bi("10"));
}

#[test]
fn parse_string_integer_literal_trims_whitespace() {
    assert_eq!(parse_string_integer_literal(" 42 ").unwrap(), bi("42"));
    assert_eq!(parse_string_integer_literal("\t-7\n").unwrap(), bi("-7"));
    assert_eq!(parse_string_integer_literal("  0xFF  ").unwrap(), bi("255"));
}

#[test]
fn parse_string_integer_literal_trims_unicode_bom() {
    assert_eq!(
        parse_string_integer_literal("\u{FEFF}42\u{FEFF}").unwrap(),
        bi("42"),
    );
    assert_eq!(
        parse_string_integer_literal("\u{FEFF}-7").unwrap(),
        bi("-7"),
    );
}

#[test]
fn parse_string_integer_literal_rejects_nel_as_non_whitespace() {
    assert!(parse_string_integer_literal("\u{0085}42").is_err());
    assert!(parse_string_integer_literal("42\u{0085}").is_err());
    assert!(parse_string_integer_literal("\u{0085}42\u{0085}").is_err());
}

#[test]
fn parse_string_integer_literal_empty_string_is_zero() {
    assert_eq!(parse_string_integer_literal("").unwrap(), bi("0"));
    assert_eq!(parse_string_integer_literal("   ").unwrap(), bi("0"));
}

#[test]
fn parse_string_integer_literal_rejects_invalid_strings() {
    assert!(parse_string_integer_literal("abc").is_err());
    assert!(parse_string_integer_literal("0x").is_err());
    assert!(parse_string_integer_literal("0b").is_err());
    assert!(parse_string_integer_literal("12.5").is_err());
    assert!(parse_string_integer_literal("12n").is_err());
    assert_eq!(parse_string_integer_literal("01").unwrap(), bi("1"));
    assert!(parse_string_integer_literal("--5").is_err());
    assert!(parse_string_integer_literal("+-5").is_err());
    assert!(parse_string_integer_literal("0xZZ").is_err());
}

#[test]
fn parse_string_integer_literal_rejects_signed_non_decimal() {
    assert!(parse_string_integer_literal("-0x10").is_err());
    assert!(parse_string_integer_literal("+0x10").is_err());
    assert!(parse_string_integer_literal("-0X10").is_err());
    assert!(parse_string_integer_literal("-0o17").is_err());
    assert!(parse_string_integer_literal("+0o17").is_err());
    assert!(parse_string_integer_literal("-0b101").is_err());
    assert!(parse_string_integer_literal("+0b101").is_err());
}

#[test]
fn parse_string_integer_literal_rejects_bare_sign_and_internal_whitespace() {
    assert!(parse_string_integer_literal("+").is_err());
    assert!(parse_string_integer_literal("-").is_err());
    assert!(parse_string_integer_literal("+ 42").is_err());
    assert!(parse_string_integer_literal("- 42").is_err());
    assert!(parse_string_integer_literal("4 2").is_err());
    assert!(parse_string_integer_literal("  +  42  ").is_err());
}

#[test]
fn parse_string_integer_literal_accepts_decimal_with_leading_zero() {
    assert_eq!(parse_string_integer_literal("01").unwrap(), bi("1"));
    assert_eq!(parse_string_integer_literal("007").unwrap(), bi("7"));
    assert_eq!(parse_string_integer_literal("-01").unwrap(), bi("-1"));
    assert_eq!(parse_string_integer_literal("+007").unwrap(), bi("7"));
}

#[test]
fn parse_string_integer_literal_signed_hex_is_syntax_error() {
    let err = parse_string_integer_literal("-0x10").err().unwrap();
    assert!(err.message.contains("-0x10"));
}

#[test]
fn parse_string_integer_literal_rejects_underscore_in_decimal() {
    assert!(parse_string_integer_literal("1_000").is_err());
    assert!(parse_string_integer_literal("1_000_000").is_err());
    assert!(parse_string_integer_literal("-1_000").is_err());
    assert!(parse_string_integer_literal("+1_000").is_err());
}

#[test]
fn parse_string_integer_literal_rejects_underscore_in_prefixed_hex() {
    assert!(parse_string_integer_literal("0xFF_FF").is_err());
    assert!(parse_string_integer_literal("0b1010_1010").is_err());
    assert!(parse_string_integer_literal("0o1_2_3").is_err());
}

#[test]
fn parse_string_integer_literal_returns_syntax_error_with_message() {
    let err = parse_string_integer_literal("abc").err().unwrap();
    assert!(
        err.message.contains("abc"),
        "syntax error message must quote the offending literal; got: {:?}",
        err.message
    );
}

#[test]
fn bigint_new_parses_hex_with_prefix() {
    assert_eq!(*b("0x10").value(), bi("16"));
}

#[test]
fn bigint_new_trims_whitespace() {
    assert_eq!(*b(" 42 ").value(), bi("42"));
}

#[test]
fn bigint_new_empty_string_is_zero() {
    assert_eq!(*b("").value(), bi("0"));
}

#[test]
fn bigint_new_invalid_string_panics_with_bigint_syntax_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_new("not-a-number");
    });
    let payload = result.expect_err("invalid BigInt literal must panic");
    let syntax_err = payload
        .downcast_ref::<BigIntSyntaxError>()
        .expect("panic payload must be BigIntSyntaxError");
    assert!(
        syntax_err.message.contains("not-a-number"),
        "syntax error message must quote the offending literal; got: {:?}",
        syntax_err.message
    );
}

#[test]
fn bigint_shl_by_zero_is_identity() {
    assert_eq!(*__ts_aot_bigint_shl(&b("42"), &b("0")).value(), bi("42"));
}

#[test]
fn bigint_shr_by_zero_is_identity() {
    assert_eq!(*__ts_aot_bigint_shr(&b("42"), &b("0")).value(), bi("42"));
}

#[test]
fn bigint_shl_by_positive_count() {
    assert_eq!(*__ts_aot_bigint_shl(&b("1"), &b("4")).value(), bi("16"));
    assert_eq!(*__ts_aot_bigint_shl(&b("3"), &b("2")).value(), bi("12"));
}

#[test]
fn bigint_shr_by_positive_count_preserves_sign() {
    let r = __ts_aot_bigint_shr(&b("-256"), &b("3"));
    assert_eq!(*r.value(), bi("-32"));
}

#[test]
fn bigint_shl_by_u32_max_count_yields_large_value() {
    let r = __ts_aot_bigint_shl(&b("1234567890"), &b("4294967295"));
    assert!(
        !r.value().is_zero(),
        "u32::MAX shift must yield a non-zero value"
    );
}

#[test]
fn bigint_shr_by_u32_max_count_preserves_sign_zero_extend() {
    let r = __ts_aot_bigint_shr(&b("-1"), &b("4294967295"));
    assert_eq!(
        *r.value(),
        bi("-1"),
        "u32::MAX shift on -1 stays -1 for arithmetic shr"
    );
}

#[test]
fn bigint_shl_by_negative_count_reverses_to_shr() {
    let r = __ts_aot_bigint_shl(&b("42"), &b("-1"));
    assert_eq!(*r.value(), bi("21"), "shl by -1 == shr by 1");
}

#[test]
fn bigint_shr_by_negative_count_reverses_to_shl() {
    let r = __ts_aot_bigint_shr(&b("42"), &b("-1"));
    assert_eq!(*r.value(), bi("84"), "shr by -1 == shl by 1");
}

#[test]
fn bigint_shl_by_negative_count_at_min_i64_uses_magnitude_safely() {
    let r = __ts_aot_bigint_shl(&b("0"), &b("-9223372036854775808"));
    assert_eq!(*r.value(), bi("0"), "shr by 2^63 on 0 stays 0");
}

#[test]
fn bigint_shl_by_count_above_u64_wraps_modulo() {
    let r = __ts_aot_bigint_shl(&b("1"), &b("18446744073709551617"));
    assert_eq!(
        *r.value(),
        bi("2"),
        "shift count u64::MAX+1 wraps to 1 via modulo, shl by 1"
    );
}

#[test]
fn bigint_shr_by_count_above_u64_wraps_modulo() {
    let r = __ts_aot_bigint_shr(&b("256"), &b("18446744073709551617"));
    assert_eq!(
        *r.value(),
        bi("128"),
        "shift count u64::MAX+1 wraps to 1 via modulo, shr by 1"
    );
}

#[test]
fn bigint_shl_by_very_large_positive_count_wraps_modulo() {
    let baseline = __ts_aot_bigint_shl(&b("1"), &b("64"));
    assert_eq!(*baseline.value(), bi("18446744073709551616"));
    let r = __ts_aot_bigint_shl(&b("1"), &b("65"));
    assert_eq!(
        *r.value(),
        bi("36893488147419103232"),
        "shift count 65 wraps to 65 via modulo u64::MAX+1"
    );
}

#[test]
fn bigint_shl_by_count_equals_u64_max_plus_one_is_identity() {
    let r = __ts_aot_bigint_shl(&b("42"), &b("18446744073709551616"));
    assert_eq!(
        *r.value(),
        bi("42"),
        "shift count u64::MAX+1 wraps to 0 via modulo, shl by 0 is identity"
    );
}

#[test]
fn bigint_shl_by_count_above_max_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_shl(&b("1"), &b("4294967296"));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_shr_by_count_above_max_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_shr(&b("256"), &b("4294967296"));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_shr_by_negative_count_above_max_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_shr(&b("256"), &b("-4294967296"));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
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
fn bigint_div_by_zero_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_div(&b("42"), &b("0"));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_rem_by_zero_throws_range_error() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_rem(&b("42"), &b("0"));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
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
    let s = __ts_aot_bigint_to_string(&b("123456789012345678901234567890"), None);
    assert_eq!(s, "123456789012345678901234567890");
}

#[test]
fn bigint_to_string_renders_negative() {
    let s = __ts_aot_bigint_to_string(&b("-42"), None);
    assert_eq!(s, "-42");
}

#[test]
fn bigint_to_string_supports_radix_2() {
    let s = __ts_aot_bigint_to_string(&b("10"), Some(2));
    assert_eq!(s, "1010");
}

#[test]
fn bigint_to_string_supports_radix_16() {
    let s = __ts_aot_bigint_to_string(&b("255"), Some(16));
    assert_eq!(s, "ff");
}

#[test]
fn bigint_to_string_supports_radix_36() {
    let s = __ts_aot_bigint_to_string(&b("35"), Some(36));
    assert_eq!(s, "z");
}

#[test]
fn bigint_to_string_negative_with_radix() {
    let s = __ts_aot_bigint_to_string(&b("-255"), Some(16));
    assert_eq!(s, "-ff");
}

#[test]
fn bigint_to_string_rejects_radix_0() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_to_string(&b("42"), Some(0));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_to_string_rejects_radix_1() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_to_string(&b("42"), Some(1));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_to_string_rejects_radix_37() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_to_string(&b("42"), Some(37));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
}

#[test]
fn bigint_to_string_rejects_radix_negative() {
    let result = std::panic::catch_unwind(|| {
        let _ = __ts_aot_bigint_to_string(&b("42"), Some(-2));
    });
    assert!(result.is_err());
    let msg = result.unwrap_err();
    let msg_str = msg
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| msg.downcast_ref::<&str>().copied())
        .expect("panic message must be String or &str");
    assert!(
        msg_str.contains("RangeError"),
        "Expected RangeError in panic message, got: {msg_str:?}",
    );
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
