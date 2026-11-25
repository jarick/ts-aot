use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn e0405(out: &DriverOutput) -> bool {
    out.diagnostics.errors().any(|d| d.code.as_str() == "E0405")
}

fn runs_to(package: &str, source: &str, expected: &str) {
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"got={}\", PKG::got(&ctx));
}
";
    let rust = rust_of(source);
    check_generated_crate(package, &rust);
    let main = MAIN.replace("PKG", package);
    let stdout = run_generated_crate(package, &rust, &main);
    assert_eq!(
        stdout.trim(),
        format!("got={expected}"),
        "the generated crate must print the exact expected line; got stdout:\n{stdout}"
    );
}

#[test]
fn a_parameter_passed_to_a_typed_callee_takes_the_callee_parameter_type() {
    const SOURCE: &str = "function takes(a: i32): i32 { return a; } \
         export function f(x): i32 { return takes(x); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "`x` is only ever passed to `takes(a: i32)`, so it must be inferred as `i32`, not `()`; \
         got:\n{rust}"
    );
    assert!(
        !norm.contains("x:())"),
        "the unannotated parameter must not fall back to the error type; got:\n{rust}"
    );
    check_generated_crate("param_infer_call_arg", &rust);
}

#[test]
fn a_parameter_passed_to_an_assert_shim_takes_the_shim_parameter_type() {
    const SOURCE: &str = "function assertSameValue(actual: i32, expected: i32, message: string = \"\"): i32 { return 0; } \
         export function check(x): i32 { assert.sameValue(x, 1); return 0; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "`assert.sameValue` is lowered to the shim `assertSameValue(actual: i32, ...)`, so `x` \
         must be inferred as `i32`; got:\n{rust}"
    );
    check_generated_crate("param_infer_assert_shim", &rust);
}

#[test]
fn a_parameter_assigned_a_literal_takes_the_literal_type() {
    const SOURCE: &str = "export function f(x): f64 { x = 1; return x; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:f64)"),
        "`x = 1` gives `x` the type of the numeric literal, which is `f64`; got:\n{rust}"
    );
    check_generated_crate("param_infer_assign", &rust);
}

#[test]
fn a_parameter_returned_from_an_annotated_function_takes_the_return_type() {
    const SOURCE: &str = "export function f(x): i32 { return x; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "`return x` in a function annotated `: i32` pins `x` to `i32`; got:\n{rust}"
    );
    check_generated_crate("param_infer_return", &rust);
}

#[test]
fn an_expression_bodied_arrow_pins_its_parameter_to_the_return_type() {
    const SOURCE: &str =
        "export function f(): string { const g = (x): string => x; return g(\"a\"); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !norm.contains("x:()"),
        "the arrow parameter must be inferred from its annotated return type; got:\n{rust}"
    );
    check_generated_crate("param_infer_arrow_return", &rust);
}

#[test]
fn an_async_function_returning_a_parameter_unwraps_the_promise_annotation() {
    const SOURCE: &str = "export async function f(x): Promise<i32> { return x; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "`return x` in an async function annotated `Promise<i32>` pins `x` to the resolved value \
         `i32`, not to `Promise<i32>`; got:\n{rust}"
    );
    check_generated_crate("param_infer_async_return", &rust);
}

#[test]
fn a_numeric_default_gives_the_parameter_f64_and_keeps_the_abi_optional() {
    const SOURCE: &str = "export function f(x = 0): f64 { return x + 1; }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(param_infer_default_f64::f(&ctx, ::core::option::Option::None), 1.0);
    assert_eq!(param_infer_default_f64::f(&ctx, ::core::option::Option::Some(4.0)), 5.0);
    println!(\"ok\");
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:Option<f64>)"),
        "`x = 0` must infer `f64` and keep the `T | undefined` ABI slot; got:\n{rust}"
    );
    check_generated_crate("param_infer_default_f64", &rust);
    let stdout = run_generated_crate("param_infer_default_f64", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_default_gives_the_parameter_the_string_type() {
    const SOURCE: &str = "export function f(x = \"s\"): string { return x; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:Option<ts_aot_runtime::JsString<'a>>)"),
        "`x = \"s\"` must infer the string type; got:\n{rust}"
    );
    check_generated_crate("param_infer_default_str", &rust);
}

