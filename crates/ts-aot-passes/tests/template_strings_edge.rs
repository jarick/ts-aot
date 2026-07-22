use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(source: &str) -> (String, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", source, &mut types);
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
fn template_strings_array_cooked_with_double_quote() {
    let src = r#"function tag(s: string[], ...x: any[]): i64 { return 0; } function f(): i64 { return tag`a"b ${42}!`; }"#;
    let (mir, diags) = convert(src);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("tplstrings(cooked=["),
        "must emit tplstrings for the tagged template; got MIR:\n{mir}"
    );
    assert!(
        mir.contains("a\\\"b"),
        "cooked part with embedded `\"` must round-trip through walker (literal `\"` in source) - dump shows it as a\\\"b (debug-escaped); got MIR:\n{mir}"
    );
}

#[test]
fn template_strings_array_cooked_with_backslash_preserved() {
    let src = r#"function tag(s: string[], ...x: any[]): i64 { return 0; } function f(): i64 { return tag`a\nb ${42}!`; }"#;
    let (mir, diags) = convert(src);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("tplstrings(cooked=["),
        "cooked parts must be emitted; got MIR:\n{mir}"
    );
}

#[test]
fn template_strings_array_with_null_byte_cooked() {
    let src = "function tag(s: string[], ...x: any[]): i64 { return 0; } function f(): i64 { return tag`a\0b ${42}!`; }";
    let (mir, diags) = convert(src);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("tplstrings"),
        "TemplateStringsArray must be emitted; got MIR:\n{mir}"
    );
}
