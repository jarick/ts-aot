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
fn symbol_call_lowers_to_symbol_new_runtime_op() {
    let (mir, diags) = convert("function f(): Symbol { return Symbol(); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("symbol_new()"),
        "Symbol() must lower to runtime call __ts_aot_symbol_new; got:\n{mir}"
    );
}

#[test]
fn symbol_call_with_string_arg_lowers_to_symbol_new_desc() {
    let (mir, diags) = convert(r#"function f(): Symbol { return Symbol("hello"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("symbol_new(") && mir.contains("string(\"hello\")"),
        "Symbol(\"hello\") must lower to `symbol_new(string(\"hello\"))` with the description \
         argument present in the runtime op args; got:\n{mir}"
    );
}

#[test]
fn symbol_for_lowers_to_runtime_call_with_jsstring_arg() {
    let (mir, diags) = convert(r#"function f(): Symbol { return Symbol.for("shared"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("symbol_for("),
        "Symbol.for(\"shared\") must lower to runtime call __ts_aot_symbol_for; got:\n{mir}"
    );
}

#[test]
fn symbol_key_for_lowers_to_runtime_call_with_symbol_arg() {
    let (mir, diags) = convert(r#"function f(s: Symbol): string { return Symbol.keyFor(s); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("symbol_key_for("),
        "Symbol.keyFor(s) must lower to runtime call __ts_aot_symbol_key_for; got:\n{mir}"
    );
}

#[test]
fn symbol_call_with_too_many_args_emits_e0406() {
    let (_, diags) = convert(r#"function f(): Symbol { return Symbol("a", "b"); }"#);
    let arity_e0406: Vec<&str> = diags
        .iter()
        .filter(|d| d.contains("E0406") && d.contains("Symbol()") && d.contains("0 or 1 argument"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        arity_e0406.len(),
        1,
        "Symbol(\"a\", \"b\") must emit exactly one E0406 mentioning Symbol() and 0-or-1 \
         argument arity contract; got {arity_e0406:?} (all diags: {diags:?})"
    );
}

#[test]
fn symbol_for_with_non_string_arg_emits_e0406() {
    let (_, diags) = convert("function f(x: i64): Symbol { return Symbol.for(x); }");
    let e0406: Vec<&str> = diags
        .iter()
        .filter(|d| d.contains("E0406") && d.contains("Symbol.for"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        e0406.len(),
        1,
        "Symbol.for(i64) must emit exactly one E0406 about Symbol.for; got {e0406:?} (all diags: {diags:?})"
    );
    assert!(
        e0406[0].contains("string") && e0406[0].contains("Symbol.for"),
        "Symbol.for(i64) E0406 must mention both 'string' (expected type) and 'Symbol.for' (offending call); got: {}",
        e0406[0]
    );
}

#[test]
fn symbol_key_for_with_non_symbol_arg_emits_e0406() {
    let (_, diags) = convert("function f(s: string): string { return Symbol.keyFor(s); }");
    let e0406: Vec<&str> = diags
        .iter()
        .filter(|d| d.contains("E0406") && d.contains("Symbol.keyFor"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        e0406.len(),
        1,
        "Symbol.keyFor(string) must emit exactly one E0406 about Symbol.keyFor; got {e0406:?} (all diags: {diags:?})"
    );
    assert!(
        e0406[0].contains("Symbol") && e0406[0].contains("Symbol.keyFor"),
        "Symbol.keyFor(string) E0406 must mention 'Symbol' (expected type) and 'Symbol.keyFor' (offending call); got: {}",
        e0406[0]
    );
}
