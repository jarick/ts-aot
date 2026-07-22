use std::io::Write;

use ts_aot_core::Severity;

use crate::{
    CompileOptions, DiagnosticBag, Driver, DriverError, DriverOutput, EmitStage, severity_label,
};

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
fn e2e_ternary_throwing_call_propagates_throws_to_mir_dump() {
    let opts = CompileOptions {
        emit: EmitStage::Mir,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function f(c: i64): never { throw c > 0 ? throwingFn() : 0; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "frontend+passes+convert must accept `throw c > 0 ? throwingFn() : 0`; got {:?}",
        out.diagnostics
    );
    let text = out
        .mir_text
        .expect("emit-mir must populate mir_text for e2e ternary throws check");
    let throws_id = parse_throws_id(&text, "f").unwrap_or_else(|| {
        panic!(
            "MIR dump must contain `fn #0 f(c: ...) -> ... throws N` for function f; got:\n{text}"
        )
    });
    assert!(
        throws_id > 0,
        "MIR dump must show `throws N` with N > 0 — `throw c > 0 ? throwingFn() : 0` must propagate the Ternary's `ty` through throw_expr_type, not the TypeId::from_raw(0) sentinel; got:\n{text}"
    );
    assert!(
        text.contains("can_throw: true"),
        "MIR dump must show `can_throw: true` in FunctionEffects — ternary with throwing call must keep can_throw set through the full pipeline; got:\n{text}"
    );
}

fn parse_throws_id(mir_text: &str, fn_name: &str) -> Option<u32> {
    let sig = format!("fn #0 {fn_name}(");
    let start = mir_text.find(&sig)?;
    let after = &mir_text[start + sig.len()..];
    let throws_idx = after.find(" throws ")?;
    let after_throws = &after[throws_idx + " throws ".len()..];
    let id_end = after_throws
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_throws.len());
    after_throws[..id_end].parse().ok()
}

