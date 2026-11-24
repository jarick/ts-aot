use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

const SOURCE: &str = "function optIdx(x: number, i?: number): number { return x * 100 + (i ?? 7); } \
     export function mapped(): number { \
         const a: number[] = [1, 2, 3]; \
         const b = Array.from(a, optIdx); \
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
    assert_eq!(array_from_optional_index::mapped(&ctx), 603.0);
    println!("ok");
}
"#;

#[test]
fn array_from_mapped_wraps_the_runtime_index_for_an_optional_parameter() {
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
        "Array.from with a mapper whose index parameter is optional must compile through the full \
         pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("::core::option::Option::Some((__ts_aot_idx)asf64)"),
        "the mapper's optional index parameter is `Option<f64>` while the runtime index is `i64`, \
         so the index argument must be wrapped in `Some(..)` with the payload cast; got:\n{rust}"
    );
    check_generated_crate("array_from_optional_index", &rust);
    let stdout = run_generated_crate("array_from_optional_index", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
