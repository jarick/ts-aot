use crate::{CompileOptions, Driver, DriverOutput, EmitStage};

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source("test.ts", source, &CompileOptions::default())
}

#[test]
fn empty_source_compiles_without_errors() {
    let out = compile("");
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
}

#[test]
fn simple_function_default_emit_produces_rust_source() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function add(a: number, b: number): number { return a + b; }",
        &CompileOptions::default(),
    );
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("default emit must produce rust source");
    assert!(!rust.is_empty());
    assert!(out.hir_text.is_none());
    assert!(out.mir_text.is_none());
}

#[test]
fn rust_emit_uses_pipeline_typetable_not_fresh_empty() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function add(a: number, b: number): number { return a + b; }",
        &CompileOptions::default(),
    );
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust source must be populated");
    assert!(
        rust.contains("i32"),
        "types from frontend/passes must reach backend; got:\n{rust}"
    );
    assert!(
        !rust.contains("__ty0"),
        "fresh TypeTable bug regressed; got:\n{rust}"
    );
}

#[test]
fn emit_hir_produces_hir_dump() {
    let opts = CompileOptions {
        emit: EmitStage::Hir,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function id(x: number): number { return x; }",
        &opts,
    );
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let text = out.hir_text.expect("emit-hir must populate hir_text");
    assert!(text.contains("HirProgram"));
    assert!(out.rust_source.is_none());
    assert!(out.mir_text.is_none());
}

#[test]
fn emit_hir_skips_mir_conversion_for_hir_only_valid_input() {
    let opts = CompileOptions {
        emit: EmitStage::Hir,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(x: number): string { return typeof x; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "--emit-hir must stop before MIR; got {:?}",
        out.diagnostics
    );
    let text = out
        .hir_text
        .expect("emit-hir must populate hir_text even when MIR would fail");
    assert!(text.contains("HirProgram"));
    assert!(out.rust_source.is_none());
    assert!(out.mir_text.is_none());
}

#[test]
fn emit_mir_produces_mir_dump() {
    let opts = CompileOptions {
        emit: EmitStage::Mir,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "export function id(x: number): number { return x; }",
        &opts,
    );
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let text = out.mir_text.expect("emit-mir must populate mir_text");
    assert!(text.contains("MirProgram"));
    assert!(out.rust_source.is_none());
    assert!(out.hir_text.is_none());
}

#[test]
fn parse_error_surfaces_as_diagnostic_and_no_artifact() {
    let out = compile("const = 1;");
    assert!(out.has_errors());
    assert!(out.rust_source.is_none());
    assert!(out.hir_text.is_none());
    assert!(out.mir_text.is_none());
}

#[test]
fn driver_is_zero_sized_and_default_constructible() {
    let _ = Driver;
    let _ = Driver::new();
}

#[test]
fn emit_stage_default_is_rust() {
    assert_eq!(EmitStage::default(), EmitStage::Rust);
    assert_eq!(EmitStage::Rust.as_str(), "rust");
    assert_eq!(EmitStage::Hir.as_str(), "hir");
    assert_eq!(EmitStage::Mir.as_str(), "mir");
}

#[test]
fn compile_options_default_uses_rust_emit() {
    let opts = CompileOptions::default();
    assert_eq!(opts.emit, EmitStage::Rust);
}

#[test]
fn driver_output_default_is_empty_and_clean() {
    let out = DriverOutput::default();
    assert!(!out.has_errors());
    assert!(out.rust_source.is_none());
    assert!(out.hir_text.is_none());
    assert!(out.mir_text.is_none());
}