#[test]
fn e2e_tagged_template_emits_indirect_call_with_string_slice_via_mir() {
    let opts = CompileOptions {
        emit: EmitStage::Mir,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function tag(strings: string[], sub: i64): i64 { return 0; } function f(): i64 { return tag`hi ${42}!`; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "tagged template must lower through full pipeline; got {:?}",
        out.diagnostics
    );
    let text = out
        .mir_text
        .expect("emit-mir must populate mir_text for e2e tagged template check");
    let f_idx = text.find("fn #1 f(").expect("f must be in MIR dump");
    let f_block_start = text[f_idx..].find("block: {").expect("f must have a block");
    let f_body_start = f_idx + f_block_start + "block: {".len();
    let f_body_end_rel = text[f_body_start..].find("      }").expect("f block end");
    let f_body = &text[f_body_start..f_body_start + f_body_end_rel];
    assert!(
        f_body.contains("tplstrings(cooked=[\"hi \", \"!\"])"),
        "tagged template must emit tplstrings with cooked parts; got f body:\n{f_body}\n\nfull dump:\n{text}"
    );
    assert!(
        f_body.contains("int(42") || f_body.contains("42"),
        "substitution `42` must appear as a direct arg to indirect_call; got f body:\n{f_body}"
    );
    assert!(
        f_body.contains("indirect_call(tag)") || f_body.contains("indirect_call(tag)("),
        "tag must be invoked as an indirect call (callee=tag); got f body:\n{f_body}"
    );
}

#[test]
fn e2e_tagged_template_string_array_arg_emits_typed_vec_string() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "function tag(strings: string[], sub: i64): i64 { return strings.len() as i64; } function f(): i64 { return tag`hi ${42}!`; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "tagged template must lower through full pipeline; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source for e2e tagged template Rust check");
    assert!(
        rust.contains("vec ! [String :: from (\"hi \") , String :: from (\"!\")]")
            || rust.contains("vec![String::from(\"hi \"), String::from(\"!\")]"),
        "tag's first arg must be a typed vec![String::from(\"hi \"), String::from(\"!\")] (no TemplateStringsArray wrapper), got rust:\n{rust}"
    );
    let has_amp_slice_str = rust.contains("& [\"hi\"")
        || rust.contains("&[\"hi\"")
        || rust.contains("& [\"!\"]")
        || rust.contains("&[\"!\"");
    assert!(
        !has_amp_slice_str,
        "tag's first arg must NOT be a &[&str] slice, got rust:\n{rust}"
    );
    let has_dyn_vec =
        rust.contains("__ts_aot_dyn_vec_new") || rust.contains("__ts_aot_dyn_vec_append");
    assert!(
        !has_dyn_vec,
        "strict AOT must NOT emit DynVec ops — subs are passed as direct typed args, got rust:\n{rust}"
    );
    let has_template_strings_array = rust.contains("TemplateStringsArray");
    assert!(
        !has_template_strings_array,
        "strict AOT must NOT reference TemplateStringsArray type — emit is typed Vec<String>, got rust:\n{rust}"
    );
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

#[test]
fn driver_output_artifact_returns_requested_field() {
    let out = Driver::new().compile_source(
        "test.ts",
        "export function id(x: number): number { return x; }",
        &CompileOptions {
            emit: EmitStage::Rust,
        },
    );
    assert!(!out.has_errors());
    let rust = out
        .artifact(EmitStage::Rust)
        .expect("artifact(rust) returns the rust_source field");
    assert!(!rust.is_empty());
    assert!(out.artifact(EmitStage::Hir).is_none());
    assert!(out.artifact(EmitStage::Mir).is_none());
}

#[test]
fn driver_output_artifact_returns_none_for_missing_stage() {
    let out = DriverOutput::default();
    assert!(out.artifact(EmitStage::Rust).is_none());
    assert!(out.artifact(EmitStage::Hir).is_none());
    assert!(out.artifact(EmitStage::Mir).is_none());
}

#[test]
fn severity_label_maps_known_variants() {
    assert_eq!(severity_label(Severity::Error), "error");
    assert_eq!(severity_label(Severity::Warning), "warning");
    assert_eq!(severity_label(Severity::Note), "note");
}

#[test]
fn core_types_are_reexported_for_embedders() {
    let _bag: DiagnosticBag = DiagnosticBag::default();
    let label = severity_label(Severity::Error);
    assert_eq!(label, "error");
}

#[test]
fn compile_file_reads_source_from_disk() {
    let dir = std::env::temp_dir();
    let path = dir.join("ts_aot_driver_compile_file_smoke.ts");
    let mut f = std::fs::File::create(&path).expect("create temp file");
    write!(
        f,
        "export function add(a: number, b: number): number {{ return a + b; }}"
    )
    .expect("write temp file");

    let out = Driver::new()
        .compile_file(&path, &CompileOptions::default())
        .expect("compile_file reads the file and compiles");
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("rust_source must be populated after compile_file");
    assert!(rust.contains("i32"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn compile_file_returns_io_error_for_missing_path() {
    let path = std::env::temp_dir().join("ts_aot_driver_does_not_exist_xyz_12345.ts");
    let err = Driver::new()
        .compile_file(&path, &CompileOptions::default())
        .expect_err("missing file must produce DriverError::Io");
    let display = format!("{err}");
    assert!(
        display.contains("read "),
        "io error should be reported via Display; got: {display}"
    );
    let src = std::error::Error::source(&err)
        .expect("DriverError::Io exposes the source io::Error")
        .downcast_ref::<std::io::Error>()
        .expect("source must downcast to std::io::Error");
    assert_eq!(src.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn driver_error_io_display_includes_path() {
    let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
    let err = DriverError::Io {
        path: "/some/file.ts".to_owned(),
        source: inner,
    };
    let s = format!("{err}");
    assert!(s.contains("/some/file.ts"));
    assert!(s.contains("nope"));
}
