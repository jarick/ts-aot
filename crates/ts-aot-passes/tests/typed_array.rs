use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (String, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types);
    let mut diags: Vec<String> = frontend
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
    diags.extend(ctx.diagnostics().iter().map(|d| format!("{:?}", d)));
    (mir.dump_text(), diags)
}

#[test]
fn new_int8_array_lowers_to_typed_array_new_runtime_op() {
    let (mir, diags) = convert("function f(): Int8Array { return new Int8Array(8); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("typed_array_new(8"),
        "new Int8Array(8) must pass length=8 as first arg, got:\n{mir}"
    );
    assert!(
        mir.contains("typed_array_new(8(:0), 0(:0))"),
        "Int8Array kind_id must be 0 (second arg), got:\n{mir}"
    );
}

#[test]
fn new_uint8_clamped_array_lowers_to_typed_array_new_with_kind_id_2() {
    let (mir, diags) =
        convert("function f(): Uint8ClampedArray { return new Uint8ClampedArray(16); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("typed_array_new(16"),
        "new Uint8ClampedArray(16) must pass length=16 as first arg, got:\n{mir}"
    );
    assert!(
        mir.contains("typed_array_new(16(:0), 2(:0))"),
        "Uint8ClampedArray kind_id must be 2 (second arg), got:\n{mir}"
    );
}

#[test]
fn new_float64_array_lowers_to_typed_array_new_with_kind_id_8() {
    let (mir, diags) = convert("function f(): Float64Array { return new Float64Array(4); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("typed_array_new(4"),
        "new Float64Array(4) must pass length=4 as first arg, got:\n{mir}"
    );
    assert!(
        mir.contains("typed_array_new(4(:0), 8(:0))"),
        "Float64Array kind_id must be 8 (second arg), got:\n{mir}"
    );
}

#[test]
fn new_typed_array_with_wrong_arity_emits_e0406() {
    let (_, diags) = convert("function f(): Int8Array { return new Int8Array(4, 8); }");
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("TypedArray"));
    assert!(
        has_e0406,
        "new Int8Array(4, 8) with 2 args must emit E0406 about arity, got: {diags:?}"
    );
}

#[test]
fn new_typed_array_with_non_numeric_length_emits_e0406() {
    let (_, diags) = convert(r#"function f(): Int8Array { return new Int8Array("hello"); }"#);
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && (d.contains("number") || d.contains("numeric")));
    assert!(
        has_e0406,
        "new Int8Array(\"hello\") with non-numeric length must emit E0406, got: {diags:?}"
    );
}
