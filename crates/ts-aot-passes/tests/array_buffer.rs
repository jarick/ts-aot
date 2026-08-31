use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (String, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types, false);
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
    ts_aot_passes::lower_closures(&mut hir, &mut types, &mut ctx);
    let _ = ts_aot_passes::lower_async(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    diags.extend(ctx.diagnostics().iter().map(|d| format!("{:?}", d)));
    (mir.dump_text(), diags)
}

#[test]
fn new_array_buffer_lowers_to_array_buffer_new_runtime_op() {
    let (mir, diags) = convert("function f(): ArrayBuffer { return new ArrayBuffer(16); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("array_buffer_new"),
        "new ArrayBuffer(16) must lower to runtime call __ts_aot_array_buffer_new; got:\n{mir}"
    );
    assert!(
        mir.contains("array_buffer_new(16"),
        "new ArrayBuffer(16) must pass 16 as byteLength argument; got:\n{mir}"
    );
}

#[test]
fn new_array_buffer_with_zero_size_lowers_to_array_buffer_new() {
    let (mir, diags) = convert("function f(): ArrayBuffer { return new ArrayBuffer(0); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("array_buffer_new(0"),
        "new ArrayBuffer(0) must pass 0 as byteLength argument; got:\n{mir}"
    );
}

#[test]
fn new_array_buffer_with_wrong_arity_emits_e0406() {
    let (_, diags) = convert("function f(): ArrayBuffer { return new ArrayBuffer(); }");
    let arity_e0406: Vec<&str> = diags
        .iter()
        .filter(|d| {
            d.contains("E0406")
                && d.contains("new ArrayBuffer(byteLength)")
                && d.contains("exactly 1 argument")
        })
        .map(String::as_str)
        .collect();
    assert_eq!(
        arity_e0406.len(),
        1,
        "new ArrayBuffer() with 0 args must emit exactly one E0406 about arity; got {arity_e0406:?}"
    );
}

#[test]
fn new_array_buffer_with_non_numeric_arg_emits_e0406() {
    let (_, diags) = convert("function f(s: string): ArrayBuffer { return new ArrayBuffer(s); }");
    let e0406: Vec<&str> = diags
        .iter()
        .filter(|d| d.contains("E0406") && d.contains("ArrayBuffer(byteLength)"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        e0406.len(),
        1,
        "new ArrayBuffer(string) must emit exactly one E0406 mentioning ArrayBuffer(byteLength); got {e0406:?}"
    );
}

#[test]
fn array_buffer_slice_method_lowers_to_array_buffer_slice_runtime_op() {
    let (mir, diags) = convert("function f(b: ArrayBuffer): ArrayBuffer { return b.slice(2, 5); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("array_buffer_slice"),
        "b.slice(2, 5) must lower to runtime call __ts_aot_array_buffer_slice; got:\n{mir}"
    );
}

#[test]
fn array_buffer_slice_method_with_no_args_lowers_to_slice_with_end_default() {
    let (mir, diags) = convert("function f(b: ArrayBuffer): ArrayBuffer { return b.slice(0); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("array_buffer_slice"),
        "b.slice(0) must lower to runtime call __ts_aot_array_buffer_slice; got:\n{mir}"
    );
}

#[test]
fn array_buffer_slice_method_on_non_arraybuffer_does_not_dispatch() {
    let (mir, diags) = convert("function f(b: string): string { return b.slice(0, 1); }");
    assert!(
        !diags.is_empty(),
        "b.slice(0, 1) on string receiver must surface diagnostics (E0406 or similar), \
         not silently pass; got: {diags:?} / mir:\n{mir}"
    );
}

#[test]
fn array_buffer_slice_method_with_too_many_args_emits_e0406() {
    let (_, diags) =
        convert("function f(b: ArrayBuffer): ArrayBuffer { return b.slice(0, 1, 2); }");
    let e0406: Vec<&str> = diags
        .iter()
        .filter(|d| d.contains("E0406") && d.contains("ArrayBuffer.prototype.slice"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        e0406.len(),
        1,
        "b.slice(0, 1, 2) must emit exactly one E0406 mentioning ArrayBuffer.prototype.slice; got {e0406:?}"
    );
}
