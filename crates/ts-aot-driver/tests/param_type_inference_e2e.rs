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
    let out = compile("export function f(x): i32 { { const x: string = \"s\"; x; } return 0; }");
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
         export function f(x: string): i32 { return takes(0); }";
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x:ts_aot_runtime::JsString<'a>)"),
        "an explicit annotation wins over any contextual hint; got:\n{rust}"
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