#[test]
fn a_boolean_default_gives_the_parameter_the_bool_type() {
    const SOURCE: &str = "export function f(x = true): bool { return x; }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:Option<bool>)"),
        "`x = true` must infer `bool`; got:\n{rust}"
    );
    check_generated_crate("param_infer_default_bool", &rust);
}

#[test]
fn a_method_parameter_written_to_a_this_field_takes_the_field_type() {
    const SOURCE: &str = "class Box { value: i32; \
             constructor() { this.value = 0; } \
             set(v): void { this.value = v; } \
             get(): i32 { return this.value; } } \
         export function run(): i32 { const b = new Box(); b.set(7); return b.get(); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(param_infer_this_field::run(&ctx), 7);
    println!(\"ok\");
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("v:i32)"),
        "`this.value = v` pins `v` to the declared field type `i32`; got:\n{rust}"
    );
    check_generated_crate("param_infer_this_field", &rust);
    let stdout = run_generated_crate("param_infer_this_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_parameter_without_any_context_still_reports_e0405() {
    let out = compile("export function f(x): i32 { return 0; }");
    assert!(
        e0405(&out),
        "a parameter used in no typed position must keep the hard E0405 error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted while an untyped parameter is unresolved"
    );
}

#[test]
fn a_parameter_used_only_as_an_array_element_still_reports_e0405() {
    let out = compile("export function f(x): i32 { const a = [x]; return 0; }");
    assert!(
        e0405(&out),
        "an array element gives no expected type, so inference must refuse; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_parameter_used_only_inside_a_nested_closure_still_reports_e0405() {
    let out = compile("export function f(x): i32 { const g = () => x; return 0; }");
    assert!(
        e0405(&out),
        "a nested closure scope is not used for inference, so the parameter stays unresolved; got \
         {:?}",
        out.diagnostics
    );
}

#[test]
fn a_shadowing_local_does_not_leak_its_type_into_the_parameter() {
    let out = compile(
        "function takes(a: string): string { return a; } \
         export function f(x): i32 { { const x: string = \"s\"; takes(x); } return 0; }",
    );
    assert!(
        e0405(&out),
        "a local that shadows the parameter must not be mistaken for it; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_var_declared_below_shadows_a_module_function_for_a_call_above_it() {
    const SOURCE: &str = "function g(a: string): string { return a; } \
         export function f(x): string { const r = g(x); \
         { var g = (n: number): number => n + 1; } return r; }";
    let out = compile(SOURCE);
    assert!(
        e0405(&out),
        "the function-scoped `var g` hoists over the whole function body, so the call above its \
         block must not take the module `g` parameter type for `x`; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_try_block_shadow_does_not_discard_the_return_hint_in_finally() {
    const SOURCE: &str = "function f(x): number { try { let x = 0; } finally { return x; } } \
         export function got(): number { return f(7); }";
    runs_to("param_infer_try_scope", SOURCE, "7");
}

#[test]
fn a_catch_parameter_does_not_shadow_the_parameter_after_the_clause() {
    const SOURCE: &str = "function f(x): number { try { } catch (x) { return 0; } \
         return x; } \
         export function got(): number { return f(7); }";
    runs_to("param_infer_catch_scope", SOURCE, "7");
}

#[test]
fn a_for_var_initializer_stays_shadowed_after_the_loop() {
    let out = compile(
        "function g(a: string): string { return a; } \
         export function f(x): string { for (var g = (n: number): number => n + 1; false; ) {} \
         return g(x); }",
    );
    assert!(
        e0405(&out),
        "a function-scoped `var` in a for initializer must stay shadowed after the loop; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_for_let_initializer_does_not_shadow_the_parameter_after_the_loop() {
    const SOURCE: &str = "function f(x): number { for (let x = 0; x < 1; x++) {} return x; } \
         export function got(): number { return f(7); }";
    runs_to("param_infer_for_scope", SOURCE, "7");
}

#[test]
fn a_for_of_let_binding_does_not_shadow_the_parameter_after_the_loop() {
    const SOURCE: &str = "function f(x): number { for (const x of [0]) {} return x; } \
         export function got(): number { return f(7); }";
    runs_to("param_infer_for_of_scope", SOURCE, "7");
}

#[test]
fn a_for_var_initializer_shadowing_a_parameter_is_refused() {
    let out = compile(
        "export function f(x): i32 { for (var x = 0; false; ) { } return x; } \
         export function got(): i32 { return f(7); }",
    );
    assert!(
        e0405(&out),
        "a function-scoped `var` in a for initializer shadows the parameter, so the `return x` \
         hint must not type it; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the shadowed parameter must not reach codegen as a silently wrong value; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_for_of_var_binding_shadowing_a_parameter_is_refused() {
    let out = compile(
        "export function f(x): i32 { for (var x of [0]) { } return x; } \
         export function got(): i32 { return f(7); }",
    );
    assert!(
        e0405(&out),
        "a function-scoped `var` in a for-of binding shadows the parameter, so the `return x` \
         hint must not type it; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the shadowed parameter must not reach codegen as a silently wrong value; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_for_in_var_binding_shadowing_a_parameter_is_refused() {
    let out = compile(
        "export function f(x): i32 { for (var x in { a: 1 }) { } return x; } \
         export function got(): i32 { return f(7); }",
    );
    assert!(
        e0405(&out),
        "a function-scoped `var` in a for-in binding shadows the parameter, so the `return x` \
         hint must not type it; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the shadowed parameter must not reach codegen as a silently wrong value; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_for_var_initializer_not_shadowing_the_parameter_keeps_the_return_hint() {
    const SOURCE: &str = "export function f(x): i32 { for (var y = 0; false; ) { } return x; } \
         export function got(): i32 { return f(7); }";
    runs_to("param_infer_for_var_other_name", SOURCE, "7");
}

#[test]
fn an_asserted_assignment_does_not_offer_the_underlying_literal_type() {
    const SOURCE: &str = "export function f(x): i32 { x = 1 as i32; return x; } \
         export function got(): i32 { return f(5); }";
    runs_to("param_infer_asserted_assign", SOURCE, "1");
}

#[test]
fn conflicting_hints_leave_the_parameter_unresolved() {
    let out = compile("export function f(x): i32 { x = 1; x = \"s\"; return 0; }");
    assert!(
        e0405(&out),
        "two incompatible literal assignments must not be unified silently; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_annotated_parameter_is_never_overwritten_by_inference() {
    const SOURCE: &str = "function takes(a: i32): i32 { return a; } \
         export function f(x: i32): i32 { x = 1; return takes(x); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:i32)"),
        "an explicit annotation wins over the literal assignment hint `f64`; got:\n{rust}"
    );
    check_generated_crate("param_infer_annotated_wins", &rust);
}

#[test]
fn a_literal_hint_that_disagrees_with_the_callee_leaves_the_parameter_unresolved() {
    let out = compile(
        "function takes(a: i32): i32 { return a; } \
         export function f(x): i32 { x = 1; return takes(x); }",
    );
    assert!(
        e0405(&out),
        "`x = 1` suggests `f64` while `takes(x)` requires `i32`; the conflict must not be unified \
         silently; got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_call_argument_disagreeing_with_an_inferred_parameter_is_rejected() {
    let out = compile(
        "export function f(x = 1): f64 { return x; } \
         export function h(): f64 { return f(\"s\"); }",
    );
    let diag = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0408")
        .expect("a string argument to a parameter inferred as `f64` must report E0408");
    assert!(
        diag.message.contains("string and numeric"),
        "E0408 must name the string/numeric mismatch; got {:?}",
        diag.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a call whose argument type contradicts the inferred parameter \
         type"
    );
}

#[test]
fn a_call_argument_disagreeing_with_an_inferred_method_parameter_is_rejected() {
    let out = compile(
        "class Box { value: f64; constructor() { this.value = 1; } \
         m(x): f64 { this.value = x; return x; } } \
         export function h(): f64 { const b = new Box(); return b.m(\"s\"); }",
    );
    let diag = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == "E0408")
        .expect("a string argument to an inferred `f64` method parameter must report E0408");
    assert!(
        diag.message.contains("string and numeric"),
        "E0408 must name the string/numeric mismatch; got {:?}",
        diag.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a method call whose argument type contradicts the inferred \
         parameter type"
    );
}

#[test]
fn an_annotated_call_chain_with_agreeing_arguments_still_compiles_and_runs() {
    const SOURCE: &str = "function takes(a: f64): f64 { return a + 1; } \
         export function f(x: f64): f64 { return takes(x); } \
         export function h(): f64 { return f(1.5); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"h={}\", param_infer_annotated_chain::h(&ctx));
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:f64)"),
        "an annotated `x: f64` must stay `f64`; got:\n{rust}"
    );
    check_generated_crate("param_infer_annotated_chain", &rust);
    let stdout = run_generated_crate("param_infer_annotated_chain", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "h=2.5",
        "f(1.5) through the annotated chain must return 2.5; got stdout:\n{stdout}"
    );
}

#[test]
fn an_inferred_parameter_with_agreeing_call_sites_still_compiles_and_runs() {
    const SOURCE: &str = "function takes(a: f64): f64 { return a * 2; } \
         export function f(x): f64 { return takes(x); } \
         export function h(): f64 { return f(2.5); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"h={}\", param_infer_inferred_chain::h(&ctx));
}
";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:f64)"),
        "`x` is only ever passed to `takes(a: f64)`, so it must stay inferred as `f64`; got:\n{rust}"
    );
    check_generated_crate("param_infer_inferred_chain", &rust);
    let stdout = run_generated_crate("param_infer_inferred_chain", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "h=5",
        "f(2.5) through the inferred chain must return 5; got stdout:\n{stdout}"
    );
}

fn rejection(out: &DriverOutput, code: &str, needle: &str, context: &str) {
    let diag = out
        .diagnostics
        .errors()
        .find(|d| d.code.as_str() == code)
        .unwrap_or_else(|| {
            panic!(
                "{context} must report {code}; got diagnostics: {:?}",
                out.diagnostics
            )
        });
    assert!(
        diag.message.contains(needle),
        "{code} must contain `{needle}`; got {:?}",
        diag.message
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for {context}"
    );
}

#[test]
fn an_error_typed_call_argument_is_classified_by_the_callee_return_type() {
    let out = compile(
        "function h(): string { return \"s\"; } \
         function g(x: number): number { return x; } \
         export function f(): number { return g(h()); }",
    );
    rejection(
        &out,
        "E0408",
        "string and numeric",
        "a string-returning call passed to a numeric parameter",
    );
}

#[test]
fn an_error_typed_binary_argument_is_classified_by_the_operator() {
    let out = compile(
        "function takes(x: string): string { return x; } \
         export function f(): string { return takes(1 + 2); }",
    );
    rejection(
        &out,
        "E0408",
        "string and numeric",
        "a numeric sum passed to a string parameter",
    );
}

#[test]
fn an_error_typed_comparison_argument_is_classified_as_boolean() {
    let out = compile(
        "function g(x: number): number { return x; } \
         export function f(): number { return g(1 < 2); }",
    );
    rejection(
        &out,
        "E0410",
        "boolean",
        "a comparison passed to a numeric parameter",
    );
}

#[test]
fn an_error_typed_unary_argument_is_classified_by_the_operator() {
    let out = compile(
        "function takes(x: string): string { return x; } \
         export function f(): string { return takes(-1); }",
    );
    rejection(
        &out,
        "E0408",
        "string and numeric",
        "a negated number passed to a string parameter",
    );
}

#[test]
fn an_error_typed_math_call_argument_is_classified_as_numeric() {
    let out = compile(
        "function takes(x: string): string { return x; } \
         export function f(): string { return takes(Math.random()); }",
    );
    rejection(
        &out,
        "E0408",
        "string and numeric",
        "Math.random() passed to a string parameter",
    );
}

#[test]
fn a_constructor_argument_disagreeing_with_the_parameter_is_rejected() {
    let out = compile(
        "class C { x: number = 0; constructor(v: number) { this.x = v; } } \
         export function f(): number { return new C(\"s\").x; }",
    );
    rejection(
        &out,
        "E0408",
        "string and numeric",
        "a string constructor argument for a numeric parameter",
    );
}

#[test]
fn a_boolean_constructor_argument_disagreeing_with_the_parameter_is_rejected() {
    let out = compile(
        "class C { x: number = 0; constructor(v: number) { this.x = v; } } \
         export function f(): number { return new C(true).x; }",
    );
    rejection(
        &out,
        "E0410",
        "boolean",
        "a boolean constructor argument for a numeric parameter",
    );
}

#[test]
fn a_bigint_argument_for_a_numeric_parameter_is_rejected() {
    let out = compile(
        "function g(x: number): number { return x; } \
         export function f(): number { return g(1n); }",
    );
    rejection(
        &out,
        "E0408",
        "bigint and numeric",
        "a bigint literal passed to a numeric parameter",
    );
}

#[test]
fn a_numeric_argument_for_a_bigint_parameter_is_rejected() {
    let out = compile(
        "function g(x: bigint): bigint { return x; } \
         export function f(): bigint { return g(1); }",
    );
    rejection(
        &out,
        "E0408",
        "bigint and numeric",
        "a numeric literal passed to a bigint parameter",
    );
}

#[test]
fn a_void_argument_for_a_typed_parameter_is_rejected() {
    let out = compile(
        "function v(): void {} \
         function g(x: number): void {} \
         export function f(): void { g(v()); }",
    );
    rejection(
        &out,
        "E0410",
        "void",
        "a void call result passed to a numeric parameter",
    );
}

#[test]
fn nullish_arguments_still_report_only_p0018() {
    let out = compile(
        "function g(x: number): void {} \
         export function f(): void { g(undefined); }",
    );
    let codes: Vec<&str> = out.diagnostics.errors().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        ["P0018"],
        "an `undefined` argument must report exactly one P0018 and no other error; got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_error_typed_call_argument_with_a_matching_return_type_still_compiles_and_runs() {
    const SOURCE: &str = "function h(): string { return \"s\"; } \
         function takes(x: string): string { return x; } \
         export function f(): string { return takes(h()); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"f={}\", param_infer_call_through::f(&ctx).to_string_lossy());
}
";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_call_through", &rust);
    let stdout = run_generated_crate("param_infer_call_through", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "f=s",
        "a string-returning call passed to a string parameter must return \"s\"; \
         got stdout:\n{stdout}"
    );
}

