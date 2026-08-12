use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

use common::normalize_rust;

#[test]
fn e2e_new_array_buffer_emits_runtime_call_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): ArrayBuffer { return new ArrayBuffer(16); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "new ArrayBuffer(16) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for new ArrayBuffer e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_array_buffer_new (16)"),
        "Rust source must contain __ts_aot_array_buffer_new(16) call; got:\n{rust}"
    );
    assert!(
        rust.contains("ts_aot_runtime::ArrayBufferHandle"),
        "ArrayBuffer return type must be ts_aot_runtime::ArrayBufferHandle; got:\n{rust}"
    );
    assert!(
        rust.contains("let _ : ts_aot_runtime::ArrayBufferHandle"),
        "ArrayBuffer dest local must be typed ts_aot_runtime::ArrayBufferHandle (not unit `()`); got:\n{rust}"
    );
}

#[test]
fn e2e_array_buffer_slice_method_emits_runtime_call_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(b: ArrayBuffer): ArrayBuffer { return b.slice(2, 5); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "b.slice(2, 5) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for ArrayBuffer.slice() e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_array_buffer_slice"),
        "Rust source must contain __ts_aot_array_buffer_slice call; got:\n{rust}"
    );
}

#[test]
fn e2e_new_array_buffer_with_wrong_arity_emits_e0406() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): ArrayBuffer { return new ArrayBuffer(); }",
        &opts,
    );
    let arity_e0406: Vec<&ts_aot_core::Diagnostic> = out
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E0406" && d.message.contains("new ArrayBuffer(byteLength)"))
        .collect();
    assert_eq!(
        arity_e0406.len(),
        1,
        "new ArrayBuffer() must emit exactly one E0406 about arity; got {:?} (all diags: {:?})",
        arity_e0406,
        out.diagnostics
    );
}
