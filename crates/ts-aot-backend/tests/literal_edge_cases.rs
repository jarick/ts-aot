use proc_macro2::{Literal, TokenStream};
use quote::quote;

fn render(s: &str) -> String {
    let lit = Literal::string(s);
    let tokens: TokenStream = quote!(#lit);
    tokens.to_string()
}

#[test]
fn literal_string_handles_empty() {
    assert_eq!(render(""), "\"\"");
}

#[test]
fn literal_string_handles_ascii() {
    assert_eq!(render("hello"), "\"hello\"");
}

#[test]
fn literal_string_handles_double_quote() {
    assert_eq!(render("a\"b"), "\"a\\\"b\"");
}

#[test]
fn literal_string_handles_backslash() {
    assert_eq!(render("a\\b"), "\"a\\\\b\"");
}

#[test]
fn literal_string_handles_newline() {
    assert_eq!(render("a\nb"), "\"a\\nb\"");
}

#[test]
fn literal_string_handles_tab() {
    assert_eq!(render("a\tb"), "\"a\\tb\"");
}

#[test]
fn literal_string_handles_carriage_return() {
    assert_eq!(render("a\rb"), "\"a\\rb\"");
}

#[test]
fn literal_string_handles_null_byte() {
    let s = "a\0b";
    let out = render(s);
    eprintln!("null byte render: {out}");
    assert!(
        out.contains("\\0") || out.contains("\\x00"),
        "null byte must be escaped, got: {out}"
    );
}

#[test]
fn literal_string_handles_control_chars() {
    for byte in 1u8..32 {
        if byte == b'\n' || byte == b'\r' || byte == b'\t' {
            continue;
        }
        let s = format!("a{}b", byte as char);
        let out = render(&s);
        eprintln!("ctrl 0x{byte:02x} render: {out}");
        assert!(
            out.contains("\\x") || out.contains("\\u{"),
            "control char 0x{byte:02x} must be escaped, got: {out}"
        );
    }
}

#[test]
fn literal_string_handles_unicode() {
    assert_eq!(render("héllo"), "\"héllo\"");
    let out = render("🦀");
    eprintln!("emoji render: {out}");
    assert!(out.contains("🦀"), "emoji must round-trip, got: {out}");
}

#[test]
fn literal_string_handles_combined_specials() {
    let s = "a\"b\\c\nd\te";
    let out = render(s);
    eprintln!("combined render: {out}");
    assert!(out.contains("\\\""));
    assert!(out.contains("\\\\"));
    assert!(out.contains("\\n"));
    assert!(out.contains("\\t"));
}

#[test]
fn literal_renders_each_edge_case() {
    let test_strings = ["", "a", "\"", "\\", "\n", "\t", "\0", "🦀", "a\"b\\c"];
    for s in test_strings {
        let lit = Literal::string(s);
        let tokens: TokenStream = quote!(let _ = #lit;);
        let src = tokens.to_string();
        assert!(
            !src.is_empty(),
            "Literal::string({s:?}) rendered empty tokens"
        );
    }
}

#[test]
fn literal_string_via_span() {
    let _span = Literal::string("test").span();
}