#[test]
fn numeric_error_typed_arguments_still_compile_and_run() {
    const SOURCE: &str = "function takes(x: f64): f64 { return x * 2; } \
         export function a(): f64 { return takes(1 + 2); } \
         export function b(): f64 { return takes(-1); } \
         export function c(): f64 { return takes(true ? 3 : 4); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"a={}\", param_infer_numeric_args::a(&ctx));
    println!(\"b={}\", param_infer_numeric_args::b(&ctx));
    println!(\"c={}\", param_infer_numeric_args::c(&ctx));
}
";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_numeric_args", &rust);
    let stdout = run_generated_crate("param_infer_numeric_args", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "a=6\nb=-2\nc=6",
        "numeric sums, negations and ternaries must reach a numeric parameter; \
         got stdout:\n{stdout}"
    );
}

#[test]
fn a_matching_constructor_argument_still_compiles_and_runs() {
    const SOURCE: &str = "class C { x: number = 0; constructor(v: number) { this.x = v; } } \
         export function f(): number { return new C(2).x; }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"f={}\", param_infer_ctor_arg::f(&ctx));
}
";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_ctor_arg", &rust);
    let stdout = run_generated_crate("param_infer_ctor_arg", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "f=2",
        "new C(2).x must return 2; got stdout:\n{stdout}"
    );
}

