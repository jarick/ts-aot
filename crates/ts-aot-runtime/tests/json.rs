use ts_aot_runtime::{
    __ts_aot_json_parse, __ts_aot_json_parse_string, __ts_aot_json_stringify,
    __ts_aot_json_stringify_string, Ctx, Host, JsString, run_in_arena,
};

fn with_ctx<R>(f: impl for<'a> FnOnce(&Ctx<'a>) -> R) -> R {
    struct H;
    impl Host for H {}
    run_in_arena(&H, f)
}

fn assert_f64_exact(actual: f64, expected: f64) {
    let equal = actual.to_bits() == expected.to_bits();
    assert!(equal, "f64 mismatch: actual={actual} expected={expected}");
}

#[test]
fn json_parse_i64_valid() {
    let s = JsString::Valid("42");
    let n: i64 = __ts_aot_json_parse(&s);
    assert_eq!(n, 42);
}

#[test]
fn json_parse_i64_negative() {
    let s = JsString::Valid("-7");
    let n: i64 = __ts_aot_json_parse(&s);
    assert_eq!(n, -7);
}

#[test]
fn json_parse_f64_valid() {
    let s = JsString::Valid("3.5");
    let n: f64 = __ts_aot_json_parse(&s);
    assert_f64_exact(n, 3.5);
}

#[test]
fn json_parse_bool_true() {
    let s = JsString::Valid("true");
    let b: bool = __ts_aot_json_parse(&s);
    assert!(b);
}

#[test]
fn json_parse_bool_false() {
    let s = JsString::Valid("false");
    let b: bool = __ts_aot_json_parse(&s);
    assert!(!b);
}

#[test]
fn json_parse_string_valid() {
    let s = JsString::Valid("\"hello\"");
    let parsed = with_ctx(|ctx| __ts_aot_json_parse_string(ctx, &s).to_string_lossy());
    assert_eq!(parsed, "hello");
}

#[test]
fn json_parse_option_i64_some() {
    let s = JsString::Valid("100");
    let parsed: Option<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, Some(100));
}

#[test]
fn json_parse_option_i64_none() {
    let s = JsString::Valid("null");
    let parsed: Option<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, None);
}

#[test]
fn json_parse_vec_i64() {
    let s = JsString::Valid("[1,2,3]");
    let parsed: Vec<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, vec![1, 2, 3]);
}

#[test]
fn json_parse_invalid_panics_via_throw() {
    let s = JsString::Valid("{not valid json");
    let result = std::panic::catch_unwind(|| {
        let _parsed: i64 = __ts_aot_json_parse(&s);
    });
    assert!(result.is_err(), "JSON.parse on invalid input must throw");
}

#[test]
fn json_stringify_i64() {
    let n: i64 = 42;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &n).to_string_lossy());
    assert_eq!(s, "42");
}

#[test]
fn json_stringify_f64() {
    let n: f64 = 3.5;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &n).to_string_lossy());
    assert_eq!(s, "3.5");
}

#[test]
fn json_stringify_bool() {
    let b: bool = true;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &b).to_string_lossy());
    assert_eq!(s, "true");
}

#[test]
fn json_stringify_string() {
    let s_in = JsString::Valid("hi");
    let s_out = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &s_in).to_string_lossy());
    assert_eq!(s_out, "\"hi\"");
}

#[test]
fn json_stringify_nan_becomes_null() {
    let n: f64 = f64::NAN;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &n).to_string_lossy());
    assert_eq!(
        s, "null",
        "JSON.stringify(NaN) must serialize to null per ECMA-404"
    );
}

#[test]
fn json_stringify_infinity_becomes_null() {
    let n: f64 = f64::INFINITY;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &n).to_string_lossy());
    assert_eq!(
        s, "null",
        "JSON.stringify(Infinity) must serialize to null per ECMA-404"
    );
}

#[test]
fn json_stringify_neg_infinity_becomes_null() {
    let n: f64 = f64::NEG_INFINITY;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &n).to_string_lossy());
    assert_eq!(s, "null");
}

#[test]
fn json_stringify_vec_i64() {
    let v: Vec<i64> = vec![1, 2, 3];
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &v).to_string_lossy());
    assert_eq!(s, "[1,2,3]");
}

