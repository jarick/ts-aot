use ts_aot_driver::{CompileOptions, Driver, DriverOutput};
use ts_aot_test_support::check_generated_crate;

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            module: false,
            ..CompileOptions::default()
        },
    )
}

#[test]
fn unannotated_function_uses_its_inferred_return_type() {
    let out = compile("export function h() { return 7; } export function run(): void { h(); }");
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("infer_fn_ret", &rust_raw);
}

#[test]
fn unannotated_closure_uses_its_inferred_return_type() {
    let out = compile("export function run(): void { const g = () => { return 7; }; g; }");
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("infer_closure_ret", &rust_raw);
}

#[test]
fn unannotated_expression_bodied_arrow_uses_its_inferred_return_type() {
    let out = compile("export function run(): void { const g = () => 7; g; }");
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("infer_arrow_ret", &rust_raw);
}

#[test]
fn unannotated_capturing_closure_uses_its_inferred_return_type() {
    let out = compile(
        "export function run(): void { const s: string = \"a\"; const g = () => { s; return 7; }; g; }",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("infer_capturing_closure_ret", &rust_raw);
}

#[test]
fn annotated_return_types_are_not_overwritten_by_inference() {
    let out =
        compile("export function h(): number { return 7; } export function run(): void { h(); }");
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    check_generated_crate("annotated_ret_wins", &rust_raw);
}

#[test]
fn unannotated_method_uses_its_inferred_return_type() {
    let out = compile(
        "class C { m() { return 7; } } \
         export function run(): number { const c = new C(); return c.m(); }",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust_raw.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains("-> i64"),
        "an unannotated method must use its inferred `i64` return type, not the error marker; \
         got:\n{rust_raw}"
    );
    check_generated_crate("infer_method_ret", &rust_raw);
}

#[test]
fn function_that_can_fall_through_is_reported_as_an_error_not_broken_rust() {
    let out = compile("export function f(x: number) { if (x > 0) { return 1; } }");
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.severity == ts_aot_driver::Severity::Error),
        "a value-returning function whose return type cannot be determined from its body must be \
         reported as an error instead of emitting `return 1` inside a `-> ()` function (E0308); \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust must be emitted for a function whose return type cannot be determined; got:\n{:?}",
        out.rust_source
    );
}

/// The un-inferable return type is reported where the function is, not by the
/// backend without a location (`E0300` at span `0..0`) and not as a misleading
/// "could not resolve type annotation" warning.
#[test]
fn un_inferable_return_type_is_reported_at_the_function_with_e0402() {
    let out = compile("export function f(x: number) { if (x > 0) { return 1; } }");
    let diag = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0415")
        .unwrap_or_else(|| panic!("expected E0415, got {:?}", out.diagnostics));
    assert_eq!(diag.severity, ts_aot_driver::Severity::Error);
    assert!(
        diag.span.end > diag.span.start,
        "the diagnostic must point at the function, not at an empty span; got {:?}",
        diag.span
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0300"),
        "the backend must not be the one reporting this; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0400"),
        "an absent annotation is not an unresolved annotation; got {:?}",
        out.diagnostics
    );
}

#[test]
fn unannotated_function_returning_a_declared_call_infers_the_callee_type() {
    let out = compile("function g(): number { return 1; } export function f() { return g(); }");
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust_raw.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains("pub fn f < 'a > (__ctx : & ts_aot_runtime :: Ctx < 'a > ,) -> f64"),
        "`return g()` must infer `f64` from the declared callee; got:\n{rust_raw}"
    );
    check_generated_crate("infer_call_ret", &rust_raw);
}

#[test]
fn unannotated_async_function_infers_its_promise_type() {
    let out = compile(
        "export async function f() { return 1; } \
         export async function g() {}",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust_raw.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains("-> ts_aot_runtime::Promise<i64>") || norm.contains("Promise < i64 >"),
        "an unannotated async function must infer `Promise<i64>` from `return 1`; got:\n{rust_raw}"
    );
    assert!(
        norm.contains("Promise<()>") || norm.contains("Promise < () >"),
        "an async function without value returns must be `Promise<()>`; got:\n{rust_raw}"
    );
    assert!(
        !norm.contains("unreachable"),
        "an empty async body must not lower to `unreachable!()`; got:\n{rust_raw}"
    );
    check_generated_crate("infer_async_ret", &rust_raw);
}

#[test]
fn unannotated_void_function_does_not_warn() {
    let out = compile(
        "export function f(): void { const x: number = 1; use(x); } \
         function use(x: number): void { x; }",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0400" || d.code.as_str() == "E0415"),
        "a function without value returns is `void`; nothing to warn about; got {:?}",
        out.diagnostics
    );
}

#[test]
fn unannotated_void_function_is_lowered_as_void() {
    let out = compile(
        "function use(x: number): void { x; } \
         export function f() { use(1); }",
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust_raw = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust_raw.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains("pub fn f < 'a > (__ctx : & ts_aot_runtime :: Ctx < 'a > ,) -> ()"),
        "a function without value returns must lower to `-> ()`; got:\n{rust_raw}"
    );
    check_generated_crate("infer_void_ret", &rust_raw);
}