#[test]
fn a_matching_bigint_argument_still_compiles() {
    const SOURCE: &str = "function takes(x: bigint): bigint { return x; } \
         export function f(): bigint { return takes(1n); }";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_bigint_arg", &rust);
}

#[test]
fn a_void_value_passed_to_a_void_parameter_still_compiles_and_runs() {
    const SOURCE: &str = "function v(): void {} \
         function takes(x: void): void {} \
         export function f(): void { takes(v()); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    param_infer_void_arg::f(&ctx);
    println!(\"ok\");
}
";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_void_arg", &rust);
    let stdout = run_generated_crate("param_infer_void_arg", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_dead_block_var_keeps_the_parameter_return_hint() {
    const SOURCE: &str = "export function f(x): i32 { if (false) { var x = 0; } return x; } \
         export function got(): i32 { return f(7); }";
    runs_to("param_infer_dead_var_block", SOURCE, "7");
}

#[test]
fn a_reachable_block_var_that_assigns_the_parameter_is_refused() {
    let out = compile(
        "export function f(x): i32 { if (true) { var x = 0; } return x; } \
         export function got(): i32 { return f(7); }",
    );
    assert!(
        e0405(&out),
        "a reachable `var x = 0` assigns the parameter, so the `return x` hint must not type it; \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the assigning `var` must not reach codegen as a silently wrong value; got:\n{:?}",
        out.rust_source
    );
}

#[test]
fn a_default_parameter_with_a_dead_block_var_keeps_the_hint() {
    const SOURCE: &str = "export function f(x = 7): f64 { if (false) { var x = 0; } return x; } \
         export function omitted(): f64 { return f(); } \
         export function supplied(): f64 { return f(9); }";
    const MAIN: &str = "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"omitted={}\", param_infer_default_dead_var::omitted(&ctx));
    println!(\"supplied={}\", param_infer_default_dead_var::supplied(&ctx));
}
";
    let rust = rust_of(SOURCE);
    check_generated_crate("param_infer_default_dead_var", &rust);
    let stdout = run_generated_crate("param_infer_default_dead_var", &rust, MAIN);
    assert_eq!(
        stdout.trim(),
        "omitted=7\nsupplied=9",
        "an omitted argument must take the `7` default while an explicit `f(9)` keeps `9`; \
         got stdout:\n{stdout}"
    );
}

#[test]
fn a_default_parameter_control_keeps_its_literal_type() {
    const SOURCE: &str = "export function f(x = 7): f64 { return x; } \
         export function omitted(): f64 { return f(); } \
         export function supplied(): f64 { return f(9); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:Option<f64>") || norm.contains("x:f64"),
        "a default parameter keeps its literal type; got:\n{rust}"
    );
    check_generated_crate("param_infer_default_control", &rust);
    let stdout = run_generated_crate(
        "param_infer_default_control",
        &rust,
        "
use ts_aot_runtime::{Bump, Ctx, Host};
struct NoHost;
impl Host for NoHost {}
fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!(\"omitted={}\", param_infer_default_control::omitted(&ctx));
    println!(\"supplied={}\", param_infer_default_control::supplied(&ctx));
}
",
    );
    assert_eq!(
        stdout.trim(),
        "omitted=7\nsupplied=9",
        "an omitted argument must take the `7` default while an explicit `f(9)` keeps `9`; \
         got stdout:\n{stdout}"
    );
}

