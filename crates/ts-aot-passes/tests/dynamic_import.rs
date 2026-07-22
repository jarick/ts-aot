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
fn dynamic_import_string_source_emits_direct_not_dynfrom() {
    let (mir, diags) = convert("function f(): i64 { return import('./mod.js'); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let import_line = mir
        .lines()
        .find(|l| l.starts_with("return import(") || l.contains(" return import("))
        .unwrap_or_else(|| panic!("expected return import(...) line, got:\n{mir}"));
    assert!(
        !import_line.contains("dynfrom("),
        "strict AOT must NOT wrap import source in DynamicFrom, got: {import_line}"
    );
    assert!(
        import_line.contains("\"./mod.js\""),
        "source literal must appear in import arg, got: {import_line}"
    );
}
