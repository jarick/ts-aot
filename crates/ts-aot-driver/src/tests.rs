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
        module: false,
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
        module: false,
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
        module: false,
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
        module: false,
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
        module: false,
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
        module: false,
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
        rust.contains("vec ! [ts_aot_runtime :: JsString :: from (\"hi \") , ts_aot_runtime :: JsString :: from (\"!\")]")
            || rust.contains("vec![ts_aot_runtime::JsString::from(\"hi \"), ts_aot_runtime::JsString::from(\"!\")]"),
        "tag's first arg must be a typed vec![JsString::from(\"hi \"), JsString::from(\"!\")] (no TemplateStringsArray wrapper), got rust:\n{rust}"
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
            module: false,
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

#[test]
fn tla_module_mode_collects_top_level_expression_statement_into_tla_main() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "1 + 2;\n", &opts);
    assert!(
        !out.has_errors(),
        "top-level ExpressionStatement in module mode must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit synthetic `fn __ts_aot_tla_main` for top-level stmts; got rust:\n{rust}"
    );
    assert!(
        !rust.contains("__tla_stmt_"),
        "inline mode must NOT generate `__tla_stmt_N` wrapper fns; got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn main"),
        "module mode must emit `fn main` entry that calls __tla_main; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_top_level_await_on_promise_typed_value_passes_through_pipeline() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "await 1;\n", &opts);
    assert!(
        !out.has_errors(),
        "top-level `await <int>` in module mode must compile (await on non-Promise lowers to a passthrough); got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit `fn __ts_aot_tla_main` for top-level stmt containing the await; got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn main"),
        "module mode must emit `fn main` entry; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_top_level_await_marks_main_async_and_drives_via_runtime_run() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "await 1;\n", &opts);
    assert!(
        !out.has_errors(),
        "top-level await must compile in module mode; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("async fn __ts_aot_tla_main"),
        "top-level await must mark the synthetic TLA main as `async fn` so the future is awaited; got rust:\n{rust}"
    );
    assert!(
        rust.contains("__ts_aot_runtime_run") && rust.contains("__ts_aot_tla_main ()"),
        "top-level await must drive the async TLA future through __ts_aot_runtime_run; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_without_await_keeps_sync_main_entry() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "1 + 2;\n", &opts);
    assert!(
        !out.has_errors(),
        "non-await top-level stmt must compile in module mode; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        !rust.contains("async fn __ts_aot_tla_main"),
        "non-await top-level stmt must keep the synthetic TLA main sync (no async); got rust:\n{rust}"
    );
    assert!(
        !rust.contains("__ts_aot_runtime_run"),
        "non-await module entry must call __ts_aot_tla_main() directly without runtime_run; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_multiple_expression_stmts_inline_in_tla_main_body() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "1 + 2;\n3 + 4;\n", &opts);
    assert!(
        !out.has_errors(),
        "multiple top-level expression statements in module mode must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit `fn __ts_aot_tla_main` body containing the top-level stmts inlined; got rust:\n{rust}"
    );
    let main_start = rust
        .find("fn __ts_aot_tla_main")
        .expect("tla main entry must be present");
    let main_body_start = main_start
        + rust[main_start..]
            .find('{')
            .expect("tla main body must have opening brace");
    let mut depth = 0usize;
    let mut main_body_end = main_body_start;
    for (i, c) in rust[main_body_start..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                main_body_end = main_body_start + i;
                break;
            }
        }
    }
    let main_body = &rust[main_body_start..main_body_end];
    let first = main_body
        .find("1 + 2")
        .expect("first expr `1 + 2` must be inlined into __tla_main body");
    let second = main_body
        .find("3 + 4")
        .expect("second expr `3 + 4` must be inlined into __tla_main body");
    assert!(
        first < second,
        "source order must be preserved inside __tla_main body when stmts are inlined: first ({first}) < second ({second}); got body:\n{main_body}"
    );
}

