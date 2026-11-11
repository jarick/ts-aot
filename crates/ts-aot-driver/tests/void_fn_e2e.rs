use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    void_fn_empty::run(&ctx);
    println!("ran");
}
"#;

#[test]
fn empty_void_function_does_not_panic_at_runtime() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function g(): void {}\nexport function run(): void { g(); }\n",
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "an empty void function must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the void-body check");
    assert!(
        !rust.contains("unimplemented"),
        "an empty void body must not emit `unimplemented!()`, which panics when called; got:\n{rust}"
    );
    let stdout = run_generated_crate("void_fn_empty", &rust, MAIN);
    assert!(
        stdout.contains("ran"),
        "calling an empty void function must return normally, got stdout:\n{stdout}"
    );
}
