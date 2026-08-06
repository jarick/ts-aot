use ts_aot_runtime::{
    __ts_aot_json_parse, __ts_aot_json_parse_string, __ts_aot_json_stringify,
    __ts_aot_json_stringify_string, JsString,
};

fn assert_f64_exact(actual: f64, expected: f64) {
    let equal = actual.to_bits() == expected.to_bits();
    assert!(equal, "f64 mismatch: actual={actual} expected={expected}");
}

#[test]
fn json_parse_i64_valid() {
    let s = JsString::from("42");
    let n: i64 = __ts_aot_json_parse(&s);
    assert_eq!(n, 42);
}

#[test]
fn json_parse_i64_negative() {
    let s = JsString::from("-7");
    let n: i64 = __ts_aot_json_parse(&s);
    assert_eq!(n, -7);
}

#[test]
fn json_parse_f64_valid() {
    let s = JsString::from("3.5");
    let n: f64 = __ts_aot_json_parse(&s);
    assert_f64_exact(n, 3.5);
}

#[test]
fn json_parse_bool_true() {
    let s = JsString::from("true");
    let b: bool = __ts_aot_json_parse(&s);
    assert!(b);
}

#[test]
fn json_parse_bool_false() {
    let s = JsString::from("false");
    let b: bool = __ts_aot_json_parse(&s);
    assert!(!b);
}

#[test]
fn json_parse_string_valid() {
    let s = JsString::from("\"hello\"");
    let parsed: JsString = __ts_aot_json_parse(&s);
    assert_eq!(parsed.to_string_lossy(), "hello");
}

#[test]
fn json_parse_option_i64_some() {
    let s = JsString::from("100");
    let parsed: Option<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, Some(100));
}

#[test]
fn json_parse_option_i64_none() {
    let s = JsString::from("null");
    let parsed: Option<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, None);
}

#[test]
fn json_parse_vec_i64() {
    let s = JsString::from("[1,2,3]");
    let parsed: Vec<i64> = __ts_aot_json_parse(&s);
    assert_eq!(parsed, vec![1, 2, 3]);
}

#[test]
fn json_parse_vec_string() {
    let s = JsString::from("[\"a\",\"b\"]");
    let parsed: Vec<JsString> = __ts_aot_json_parse(&s);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].to_string_lossy(), "a");
    assert_eq!(parsed[1].to_string_lossy(), "b");
}

#[test]
fn json_parse_invalid_panics_via_throw() {
    let s = JsString::from("{not valid json");
    let result = std::panic::catch_unwind(|| {
        let _parsed: i64 = __ts_aot_json_parse(&s);
    });
    assert!(result.is_err(), "JSON.parse on invalid input must throw");
}

#[test]
fn json_stringify_i64() {
    let n: i64 = 42;
    let s = __ts_aot_json_stringify(&n);
    assert_eq!(s.to_string_lossy(), "42");
}

#[test]
fn json_stringify_f64() {
    let n: f64 = 3.5;
    let s = __ts_aot_json_stringify(&n);
    assert_eq!(s.to_string_lossy(), "3.5");
}

#[test]
fn json_stringify_bool() {
    let b: bool = true;
    let s = __ts_aot_json_stringify(&b);
    assert_eq!(s.to_string_lossy(), "true");
}

#[test]
fn json_stringify_string() {
    let s_in = JsString::from("hi");
    let s_out = __ts_aot_json_stringify(&s_in);
    assert_eq!(s_out.to_string_lossy(), "\"hi\"");
}

#[test]
fn json_stringify_nan_becomes_null() {
    let n: f64 = f64::NAN;
    let s = __ts_aot_json_stringify(&n);
    assert_eq!(
        s.to_string_lossy(),
        "null",
        "JSON.stringify(NaN) must serialize to null per ECMA-404"
    );
}

#[test]
fn json_stringify_infinity_becomes_null() {
    let n: f64 = f64::INFINITY;
    let s = __ts_aot_json_stringify(&n);
    assert_eq!(
        s.to_string_lossy(),
        "null",
        "JSON.stringify(Infinity) must serialize to null per ECMA-404"
    );
}

#[test]
fn json_stringify_neg_infinity_becomes_null() {
    let n: f64 = f64::NEG_INFINITY;
    let s = __ts_aot_json_stringify(&n);
    assert_eq!(s.to_string_lossy(), "null");
}

#[test]
fn json_stringify_vec_i64() {
    let v: Vec<i64> = vec![1, 2, 3];
    let s = __ts_aot_json_stringify(&v);
    assert_eq!(s.to_string_lossy(), "[1,2,3]");
}

#[test]
fn json_stringify_option_i64_some() {
    let v: Option<i64> = Some(42);
    let s = __ts_aot_json_stringify(&v);
    assert_eq!(s.to_string_lossy(), "42");
}

#[test]
fn json_stringify_option_i64_none() {
    let v: Option<i64> = None;
    let s = __ts_aot_json_stringify(&v);
    assert_eq!(s.to_string_lossy(), "null");
}