#[test]
fn tla_script_mode_does_not_emit_tla_main_or_main() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: false,
    };
    let out =
        Driver::new().compile_source("test.ts", "function f(): number { return 1; }\n", &opts);
    assert!(!out.has_errors(), "{:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("script mode rust emit must populate rust_source");
    assert!(
        !rust.contains("fn __ts_aot_tla_main"),
        "script mode must NOT emit synthetic `__tla_main`; got rust:\n{rust}"
    );
    assert!(
        !rust.contains("fn main"),
        "script mode must NOT emit `fn main`; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_with_only_declarations_still_emits_main_entry() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out =
        Driver::new().compile_source("test.mts", "function f(): number { return 1; }\n", &opts);
    assert!(
        !out.has_errors(),
        "module mode with only declarations (no top-level expression statements) must still compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        !rust.contains("fn __ts_aot_tla_main"),
        "module mode with only declarations must NOT emit synthetic `fn __ts_aot_tla_main` (would be empty body with `unimplemented!()`); got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn main () { }"),
        "module mode with only declarations must emit no-op `fn main() {{}}` entry (no TLA work to dispatch); got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_top_level_let_with_init_and_global_ref_compiles() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "let a: i32 = 42;\na;\n", &opts);
    assert!(
        !out.has_errors(),
        "top-level `let a: i32 = 42; a;` in module mode must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("pub static a : i32 = 42"),
        "frontend must walk let init and backend must emit `pub static a : i32 = 42`; got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit `fn __ts_aot_tla_main` containing inlined `a;`; got rust:\n{rust}"
    );
    assert!(
        !rust.contains("__tla_stmt_"),
        "inline mode must NOT generate `__tla_stmt_N` wrapper fns; got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn main"),
        "module mode must emit `fn main`; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_preserves_source_order_in_tla_main_body() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out =
        Driver::new().compile_source("test.mts", "1 + 1;\nlet x: i32 = 1 + 2;\n2 + 2;\n", &opts);
    assert!(
        !out.has_errors(),
        "interleaved stmt + let + stmt in module mode must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    let main_start = rust
        .find("fn __ts_aot_tla_main")
        .expect("tla main entry must be present");
    let main_body_start = main_start
        + rust[main_start..]
            .find('{')
            .expect("tla main body must have opening brace");
    let mut depth = 0usize;
    let mut main_body_end = main_body_start;
    for (i, c) in rust[main_body_start..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                main_body_end = main_body_start + i;
                break;
            }
        }
    }
    let main_body = &rust[main_body_start..main_body_end];
    let first = main_body
        .find("1 + 1")
        .expect("first inlined expr `1 + 1` must appear in __tla_main body");
    let let_x = main_body
        .find("let x")
        .expect("let x must appear in __tla_main body");
    let second = main_body
        .find("2 + 2")
        .expect("second inlined expr `2 + 2` must appear in __tla_main body");
    assert!(
        first < let_x && let_x < second,
        "source order must be preserved in __tla_main body: first ({first}) < let_x ({let_x}) < second ({second}); got __tla_main body:\n{main_body}"
    );
}

#[test]
fn tla_module_mode_top_level_expr_can_reference_runtime_let_binding() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "let x: i32 = 1 + 2;\nx;\n", &opts);
    assert!(
        !out.has_errors(),
        "top-level expr statement `x;` referencing runtime-initialized `let x` must compile in module mode; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit `fn __ts_aot_tla_main` containing both `let x = 1 + 2;` and inlined `x;`; got rust:\n{rust}"
    );
    let main_start = rust
        .find("fn __ts_aot_tla_main")
        .expect("tla main entry must be present");
    let main_body_start = main_start
        + rust[main_start..]
            .find('{')
            .expect("tla main body must have opening brace");
    let mut depth = 0usize;
    let mut main_body_end = main_body_start;
    for (i, c) in rust[main_body_start..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                main_body_end = main_body_start + i;
                break;
            }
        }
    }
    let main_body = &rust[main_body_start..main_body_end];
    let let_x = main_body
        .find("let x")
        .expect("let x must appear in __tla_main body");
    let x_ref = main_body
        .find("x ;")
        .or_else(|| main_body.find("x;"))
        .expect("inlined expr `x;` must appear in __tla_main body (after let x)");
    assert!(
        let_x < x_ref,
        "let x must be emitted before the inlined `x;` reference (shared scope: runtime binding must be in scope); got body:\n{main_body}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_main_function() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out =
        Driver::new().compile_source("test.mts", "function main(): number { return 1; }\n", &opts);
    assert!(
        out.has_errors(),
        "module mode must reject user-declared `function main()` (collides with synthesized entry); got no errors. diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0510"),
        "expected E0510 reserved-name diagnostic for `main`; got: {combined}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_tla_main_sentinel() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "function __ts_aot_tla_main(): number { return 1; }\n",
        &opts,
    );
    assert!(
        out.has_errors(),
        "module mode must reject user-declared `function __ts_aot_tla_main()` (collides with mangled entry); got no errors. diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0511"),
        "expected E0511 reserved-name diagnostic for `__ts_aot_tla_main`; got: {combined}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_main_global() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "const main: i32 = 42;\n", &opts);
    assert!(
        out.has_errors(),
        "module mode must reject user-declared `const main` (collides with synthesized entry); got no errors. diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0510"),
        "expected E0510 reserved-name diagnostic for global `main`; got: {combined}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_tla_main_global() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out =
        Driver::new().compile_source("test.mts", "const __ts_aot_tla_main: i32 = 42;\n", &opts);
    assert!(
        out.has_errors(),
        "module mode must reject user-declared `const __ts_aot_tla_main` (collides with mangled entry); got no errors. diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0511"),
        "expected E0511 reserved-name diagnostic for global `__ts_aot_tla_main`; got: {combined}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_generated_tla_main_name() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "function __tla_main_42(): number { return 1; }\n",
        &opts,
    );
    assert!(
        out.has_errors(),
        "module mode must reject user-declared `function __tla_main_42` (collides with generated TLA main name namespace); got no errors. diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0512"),
        "expected E0512 reserved-name diagnostic for `__tla_main_42` (generated TLA main namespace); got: {combined}"
    );
}

