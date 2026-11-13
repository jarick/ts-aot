use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_rust(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn run_case(package: &str, source: &str, expected: f64) {
    let out = compile_rust(source);
    assert!(
        !out.has_errors(),
        "{package} must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate(package, &rust);
    let main = format!(
        r#"
use ts_aot_runtime::{{Bump, Ctx, Host}};

struct NoHost;
impl Host for NoHost {{}}

fn main() {{
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx {{ arena: &bump, host: &host }};
    assert_eq!({package}::got(&ctx), {expected:?});
    println!("ok");
}}
"#
    );
    let stdout = run_generated_crate(package, &rust, &main);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_captured_local_arrow_is_invoked_from_a_nested_closure() {
    run_case(
        "captured_callable_local_arrow",
        "export function got(): number { \
             const h = (): number => 5; \
             const inner = (): number => h(); \
             return inner(); \
         }",
        5.0,
    );
}

#[test]
fn a_captured_parameter_is_invoked_from_a_nested_closure() {
    run_case(
        "captured_callable_parameter",
        "function wrap(f: () => number): number { \
             const inner = (): number => f(); \
             return inner(); \
         } \
         export function got(): number { return wrap((): number => 7); }",
        7.0,
    );
}

#[test]
fn an_aliased_captured_closure_is_invoked_from_a_nested_closure() {
    run_case(
        "captured_callable_alias",
        "export function got(): number { \
             const h = (): number => 3; \
             const inner = (): number => h(); \
             const alias = inner; \
             return alias(); \
         }",
        3.0,
    );
}

#[test]
fn a_captured_callable_with_an_argument_is_invoked_from_a_nested_closure() {
    run_case(
        "captured_callable_argument",
        "export function got(): number { \
             const f: (n: number) => number = (n: number): number => n + 1; \
             const inner = (): number => f(4); \
             return inner(); \
         }",
        5.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_inside_an_if() {
    run_case(
        "captured_callable_if",
        "export function got(): number { \
             const h = (): number => 9; \
             const inner = (): number => { \
                 if (true) { return h(); } \
                 return 0; \
             }; \
             return inner(); \
         }",
        9.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_through_an_iife_wrapper() {
    run_case(
        "captured_callable_iife",
        "export function got(): number { \
             const f = (): number => 6; \
             return ((): number => f())(); \
         }",
        6.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_from_a_method_wrapper() {
    run_case(
        "captured_callable_method_wrapper",
        "class W { run(): number { \
             const h = (): number => 8; \
             const inner = (): number => h(); \
             return inner(); \
         } } \
         export function got(): number { return new W().run(); }",
        8.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_from_a_nested_arrow_wrapper() {
    run_case(
        "captured_callable_nested_arrow_wrapper",
        "export function got(): number { \
             const h = (): number => 4; \
             const outer = (): number => { \
                 const inner = (): number => h(); \
                 return inner(); \
             }; \
             return outer(); \
         }",
        4.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_from_a_nested_method_wrapper() {
    run_case(
        "captured_callable_nested_method_wrapper",
        "class W { run(): number { \
             const h = (): number => 8; \
             const outer = (): number => { \
                 const inner = (): number => h(); \
                 return inner(); \
             }; \
             return outer(); \
         } } \
         export function got(): number { return new W().run(); }",
        8.0,
    );
}

#[test]
fn a_captured_callable_is_invoked_from_a_three_level_arrow_wrapper() {
    run_case(
        "captured_callable_three_level_wrapper",
        "export function got(): number { \
             const h = (): number => 4; \
             const a = (): number => { \
                 const b = (): number => { \
                     const c = (): number => h(); \
                     return c(); \
                 }; \
                 return b(); \
             }; \
             return a(); \
         }",
        4.0,
    );
}
