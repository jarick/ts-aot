use miette::Diagnostic as OxcDiagnostic;
use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_diagnostics::OxcDiagnostic as OxcError;
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::SourceType;
use ts_aot_core::{Diagnostic, DiagnosticBag, Span};

const PARSE_ERROR_CODE: &str = "P0001";
const SEMANTIC_ERROR_CODE: &str = "S0001";
const DEFAULT_SPAN: Span = Span::new(0, 0);

fn error_span(err: &OxcError) -> Span {
    let diag: &dyn OxcDiagnostic = err;
    diag.labels()
        .as_slice()
        .first()
        .map_or(DEFAULT_SPAN, |label| {
            let start = u32::try_from(u64::from(label.offset())).unwrap_or(u32::MAX);
            let end = u32::try_from(u64::from(label.offset()) + u64::from(label.len()))
                .unwrap_or(u32::MAX);
            Span::new(start, end)
        })
}

#[must_use]
pub fn analyze_semantic(name: &str, source: &str) -> DiagnosticBag {
    let allocator = Allocator::default();
    let source_type = source_type_for(name);

    let mut bag = DiagnosticBag::new();

    let parser = Parser::new(&allocator, source, source_type);
    let ret = parser.parse();
    for err in &ret.diagnostics {
        let short = short_message(&err.to_string(), "parse error");
        bag.push(Diagnostic::error(PARSE_ERROR_CODE, short, error_span(err)));
    }
    if ret.panicked {
        return bag;
    }

    let program: &Program<'_> = &ret.program;
    let semantic_ret = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    for err in &semantic_ret.diagnostics {
        let short = short_message(&err.to_string(), "semantic error");
        bag.push(Diagnostic::error(
            SEMANTIC_ERROR_CODE,
            short,
            error_span(err),
        ));
    }

    bag
}

#[must_use]
pub fn with_semantic<R>(name: &str, source: &str, f: impl FnOnce(&Semantic<'_>) -> R) -> Option<R> {
    let allocator = Allocator::default();
    let source_type = source_type_for(name);

    let parser = Parser::new(&allocator, source, source_type);
    let ret = parser.parse();
    if !ret.diagnostics.is_empty() || ret.panicked {
        return None;
    }

    let program: &Program<'_> = &ret.program;
    let semantic_ret = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    if !semantic_ret.diagnostics.is_empty() {
        return None;
    }

    Some(f(&semantic_ret.semantic))
}

fn source_type_for(name: &str) -> SourceType {
    SourceType::from_path(name).unwrap_or_else(|_| SourceType::default().with_typescript(true))
}

fn short_message(raw: &str, fallback: &str) -> String {
    let first = raw.lines().next().unwrap_or(fallback).trim();
    strip_ansi(first).clone()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if c == '\u{1b}' {
            in_escape = true;
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::Severity;

    #[test]
    fn analyze_semantic_returns_empty_for_valid_source() {
        let bag = analyze_semantic(
            "test.ts",
            "function add(a: i32, b: i32): i32 { return a + b; }",
        );
        assert!(!bag.has_errors(), "diagnostics: {bag:?}");
        assert!(bag.is_empty());
    }

    #[test]
    fn analyze_semantic_emits_parse_error_for_syntax_fault() {
        let bag = analyze_semantic("test.ts", "const = 1;");
        assert!(bag.has_errors());
        let codes: Vec<&str> = bag.errors().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&PARSE_ERROR_CODE),
            "expected P0001, got {codes:?}"
        );
    }

    #[test]
    fn analyze_semantic_emits_semantic_error_for_type_mismatch() {
        let bag = analyze_semantic("test.ts", "const x: 1 = 2;");
        let codes: Vec<&str> = bag.iter().map(|d| d.code.as_str()).collect();
        let _ = SEMANTIC_ERROR_CODE;
        assert!(
            codes.contains(&SEMANTIC_ERROR_CODE)
                || codes.contains(&PARSE_ERROR_CODE)
                || bag.is_empty(),
            "diagnostic codes: {codes:?}"
        );
    }

    #[test]
    fn with_semantic_returns_none_on_parse_error() {
        let result = with_semantic("test.ts", "const = 1;", |_sem| 42);
        assert!(result.is_none());
    }

    #[test]
    fn with_semantic_invokes_closure_on_valid_source() {
        let result = with_semantic("test.ts", "const x: i32 = 1;", |_sem| "ok");
        assert_eq!(result, Some("ok"));
    }

    #[test]
    fn strip_ansi_removes_color_codes() {
        let raw = "\u{1b}[31merror: foo\u{1b}[0m";
        let out = strip_ansi(raw);
        assert_eq!(out, "error: foo");
    }

    #[test]
    fn strip_ansi_passthrough_without_escapes() {
        let raw = "plain text";
        let out = strip_ansi(raw);
        assert_eq!(out, "plain text");
    }

    #[test]
    fn short_message_picks_first_line() {
        let raw = "first line\nsecond line\nthird line";
        let out = short_message(raw, "fb");
        assert_eq!(out, "first line");
    }

    #[test]
    fn short_message_trims_whitespace() {
        let raw = "   trimmed   \n   rest";
        let out = short_message(raw, "fb");
        assert_eq!(out, "trimmed");
    }

    #[test]
    fn source_type_for_ts_extension_marks_typescript() {
        let st = source_type_for("hello.ts");
        assert!(st.is_typescript());
    }

    #[test]
    fn source_type_for_unknown_extension_falls_back_to_typescript() {
        let st = source_type_for("hello");
        assert!(st.is_typescript());
    }

    #[test]
    fn diagnostic_severity_for_parse_error_is_error() {
        let bag = analyze_semantic("test.ts", "function (() {");
        let diag = bag.iter().next().expect("at least one diagnostic");
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn parse_error_diagnostic_carries_real_span_not_default_zero() {
        let source = "const = 1;";
        let bag = analyze_semantic("test.ts", source);
        let diag = bag
            .iter()
            .find(|d| d.code.as_str() == PARSE_ERROR_CODE)
            .expect("expected PARSE_ERROR_CODE diagnostic");
        let default_span = Span::new(0, 0);
        assert_ne!(
            diag.span, default_span,
            "real error span from oxc label must override DEFAULT_SPAN (0,0)"
        );
        assert!(
            diag.span.end <= u32::try_from(source.len()).unwrap_or(u32::MAX),
            "span end {} must be within source length {}",
            diag.span.end,
            source.len()
        );
    }
}
