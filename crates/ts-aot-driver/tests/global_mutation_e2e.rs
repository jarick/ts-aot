use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn compile_rust(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

#[test]
fn mutable_global_read_write_is_wrapped_in_unsafe() {
    let out = compile_rust(
        "var counter: i64 = 0; \
         export function bump(): i64 { counter = counter + 1; return counter; }",
    );
    assert!(
        !out.has_errors(),
        "mutable global assignment must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the global check");
    assert!(
        source.contains("unsafe"),
        "reads/writes of a mutable `static mut` global must be wrapped in `unsafe`; got:\n{source}"
    );
    assert!(
        source.contains("static mut counter"),
        "a reassigned global must be emitted as a mutable static; got:\n{source}"
    );
}

#[test]
fn immutable_global_is_not_wrapped_in_unsafe() {
    let out = compile_rust("const answer: i64 = 42; export function get(): i64 { return answer; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("static answer"),
        "an immutable global must be emitted as a plain `static`; got:\n{source}"
    );
    assert!(
        !source.contains("unsafe"),
        "reads of an immutable global must not require `unsafe`; got:\n{source}"
    );
}
