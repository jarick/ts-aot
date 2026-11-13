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

const COUNTER: &str = "class Counter { n: number = 0; add(x: number): void { \
                       this.n = this.n + x; } } let g = new Counter();";

#[test]
fn the_reported_repro_forwards_a_closure_parameter_to_a_captured_callable() {
    run_case(
        "fcp_reported_repro",
        &format!(
            "{COUNTER} export function got(): number {{ \
                 const w1 = (cb: () => number): number => cb(); \
                 const w2 = (cb: () => number): number => w1(cb); \
                 g.add(w2((): number => 1)); \
                 return g.n; \
             }}"
        ),
        1.0,
    );
}

#[test]
fn the_second_wrapper_forwards_an_inline_arrow_argument() {
    run_case(
        "fcp_inline_arrow_argument",
        "export function got(): number { \
             const w1 = (cb: () => number): number => cb(); \
             const w2 = (cb: () => number): number => w1(cb); \
             return w2((): number => 1); \
         }",
        1.0,
    );
}

#[test]
fn the_second_wrapper_forwards_a_local_arrow_argument() {
    run_case(
        "fcp_local_arrow_argument",
        "export function got(): number { \
             const w1 = (cb: () => number): number => cb(); \
             const w2 = (cb: () => number): number => w1(cb); \
             const f = (): number => 1; \
             return w2(f); \
         }",
        1.0,
    );
}

#[test]
fn the_second_wrapper_forwards_the_first_wrapper_and_one_is_added() {
    run_case(
        "fcp_first_wrapper_as_argument",
        "export function got(): number { \
             const w1 = (): number => 1; \
             const w2 = (cb: () => number): number => cb(); \
             return w2(w1) + 1; \
         }",
        2.0,
    );
}

#[test]
fn three_levels_of_forwarding_keep_the_parameter_identity() {
    run_case(
        "fcp_three_levels",
        "export function got(): number { \
             const w1 = (cb: () => number): number => cb(); \
             const w2 = (cb: () => number): number => w1(cb); \
             const w3 = (cb: () => number): number => w2(cb); \
             return w3((): number => 1); \
         }",
        1.0,
    );
}

#[test]
fn both_wrappers_are_called_with_the_same_local_arrow() {
    run_case(
        "fcp_sum_of_two_wrappers",
        "export function got(): number { \
             const w1 = (cb: () => number): number => cb(); \
             const w2 = (cb: () => number): number => w1(cb); \
             const f = (): number => 1; \
             return w2(f) + w1(f); \
         }",
        2.0,
    );
}

#[test]
fn both_wrappers_are_called_as_a_method_argument() {
    run_case(
        "fcp_sum_of_two_wrappers_argument",
        &format!(
            "{COUNTER} export function got(): number {{ \
                 const w1 = (cb: () => number): number => cb(); \
                 const w2 = (cb: () => number): number => w1(cb); \
                 const f = (): number => 1; \
                 g.add(w2(f) + w1(f)); \
                 return g.n; \
             }}"
        ),
        2.0,
    );
}

#[test]
fn the_closure_parameter_is_used_as_a_callee() {
    run_case(
        "fcp_parameter_as_callee",
        "export function got(): number { \
             const w = (cb: () => number): number => cb() + 1; \
             return w((): number => 1); \
         }",
        2.0,
    );
}

#[test]
fn a_nested_closure_captures_the_outer_parameter() {
    run_case(
        "fcp_nested_capture_of_parameter",
        "export function got(): number { \
             const outer = (cb: () => number): number => { \
                 const inner = (): number => cb(); \
                 return inner(); \
             }; \
             return outer((): number => 1); \
         }",
        1.0,
    );
}

#[test]
fn a_nested_closure_uses_its_own_parameter_and_the_outer_one() {
    run_case(
        "fcp_nested_param_plus_capture",
        "export function got(): number { \
             const outer = (cb: () => number): number => { \
                 const inner = (d: number): number => cb() + d; \
                 return inner(0); \
             }; \
             return outer((): number => 1); \
         }",
        1.0,
    );
}

#[test]
fn a_two_parameter_wrapper_forwards_its_closure_parameter() {
    run_case(
        "fcp_two_parameter_wrapper",
        "export function got(): number { \
             const w0 = (a: number, cb: () => number): number => cb() + a; \
             const w = (cb: () => number): number => w0(1, cb); \
             return w((): number => 1); \
         }",
        2.0,
    );
}

#[test]
fn a_closure_parameter_shadowing_an_outer_binding_resolves_to_itself() {
    run_case(
        "fcp_shadowed_parameter",
        "export function got(): number { \
             const cb = (): number => 100; \
             const w = (cb: () => number): number => cb(); \
             return w((): number => 1); \
         }",
        1.0,
    );
}
