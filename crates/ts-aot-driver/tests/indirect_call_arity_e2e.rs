use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
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
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn omitting_a_question_mark_optional_parameter_of_an_indirect_callee_is_allowed() {
    const SOURCE: &str = "export function run(): number { \
             const f = (a: number, b?: number): number => a + (b ?? 1); \
             return f(1); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `f(1)`: `a + (b ?? 1)` with `b` omitted is `1 + 1`.
    assert_eq!(indirect_arity_optional::run(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("b:Option<f64>"),
        "a `?`-optional parameter is an `Option` ABI parameter; got:\n{rust}"
    );
    assert!(
        norm.contains("Option::None"),
        "the omitted argument must be passed as `None`; got:\n{rust}"
    );
    check_generated_crate("indirect_arity_optional", &rust);
    let stdout = run_generated_crate("indirect_arity_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn omitting_a_required_undefined_typed_parameter_is_rejected_with_p0004() {
    let out = compile_module(
        "export function run(): number { \
             const f = (a: number, b: number | undefined): number => a + (b ?? 1); \
             return f(1); \
         }",
    );
    assert!(
        out.has_errors(),
        "`b: number | undefined` is a required parameter, so omitting it must be rejected; \
         got diagnostics {:?}",
        out.diagnostics
    );
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0004")
        .map_or_else(
            || panic!("expected P0004, got {:?}", out.diagnostics),
            |d| d.message.clone(),
        );
    assert!(
        message.contains("omitting a required argument"),
        "the diagnostic must explain the missing required argument; got: {message}"
    );
    assert!(
        message.contains("requires 2"),
        "the optionality flags say both parameters are required; got: {message}"
    );
}