#[test]
fn json_parse_stringify_roundtrip_i64() {
    let original: i64 = 12345;
    let serialized = __ts_aot_json_stringify(&original);
    let parsed: i64 = __ts_aot_json_parse(&serialized);
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
        let raw = JsString::Raw(units.to_vec());
        let result = std::panic::catch_unwind(|| {
            let _ = __ts_aot_json_parse_string(&raw);
        });
        assert!(result.is_err(), "{label} must throw, got Ok");
    }
}

#[test]
fn jsstring_serde_valid_roundtrips_losslessly() {
    let original = JsString::Valid("hello world".to_string());
    let json = serde_json::to_string(&original).expect("serialize Valid");
    assert_eq!(json, r#""hello world""#);
    let parsed: JsString = serde_json::from_str(&json).expect("deserialize Valid");
    match parsed {
        JsString::Valid(s) => assert_eq!(s, "hello world"),
        JsString::Raw(units) => panic!("Valid round-trip must stay Valid, got Raw({units:?})"),
    }
}

#[test]
fn jsstring_serde_raw_lone_surrogate_is_lossy() {
    let original = JsString::Raw(vec![0xD800]);
    let json = serde_json::to_string(&original).expect("serialize Raw");
    assert_eq!(
        json, r#""�""#,
        "Raw([0xD800]) must serialize as the lossy replacement U+FFFD; serde cannot preserve \
         lone surrogates through str-based serialization. For full preservation use \
         __ts_aot_json_stringify_string."
    );
    let parsed: JsString = serde_json::from_str(&json).expect("deserialize");
    match parsed {
        JsString::Valid(s) => assert_eq!(
            s, "\u{FFFD}",
            "lone surrogate must round-trip as U+FFFD (replacement), not the original 0xD800"
        ),
        JsString::Raw(units) => {
            panic!("lossy serde round-trip of Raw([0xD800]) must land in Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn jsstring_serde_raw_valid_pair_roundtrips_losslessly() {
    let original = JsString::Raw(vec![0xD83D, 0xDE00]);
    let json = serde_json::to_string(&original).expect("serialize");
    assert_eq!(json, r#""😀""#);
    let parsed: JsString = serde_json::from_str(&json).expect("deserialize");
    match parsed {
        JsString::Valid(s) => assert_eq!(s, "\u{1F600}"),
        JsString::Raw(units) => panic!("valid pair must round-trip as Valid, got Raw({units:?})"),
    }
}

#[test]
fn json_stringify_escapes_double_quote_raw() {
    let original = JsString::Raw(vec![0x22]);
    let serialized = __ts_aot_json_stringify_string(&original);
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &[0x22, 0x5C, 0x22, 0x22][..],
            "Raw([0x22]) must stringify as JSON source `\"\\\"\"` (4 units), got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, "\"",
            "round-trip must yield a string containing one double quote, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn json_stringify_escapes_backslash_raw() {
    let original = JsString::Raw(vec![0x5C]);
    let serialized = __ts_aot_json_stringify_string(&original);
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &[0x22, 0x5C, 0x5C, 0x22][..],
            "Raw([0x5C]) must stringify as JSON source `\"\\\\\"` (4 units), got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, "\\",
            "round-trip must yield a string containing one backslash, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn json_stringify_escapes_newline_raw() {
    let original = JsString::Raw(vec![0x0A]);
    let serialized = __ts_aot_json_stringify_string(&original);
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &[0x22, 0x5C, 0x6E, 0x22][..],
            "Raw([0x0A]) must stringify as JSON source `\\n` (4 units: quote, backslash, n, quote), got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, "\n",
            "round-trip must yield a string containing one newline, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn json_stringify_escapes_tab_raw() {
    let original = JsString::Raw(vec![0x09]);
    let serialized = __ts_aot_json_stringify_string(&original);
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &[0x22, 0x5C, 0x74, 0x22][..],
            "Raw([0x09]) must stringify as JSON source `\\t` (4 units: quote, backslash, t, quote), got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, "\t",
            "round-trip must yield a string containing one tab, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn json_stringify_escapes_control_char_without_short_form() {
    let original = JsString::Raw(vec![0x01]);
    let serialized = __ts_aot_json_stringify_string(&original);
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &[0x22, 0x5C, 0x75, 0x30, 0x30, 0x30, 0x31, 0x22][..],
            "Raw([0x01]) must stringify as JSON source `\\u0001` (8 units), got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, "\u{1}",
            "round-trip must yield a string containing U+0001, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}

#[test]
fn json_stringify_escapes_valid_quote_in_body() {
    let original = JsString::Valid(r#"hello"world"#.to_string());
    let serialized = __ts_aot_json_stringify_string(&original);
    let expected_serialized: Vec<u16> = r#""hello\"world""#.encode_utf16().collect();
    match &serialized {
        JsString::Raw(units) => assert_eq!(
            units,
            &expected_serialized[..],
            "Valid containing quote must produce escaped JSON source, got {units:?}"
        ),
        other @ JsString::Valid(_) => panic!("stringify_string must return Raw, got {other:?}"),
    }
    let reparsed = __ts_aot_json_parse_string(&serialized);
    match reparsed {
        JsString::Valid(s) => assert_eq!(
            s, r#"hello"world"#,
            "round-trip must recover original Valid with embedded quote, got {s:?}"
        ),
        JsString::Raw(units) => {
            panic!("__ts_aot_json_parse_string now returns Valid, got Raw({units:?})")
        }
    }
}
