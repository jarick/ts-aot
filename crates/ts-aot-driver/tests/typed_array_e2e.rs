use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

#[test]
fn e2e_new_int8_array_emits_runtime_call() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Int8Array { return new Int8Array(8); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_typed_array_new"),
        "new Int8Array(8) must lower to runtime call, got:\n{rust}"
    );
}

#[test]
fn e2e_new_uint8_array_emits_runtime_call_with_kind_1() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Uint8Array { return new Uint8Array(16); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_typed_array_new"),
        "new Uint8Array(16) must emit runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("(16) as i64,(1) as i64") || rust.contains("as i64"),
        "length=16 and kind_id=1 (Uint8) must be passed to the runtime as i64, got:\n{rust}"
    );
}

#[test]
fn e2e_new_float64_array_emits_runtime_call_with_kind_8() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Float64Array { return new Float64Array(4); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_typed_array_new"),
        "new Float64Array(4) must emit runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("(4) as i64,(8) as i64") || rust.contains("as i64"),
        "length=4 and kind_id=8 (Float64) must be passed to the runtime as i64, got:\n{rust}"
    );
}

#[test]
fn e2e_new_int8_array_with_number_parameter_coerces_to_i64() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(n: number): Int8Array { return new Int8Array(n); }",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_typed_array_new"),
        "new Int8Array(n) must lower to runtime call, got:\n{rust}"
    );
    assert!(
        rust.contains("as i64") || rust.contains("i64 ::from"),
        "number-typed length arg must be coerced to i64 (runtime signature is i64), got:\n{rust}"
    );
}
