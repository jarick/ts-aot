use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

use common::normalize_rust;

#[test]
fn e2e_json_parse_i64_emits_generic_type_arg_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): i64 { return JSON.parse<i64>("42"); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.parse<i64> must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for JSON.parse e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_parse::<i64>"),
        "Rust source must contain __ts_aot_json_parse::<i64> generic type arg; got:\n{rust}"
    );
}

#[test]
fn e2e_json_parse_f64_emits_generic_type_arg_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): f64 { return JSON.parse<f64>("3.5"); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.parse<f64> must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for JSON.parse<f64> e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_parse::<f64>"),
        "Rust source must contain __ts_aot_json_parse::<f64> generic type arg; got:\n{rust}"
    );
}

#[test]
fn e2e_json_parse_string_emits_non_generic_helper() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): string { return JSON.parse<string>("\"hi\""); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.parse<string> must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for JSON.parse<string> e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_parse_string"),
        "Rust source must call non-generic __ts_aot_json_parse_string for T=string (surrogate-preserving path); got:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_json_parse::<"),
        "Rust source must NOT use generic __ts_aot_json_parse::<T> for T=string (that path would reject lone surrogates); got:\n{rust}"
    );
}

#[test]
fn e2e_json_parse_vec_i64_emits_generic_type_arg() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): Array<i64> { return JSON.parse<Array<i64>>("[1,2,3]"); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.parse<Array<i64>> must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for JSON.parse<Array<i64>> e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_parse::<Vec<i64>>"),
        "Rust source must contain __ts_aot_json_parse::<Vec<i64>> generic type arg; got:\n{rust}"
    );
}

#[test]
fn e2e_json_stringify_i64_emits_generic_type_arg() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(n: i64): string { return JSON.stringify(n); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.stringify(i64) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for JSON.stringify e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_stringify::<i64>"),
        "Rust source must contain __ts_aot_json_stringify::<i64> generic type arg; got:\n{rust}"
    );
}

#[test]
fn e2e_json_parse_without_type_arg_produces_error() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): i64 { return JSON.parse("42"); }"#,
        &opts,
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0406"),
        "JSON.parse without <T> must produce E0406 diagnostic, got: {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "JSON.parse without <T> must NOT produce rust_source; got:\n{}",
        out.rust_source.as_deref().unwrap_or_default()
    );
}

#[test]
fn e2e_json_parse_string_lone_surrogate_emits_non_generic_helper() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): string { return JSON.parse<string>("\"\\uD800\""); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "JSON.parse<string> with lone surrogate must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for lone-surrogate JSON.parse e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_json_parse_string"),
        "Rust source must call non-generic __ts_aot_json_parse_string for T=string with lone surrogate; got:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_json_parse::<"),
        "Rust source must NOT use generic __ts_aot_json_parse::<T> for T=string (that path would reject lone surrogates); got:\n{rust}"
    );
}
