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
fn json_parse_i64_emits_runtime_call() {
    let (mir, diags) = convert(r#"function f(): i64 { return JSON.parse<i64>("42"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_parse(") && mir.contains(") -> T#0"),
        "JSON.parse<i64>(\"42\") must lower to a json_parse statement with exact i64 type id T#0 \
         (T#0 = I64, not any T#N sentinel); got:\n{mir}"
    );
}

#[test]
fn json_parse_with_unsupported_target_type_emits_e0406() {
    let (_mir, diags) = convert("function f(s: string): never { return JSON.parse<never>(s); }");
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.parse") && d.contains("target type"));
    assert!(
        has_e0406,
        "JSON.parse<never>(s) must emit E0406 via is_json_supported_target_type (Type::Never is not in MVP whitelist); got: {diags:?}"
    );
}

#[test]
fn json_parse_f64_emits_runtime_call() {
    let (mir, diags) = convert(r#"function f(): f64 { return JSON.parse<f64>("3.5"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_parse("),
        "JSON.parse<f64>(\"3.5\") must lower to __ts_aot_json_parse, got:\n{mir}"
    );
}

#[test]
fn json_parse_string_emits_runtime_call() {
    let (mir, diags) = convert(r#"function f(): string { return JSON.parse<string>("\"hi\""); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_parse_string("),
        "JSON.parse<string> must lower to __ts_aot_json_parse_string (surrogate-preserving path), got:\n{mir}"
    );
}

#[test]
fn json_parse_bool_emits_runtime_call() {
    let (mir, diags) = convert(r#"function f(): bool { return JSON.parse<bool>("true"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_parse("),
        "JSON.parse<bool>(\"true\") must lower to __ts_aot_json_parse, got:\n{mir}"
    );
}

#[test]
fn json_parse_vec_i64_emits_runtime_call() {
    let (mir, diags) =
        convert(r#"function f(): Array<i64> { return JSON.parse<Array<i64>>("[1,2,3]"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_parse("),
        "JSON.parse<Array<i64>> must lower to __ts_aot_json_parse, got:\n{mir}"
    );
}

#[test]
fn json_stringify_i64_emits_runtime_call() {
    let (mir, diags) = convert("function f(n: i64): string { return JSON.stringify(n); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_stringify("),
        "JSON.stringify(i64) must lower to __ts_aot_json_stringify, got:\n{mir}"
    );
}

#[test]
fn json_stringify_f64_emits_runtime_call() {
    let (mir, diags) = convert("function f(n: f64): string { return JSON.stringify(n); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_stringify("),
        "JSON.stringify(f64) must lower to __ts_aot_json_stringify, got:\n{mir}"
    );
}

#[test]
fn json_stringify_string_emits_runtime_call() {
    let (mir, diags) = convert(r#"function f(s: string): string { return JSON.stringify(s); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_stringify_string("),
        "JSON.stringify(string) must lower to __ts_aot_json_stringify_string (surrogate-preserving path), got:\n{mir}"
    );
}

#[test]
fn json_stringify_vec_i64_emits_runtime_call() {
    let (mir, diags) = convert("function f(v: Array<i64>): string { return JSON.stringify(v); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("json_stringify("),
        "JSON.stringify(Array<i64>) must lower to __ts_aot_json_stringify, got:\n{mir}"
    );
}

#[test]
fn json_parse_without_type_arg_emits_e0406() {
    let (mir, diags) = convert(r#"function f(): i64 { return JSON.parse("42"); }"#);
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.parse") && d.contains("type argument"));
    assert!(
        has_e0406,
        "JSON.parse without <T> must emit E0406 with explicit reason mentioning type argument; got: {diags:?}"
    );
    assert!(
        !mir.contains("json_parse("),
        "JSON.parse without <T> must NOT emit runtime call, got:\n{mir}"
    );
}

#[test]
fn json_stringify_with_untyped_int_literal_emits_e0406() {
    let (mir, diags) = convert("function f(): string { return JSON.stringify(42); }");
    let has_e0406 = diags.iter().any(|d| {
        d.contains("E0406") && d.contains("JSON.stringify") && d.contains("unresolvable type")
    });
    assert!(
        has_e0406,
        "JSON.stringify(42) with untyped int literal must emit E0406 with explicit reason about unresolvable value type; got: {diags:?}"
    );
    assert!(
        !mir.contains("json_stringify("),
        "JSON.stringify(42) must NOT emit runtime call (rejected before dispatch), got:\n{mir}"
    );
}

#[test]
fn json_parse_with_non_string_arg_emits_e0406() {
    let (_mir, diags) = convert("function f(): i64 { return JSON.parse<i64>(123); }");
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.parse"));
    assert!(
        has_e0406,
        "JSON.parse<i64>(123) must emit E0406 about non-string arg, got: {diags:?}"
    );
}

#[test]
fn json_parse_with_no_args_emits_e0406() {
    let (_mir, diags) = convert("function f(): i64 { return JSON.parse<i64>(); }");
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.parse"));
    assert!(
        has_e0406,
        "JSON.parse<i64>() with 0 args must emit E0406, got: {diags:?}"
    );
}

#[test]
fn json_parse_with_two_args_emits_e0406() {
    let (_mir, diags) =
        convert(r#"function f(): i64 { return JSON.parse<i64>("42", "reviver"); }"#);
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.parse"));
    assert!(
        has_e0406,
        "JSON.parse<i64>(text, reviver) with 2 args must emit E0406, got: {diags:?}"
    );
}

#[test]
fn json_stringify_with_no_args_emits_e0406() {
    let (_mir, diags) = convert("function f(): string { return JSON.stringify(); }");
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.stringify"));
    assert!(
        has_e0406,
        "JSON.stringify() with 0 args must emit E0406, got: {diags:?}"
    );
}

#[test]
fn json_stringify_with_two_args_emits_e0406() {
    let (_mir, diags) =
        convert(r#"function f(n: i64): string { return JSON.stringify(n, "space"); }"#);
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("JSON.stringify"));
    assert!(
        has_e0406,
        "JSON.stringify(value, replacer) with 2 args must emit E0406, got: {diags:?}"
    );
}
