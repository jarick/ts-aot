use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::check_generated_crate;

#[test]
fn plain_number_into_optional_f32_narrows_the_payload_end_to_end() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function narrow(n: number): f32 | undefined { return n; }",
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "a `number` returned from an `f32 | undefined` function must compile through the full \
         pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for the optional-f32 check");
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Some((n)asf32)"),
        "a plain `f64` value reaching an optional `f32` target must cast the payload to `f32`; \
         got:\n{rust}"
    );
    check_generated_crate("float_width_optional_e2e", &rust);
}
