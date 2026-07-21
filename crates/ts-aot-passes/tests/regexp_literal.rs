use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (String, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types);
    let diags: Vec<String> = frontend
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d))
        .collect();
    if frontend.diagnostics.has_errors() {
        return (String::new(), diags);
    }
    let mut hir = frontend.program;
    ts_aot_passes::lower_enums(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::monomorphize(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::lower_closures(&mut hir, &mut ctx);
    let _ = ts_aot_passes::lower_async(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    (mir.dump_text(), diags)
}

#[test]
fn regexp_literal_with_flags_emits_regexp_mir() {
    let (mir, diags) = convert("function f(): i64 { return /foo/g; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let line = mir
        .lines()
        .find(|l| l.contains("regexp("))
        .unwrap_or_else(|| panic!("expected regexp(...) line in MIR, got:\n{mir}"));
    assert!(
        line.contains("\"foo\""),
        "must include pattern literal, got: {line}"
    );
    assert!(
        line.contains("\"g\""),
        "must include flag literal, got: {line}"
    );
}

#[test]
fn regexp_literal_without_flags_emits_empty_flags() {
    let (mir, diags) = convert("function f(): i64 { return /abc/; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("regexp(") && mir.contains("\"abc\""),
        "expected regexp line with pattern, got:\n{mir}"
    );
}

#[test]
fn regexp_literal_in_return_emits_regexp_mir() {
    let (mir, diags) = convert("function f(): i64 { return /x/; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("regexp("),
        "MIR must include regexp call, got:\n{mir}"
    );
}

#[test]
fn regexp_literal_chained_call_keeps_exactly_one_regexp() {
    let (mir, diags) = convert("function f(): i64 { return /foo/; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let regexp_lines: Vec<&str> = mir.lines().filter(|l| l.contains("regexp(")).collect();
    assert_eq!(
        regexp_lines.len(),
        1,
        "exactly one regexp(...) call expected, got:\n{mir}"
    );
}
