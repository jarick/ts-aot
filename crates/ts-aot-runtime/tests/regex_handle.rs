use ts_aot_runtime::__ts_aot_regex_new;

#[test]
fn regexp_handle_new_compiles_simple_pattern() {
    let h = __ts_aot_regex_new("foo", "");
    assert_eq!(h.source(), "foo");
}

#[test]
fn regexp_handle_new_preserves_pattern_chars() {
    let h = __ts_aot_regex_new("[a-z]+", "i");
    assert!(h.source().contains("a-z"));
}

#[test]
fn regexp_handle_new_no_flags_works() {
    let _h = __ts_aot_regex_new("test", "");
}

#[test]
fn regexp_handle_is_cloneable() {
    let h = __ts_aot_regex_new("foo", "");
    let _h2 = h.clone();
}

#[test]
fn regexp_handle_new_accepts_multiline_flag() {
    let h = __ts_aot_regex_new("^foo", "m");
    let _ = h.source();
}

#[test]
fn regexp_handle_new_combines_i_s_and_m_flags() {
    let h = __ts_aot_regex_new("^foo$", "mis");
    let _ = h.source();
}

#[test]
fn regexp_handle_new_accepts_global_flag() {
    let h = __ts_aot_regex_new("foo", "g");
    assert_eq!(h.source(), "foo");
}

#[test]
fn regexp_handle_new_accepts_unicode_flag() {
    let h = __ts_aot_regex_new("foo", "u");
    assert_eq!(h.source(), "foo");
}

#[test]
fn regexp_handle_new_accepts_sticky_flag() {
    let h = __ts_aot_regex_new("foo", "y");
    assert_eq!(h.source(), "foo");
}

#[test]
fn regexp_handle_new_accepts_combined_g_u_y_flags() {
    let h = __ts_aot_regex_new("foo", "guy");
    assert_eq!(h.source(), "foo");
}

#[test]
#[should_panic(expected = "invalid flag 'x'")]
fn regexp_handle_new_rejects_unknown_flag() {
    let _h = __ts_aot_regex_new("foo", "x");
}

#[test]
#[should_panic(expected = "duplicate flag 'g'")]
fn regexp_handle_new_rejects_duplicate_global_flag() {
    let _h = __ts_aot_regex_new("foo", "gg");
}

#[test]
#[should_panic(expected = "duplicate flag 'i'")]
fn regexp_handle_new_rejects_duplicate_flag() {
    let _h = __ts_aot_regex_new("foo", "ii");
}

#[test]
#[should_panic(expected = "SyntaxError")]
fn regexp_handle_new_rejects_invalid_pattern() {
    let _h = __ts_aot_regex_new("(unclosed", "");
}