#[test]
fn tla_module_mode_allows_user_declared_name_with_tla_main_prefix_but_no_digits() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "function __tla_main_foo(): number { return 1; }\n",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "module mode must allow user-declared `function __tla_main_foo` (suffix is not all-digits, not in generated namespace); got errors: {:?}",
        out.diagnostics
    );
}

#[test]
fn tla_module_mode_assigns_distinct_local_ids_to_runtime_let_bindings() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "let a: i32 = 1 + 2;\nlet b: i32 = 3 + 4;\nlet c: i32 = 5 + 6;\n",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "multiple non-const top-level let bindings in module mode must compile; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("fn __ts_aot_tla_main"),
        "module mode must emit `fn __ts_aot_tla_main` with all three `let` bindings inlined; got rust:\n{rust}"
    );
    let main_start = rust
        .find("fn __ts_aot_tla_main")
        .expect("tla main entry must be present");
    let main_body_start = main_start
        + rust[main_start..]
            .find('{')
            .expect("tla main body must have opening brace");
    let mut depth = 0usize;
    let mut main_body_end = main_body_start;
    for (i, c) in rust[main_body_start..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                main_body_end = main_body_start + i;
                break;
            }
        }
    }
    let main_body = &rust[main_body_start..main_body_end];
    let let_a = main_body
        .find("let a")
        .expect("let a must be inlined into __tla_main body");
    let let_b = main_body
        .find("let b")
        .expect("let b must be inlined into __tla_main body");
    let let_c = main_body
        .find("let c")
        .expect("let c must be inlined into __tla_main body");
    assert!(
        let_a < let_b && let_b < let_c,
        "source order must be preserved: let a ({let_a}) < let b ({let_b}) < let c ({let_c}); got body:\n{main_body}"
    );
}

#[test]
fn tla_module_mode_rejects_cross_scope_reference_to_runtime_let_binding() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "let x: i32 = 1 + 2;\nfunction use_x(): i32 { return x; }\n",
        &opts,
    );
    assert!(
        out.has_errors(),
        "cross-scope reference to a TLA-only binding must produce a diagnostic (otherwise the backend would emit an undeclared Rust identifier); got diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0513"),
        "expected E0513 cross-scope diagnostic for `x`; got: {combined}"
    );
}

#[test]
fn tla_module_mode_cross_scope_reference_rejected_even_when_let_follows_function() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source(
        "test.mts",
        "function use_cfg(): i32 { return cfg; }\nlet cfg: i32 = 1 + 2;\n",
        &opts,
    );
    assert!(
        out.has_errors(),
        "cross-scope reference to a TLA-only binding must be rejected even when the function is declared before the let; got diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0513"),
        "expected E0513 cross-scope diagnostic for `cfg` (function-before-let order); got: {combined}"
    );
}

#[test]
fn tla_module_mode_uninitialized_let_remains_module_wide_global() {
    let opts = CompileOptions {
        emit: EmitStage::Rust,
        module: true,
    };
    let out = Driver::new().compile_source("test.mts", "let cfg: i32;\n", &opts);
    assert!(
        !out.has_errors(),
        "uninitialized top-level let must compile cleanly (no frontend or backend error); got diagnostics: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !combined.contains("E0513"),
        "uninitialized top-level let is a module-wide global; cross-scope references must NOT raise E0513; got diagnostics:\n{combined}"
    );
}
