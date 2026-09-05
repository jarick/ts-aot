use ts_aot_core::DiagnosticCode;
use ts_aot_driver::{CompileOptions, Driver};

mod common;

use common::normalize_rust;

#[test]
fn e2e_weakmap_constructor_emits_weak_map_new_runtime_call() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out =
        Driver::new().compile_source("test.ts", "function f() { return new WeakMap(); }", &opts);
    assert!(
        !out.has_errors(),
        "new WeakMap() must compile through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for WeakMap e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("__ts_aot_weak_map_new"),
        "Rust source must contain __ts_aot_weak_map_new call; got:\n{rust}"
    );
}

#[test]
fn e2e_weakmap_type_resolves_to_weak_map_handle() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out =
        Driver::new().compile_source("test.ts", "function f() { return new WeakMap(); }", &opts);
    assert!(
        !out.has_errors(),
        "WeakMap type annotation must resolve; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for WeakMap type e2e check");
    let rust = normalize_rust(&rust);
    assert!(
        rust.contains("WeakMapHandle"),
        "Rust source must reference ts_aot_runtime::WeakMapHandle type; got:\n{rust}"
    );
}

#[test]
fn e2e_weakmap_get_emits_pattern_match_with_default() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(wm: WeakMap<object, i64>, k: object): i64 { return wm.get(k); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "WeakMap with bare `object` key must be rejected at type resolution; got {:?}",
        out.diagnostics
    );
    let e0403: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0403") && d.message.contains("object"))
        .collect();
    assert!(
        !e0403.is_empty(),
        "diagnostic bag must contain an E0403 entry rejecting the bare `object` key; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_weakmap_function_emits_persistent_liveness_prologue() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(wm: WeakMap<object, i64>, k: object): i64 { wm.set(k, 42); return wm.get(k); }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "WeakMap with bare `object` key must be rejected at type resolution; got {:?}",
        out.diagnostics
    );
    let e0403: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0403") && d.message.contains("object"))
        .collect();
    assert!(
        !e0403.is_empty(),
        "diagnostic bag must contain an E0403 entry rejecting the bare `object` key; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_weakmap_compiled_crate_passes_cargo_check() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(wm: WeakMap<object, i64>, k: object): i64 { wm.set(k, 42); if (wm.has(k)) { wm.delete(k); return 1; } wm.delete(k); return 0; }",
        &opts,
    );
    assert!(
        out.has_errors(),
        "WeakMap with bare `object` key must be rejected at type resolution; got {:?}",
        out.diagnostics
    );
    let e0403: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0403") && d.message.contains("object"))
        .collect();
    assert!(
        !e0403.is_empty(),
        "diagnostic bag must contain an E0403 entry rejecting the bare `object` key; got: {:?}",
        out.diagnostics
    );
}

#[test]
fn e2e_weakmap_bare_type_annotation_rejected_with_e0403() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(wm: WeakMap): WeakMap { return wm; }",
        &opts,
    );
    let e0403: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == DiagnosticCode::from("E0403") && d.message.contains("requires type arguments")
        })
        .collect();
    assert!(
        !e0403.is_empty(),
        "diagnostic bag must contain an E0403 entry for bare `WeakMap` (with 'requires type arguments' message); got: {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.as_deref().unwrap_or("");
    assert!(
        !rust.contains("WeakMapHandle"),
        "backend must not emit a WeakMapHandle type for the bare-`WeakMap` annotation, got rust: {rust}"
    );
}