#[test]
fn a_for_var_initializer_naming_an_annotated_parameter_assigns_it() {
    const SOURCE: &str = "export function f(x: i32): i32 { for (var x = 0; false; ) { } return x; } \
         export function got(): i32 { return f(7); }";
    runs_to("param_infer_for_var_param_assign", SOURCE, "0");
}

#[test]
fn a_block_var_naming_an_annotated_parameter_assigns_it() {
    const SOURCE: &str = "export function f(x: i32): i32 { { var x = 0; } return x; } \
         export function got(): i32 { return f(7); }";
    runs_to("param_infer_block_var_param_assign", SOURCE, "0");
}

#[test]
fn a_var_redeclaring_an_annotated_parameter_keeps_the_annotation() {
    const SOURCE: &str = "export function f(x: i32): i32 { var x = 5; return x; } \
         export function got(): i32 { return f(7); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("mutx:i32"),
        "`var x` names the parameter, so the parameter must stay the `i32` binding the \
         initializer is assigned to; got:\n{rust}"
    );
    assert!(
        !norm.contains("x_:i64"),
        "the redeclaration must not create a fresh `i64` local against the `i32` annotation; got:\n\
         {rust}"
    );
    runs_to("param_infer_var_param_type", SOURCE, "5");
}

#[test]
fn a_var_redeclaring_a_parameter_with_an_incompatible_initializer_is_refused() {
    let out = compile("export function f(x: i32): i32 { var x = \"s\"; return x; }");
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0408"),
        "a string initializer for an `i32` parameter must be a hard E0408; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the refused redeclaration must not emit Rust; got:\n{:?}",
        out.rust_source
    );
}
