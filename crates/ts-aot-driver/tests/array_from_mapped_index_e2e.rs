use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "function doubleIt(x: number, i: number): number { return x * 2 + i; } \
     export function mapped(): number { \
         const a: number[] = [1, 2, 3]; \
         const b = Array.from(a, doubleIt); \
         return b[0] + b[1] + b[2]; \
     }";

const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(array_from_mapped_index::mapped(&ctx), 15.0);
    println!("ok");
}
"#;

#[test]
fn array_from_mapped_coerces_the_runtime_index_to_the_mapper_parameter() {
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "Array.from with a two-parameter mapper must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("__ts_aot_idx)asf64"),
        "the runtime index is typed `i64` while the mapper's `number` parameter is `f64`, so the \
         index argument must be cast to the mapper's declared parameter type; got:\n{rust}"
    );
    check_generated_crate("array_from_mapped_index", &rust);
    let stdout = run_generated_crate("array_from_mapped_index", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
