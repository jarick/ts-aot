use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

#[test]
fn e2e_array_destructuring_emits_runtime_call_per_element() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(arr: i64[]): i64 {
            let [a, b, c] = arr;
            return a + b + c;
        }
        ",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert_eq!(
        rust.matches("__ts_aot_array_get_or_default::<").count(),
        3,
        "array destructuring must emit one typed runtime call per binding, got:\n{rust}"
    );
}

#[test]
fn e2e_array_destructuring_on_non_array_rhs_emits_diagnostic() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(): i64 {
            let [a, b] = 42;
            return a;
        }
        ",
        &opts,
    );
    let has_diagnostic = out
        .diagnostics
        .iter()
        .any(|d| d.message.contains("array type") || d.message.contains("destructuring"));
    assert!(
        has_diagnostic,
        "destructuring non-array rhs must surface a diagnostic, got: {:?}",
        out.diagnostics
    );
}
