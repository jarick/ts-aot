use ts_aot_core::Diagnostic;
use ts_aot_driver::{CompileOptions, Driver, EmitStage};

mod common;

use common::normalize_rust;

#[test]
fn e2e_symbol_call_emits_symbol_new_runtime_call_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Symbol { return Symbol(); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "Symbol() must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for Symbol() e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_symbol_new"),
        "Rust source must contain __ts_aot_symbol_new call; got:\n{rust}"
    );
}

#[test]
fn e2e_symbol_for_emits_symbol_for_runtime_call_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r#"function f(): Symbol { return Symbol.for("shared"); }"#,
        &opts,
    );
    assert!(
        !out.has_errors(),
        "Symbol.for(\"shared\") must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for Symbol.for() e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_symbol_for"),
        "Rust source must contain __ts_aot_symbol_for call; got:\n{rust}"
    );
}

#[test]
fn e2e_symbol_call_with_undefined_description_collapses_to_no_arg_constructor() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Symbol { return Symbol(undefined); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "Symbol(undefined) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for Symbol(undefined) e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_symbol_new ()"),
        "Symbol(undefined) must collapse to the no-description constructor __ts_aot_symbol_new(); \
         our AOT model treats undefined as 'no description' rather than the string \"undefined\" \
         (full spec fidelity is a follow-up; see TEST262_PASS_PLAN.md 7.3 deferred items); got:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_symbol_new_desc"),
        "Symbol(undefined) must NOT call __ts_aot_symbol_new_desc (no JsString description \
         argument to forward); got:\n{rust}"
    );
    assert!(
        !rust.contains("JsString::from (\"undefined\")"),
        "Symbol(undefined) must NOT emit JsString::from(\"undefined\") — undefined is treated as \
         absent, not as the literal string \"undefined\"; got:\n{rust}"
    );
}

#[test]
fn e2e_symbol_call_with_null_description_emits_jsstring_null_literal() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(): Symbol { return Symbol(null); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "Symbol(null) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for Symbol(null) e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("JsString::from (\"null\")"),
        "Symbol(null) must lower to JsString::from(\"null\") per spec; got:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_symbol_new_desc"),
        "Symbol(null) must call __ts_aot_symbol_new_desc with a JsString arg; got:\n{rust}"
    );
}

#[test]
fn e2e_symbol_key_for_emits_symbol_key_for_runtime_call_in_rust_source() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"function f(s: Symbol): string | null { return Symbol.keyFor(s); }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "Symbol.keyFor(s) must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for Symbol.keyFor() e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_symbol_key_for"),
        "Rust source must contain __ts_aot_symbol_key_for call; got:\n{rust}"
    );
    assert!(
        rust.contains("Option<ts_aot_runtime::JsString>"),
        "Symbol.keyFor(s) must lower to Option<ts_aot_runtime::JsString> return type \
         (None for unregistered, Some(JsString) for registered) per ECMAScript spec; got:\n{rust}"
    );
}

#[test]
fn e2e_symbol_key_for_with_non_symbol_arg_emits_e0406() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(s: string): string { return Symbol.keyFor(s); }",
        &opts,
    );
    let key_for_e0406: Vec<&Diagnostic> = out
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E0406" && d.message.contains("Symbol.keyFor"))
        .collect();
    assert_eq!(
        key_for_e0406.len(),
        1,
        "Symbol.keyFor(string) must emit exactly one E0406 mentioning Symbol.keyFor; got {:?} (all diags: {:?})",
        key_for_e0406,
        out.diagnostics
    );
    assert!(
        key_for_e0406[0].message.contains("Symbol"),
        "Symbol.keyFor(string) E0406 must mention 'Symbol' as expected type; got: {}",
        key_for_e0406[0].message
    );
}
