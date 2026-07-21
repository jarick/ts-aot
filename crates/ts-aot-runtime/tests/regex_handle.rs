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
fn regexp_handle_new_ignores_unsupported_flags_silently() {
    let h = __ts_aot_regex_new("x", "gy");
    let _ = h.source();
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
fn regexp_handle_new_dedupes_recognized_flags() {
    let h = __ts_aot_regex_new("foo", "iimm");
    let _ = h.source();
}