#[test]
fn json_stringify_option_i64_some() {
    let v: Option<i64> = Some(42);
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &v).to_string_lossy());
    assert_eq!(s, "42");
}

#[test]
fn json_stringify_option_i64_none() {
    let v: Option<i64> = None;
    let s = with_ctx(|ctx| __ts_aot_json_stringify(ctx, &v).to_string_lossy());
    assert_eq!(s, "null");
}

#[test]
fn json_parse_stringify_roundtrip_i64() {
    let original: i64 = 12345;
    let parsed = with_ctx(|ctx| {
        let serialized = __ts_aot_json_stringify(ctx, &original);
        __ts_aot_json_parse::<i64>(&serialized)
    });
    assert_eq!(parsed, original);
}

#[test]
fn json_parse_string_rejects_non_hex_unicode_escape() {
    let cases: &[(&[u16], &str)] = &[
        (
            &[0x22, 0x5C, 0x75, 0x2D, 0x44, 0x38, 0x30, 0x22],
            r#""\u-D80" (leading sign)"#,
        ),
        (
            &[0x22, 0x5C, 0x75, 0x2B, 0x38, 0x30, 0x30, 0x22],
            r#""\u+800" (sign)"#,
        ),
        (
            &[0x22, 0x5C, 0x75, 0x44, 0x38, 0x5A, 0x22],
            r#""\uD8Z" (3 hex + non-hex)"#,
        ),
        (
            &[0x22, 0x5C, 0x75, 0x47, 0x47, 0x47, 0x47, 0x22],
            r#""\uGGGG" (all non-hex letters)"#,
        ),
        (
            &[0x22, 0x5C, 0x75, 0x44, 0x38, 0x20, 0x22],
            r#""\uD8 " (hex + space)"#,
        ),
    ];
    for (units, label) in cases {
        let raw = JsString::Raw(units);
        let result = with_ctx(|ctx| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = __ts_aot_json_parse_string(ctx, &raw);
            }))
            .is_err()
        });
        assert!(result, "{label} must throw, got Ok");
    }
}

#[test]
fn json_stringify_escapes_double_quote_raw() {
    let units = [0x22u16];
    let original = JsString::Raw(&units);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    assert_eq!(
        serialized,
        vec![0x22, 0x5C, 0x22, 0x22],
        "Raw([0x22]) must stringify as JSON source `\"\\\"\"` (4 units)"
    );
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, "\"");
}

#[test]
fn json_stringify_escapes_backslash_raw() {
    let units = [0x5Cu16];
    let original = JsString::Raw(&units);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    assert_eq!(serialized, vec![0x22, 0x5C, 0x5C, 0x22]);
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, "\\");
}

#[test]
fn json_stringify_escapes_newline_raw() {
    let units = [0x0Au16];
    let original = JsString::Raw(&units);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    assert_eq!(serialized, vec![0x22, 0x5C, 0x6E, 0x22]);
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, "\n");
}

#[test]
fn json_stringify_escapes_tab_raw() {
    let units = [0x09u16];
    let original = JsString::Raw(&units);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    assert_eq!(serialized, vec![0x22, 0x5C, 0x74, 0x22]);
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, "\t");
}

#[test]
fn json_stringify_escapes_control_char_without_short_form() {
    let units = [0x01u16];
    let original = JsString::Raw(&units);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    assert_eq!(
        serialized,
        vec![0x22, 0x5C, 0x75, 0x30, 0x30, 0x30, 0x31, 0x22]
    );
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, "\u{1}");
}

#[test]
fn json_stringify_escapes_valid_quote_in_body() {
    let original = JsString::Valid(r#"hello"world"#);
    let serialized = with_ctx(|ctx| __ts_aot_json_stringify_string(ctx, &original).to_units());
    let expected: Vec<u16> = r#""hello\"world""#.encode_utf16().collect();
    assert_eq!(serialized, expected);
    let reparsed = with_ctx(|ctx| {
        let serialized = JsString::Raw(&serialized);
        __ts_aot_json_parse_string(ctx, &serialized).to_string_lossy()
    });
    assert_eq!(reparsed, r#"hello"world"#);
}
