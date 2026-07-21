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
fn bigint_literal_emits_bigint_mir() {
    let (mir, diags) = convert("function f(): i64 { return 42n; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let line = mir
        .lines()
        .find(|l| l.contains("bigint("))
        .unwrap_or_else(|| panic!("expected bigint(...) line in MIR, got:\n{mir}"));
    assert!(
        line.contains("\"42\""),
        "must include value literal, got: {line}"
    );
}

#[test]
fn bigint_literal_large_value_const_folds() {
    let (mir, diags) = convert("function f(): i64 { return 99999999999999999999n; }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("bigint(") && mir.contains("\"99999999999999999999\""),
        "expected bigint line with const-folded value, got:\n{mir}"
    );
}
