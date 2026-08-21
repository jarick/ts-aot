#[must_use]
pub fn sanitize_rust_ident(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, ch) in raw.chars().enumerate() {
        let valid = ch == '_' || ch.is_ascii_alphanumeric();
        if valid {
            if i == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if is_rust_keyword(&out) {
        out.push('_');
    }
    out
}

#[must_use]
pub fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "try"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::{is_rust_keyword, sanitize_rust_ident};

    #[test]
    fn sanitize_rust_ident_replaces_qualified_separator() {
        assert_eq!(sanitize_rust_ident("foo"), "foo");
        assert_eq!(sanitize_rust_ident("foo-bar"), "foo_bar");
        assert_eq!(sanitize_rust_ident("foo::bar"), "foo__bar");
        assert_eq!(sanitize_rust_ident("a.b.c"), "a_b_c");
    }

    #[test]
    fn sanitize_rust_ident_prefixes_digit_start() {
        assert_eq!(sanitize_rust_ident("7greet"), "_7greet");
        assert_eq!(sanitize_rust_ident("0"), "_0");
    }

    #[test]
    fn sanitize_rust_ident_handles_empty_input() {
        assert_eq!(sanitize_rust_ident(""), "_");
        assert_eq!(sanitize_rust_ident("###"), "___");
    }

    #[test]
    fn sanitize_rust_ident_suffixes_keywords() {
        assert_eq!(sanitize_rust_ident("for"), "for_");
        assert_eq!(sanitize_rust_ident("gen"), "gen_");
        assert_eq!(sanitize_rust_ident("type"), "type_");
        assert_eq!(sanitize_rust_ident("fn"), "fn_");
        assert_eq!(sanitize_rust_ident("Self"), "Self_");
    }

    #[test]
    fn is_rust_keyword_recognizes_all_listed_keywords() {
        let all = [
            "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
            "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
            "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await", "dyn", "try", "abstract",
            "become", "box", "do", "final", "gen", "macro", "override", "priv", "typeof",
            "unsized", "virtual", "yield",
        ];
        for kw in all {
            assert!(
                is_rust_keyword(kw),
                "expected {kw:?} to be recognized as a keyword"
            );
            let suffixed = format!("{kw}_");
            assert!(
                !is_rust_keyword(&suffixed),
                "expected {suffixed:?} (suffixed) NOT to be a keyword"
            );
        }
        assert!(!is_rust_keyword("not_a_keyword"));
        assert!(!is_rust_keyword(""));
    }
}
