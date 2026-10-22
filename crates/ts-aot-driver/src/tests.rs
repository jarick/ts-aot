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
    assert_ne!(rust, String::new());
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
    let opts = CompileOptions::with_emit(EmitStage::Hir);
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
    let opts = CompileOptions::with_emit(EmitStage::Hir);
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
fn parameter_default_value_is_reported_as_ignored() {
    let opts = CompileOptions::with_emit(EmitStage::Hir);
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f(x = 1): number { return x; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a parameter default must be a warning, not an error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0017"),
        "a parameter default must be reported as ignored (P0017), got {:?}",
        out.diagnostics
    );
}

#[test]
fn destructured_parameter_default_value_is_reported_as_ignored() {
    let opts = CompileOptions::with_emit(EmitStage::Hir);
    let out = Driver::new().compile_source(
        "test.ts",
        "export function f({a = 1}: { a: i32 }): i32 { return a; }",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "a destructured parameter default must be a warning, not an error; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "P0016"),
        "a destructured parameter default must be reported as ignored (P0016), got {:?}",
        out.diagnostics
    );
}

#[test]
fn emit_mir_produces_mir_dump() {
    let opts = CompileOptions::with_emit(EmitStage::Mir);
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
    let opts = CompileOptions::with_emit(EmitStage::Mir);
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
    let opts = CompileOptions::with_emit(EmitStage::Mir);
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
    let opts = CompileOptions::default();
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
fn e2e_array_length_borrows_plain_and_maps_optional_receiver() {
    for src in [
        "function f(a: i32[] | null): i64 | null { return a?.length; }",
        "function f(a: i32[] | null): i64 | null { return a?.len(); }",
    ] {
        let out = compile(src);
        assert!(
            !out.has_errors(),
            "`{src}` must compile; got {:?}",
            out.diagnostics
        );
        let rust = out.rust_source.expect("rust_source");
        assert!(
            rust.contains(". as_ref () . map (| o | __ts_aot_array_len (o))"),
            "optional array length must map the helper over `as_ref()` (Some-only) for `{src}`, got:\n{rust}"
        );
        assert!(
            rust.contains("Option < i64 >"),
            "optional array length result must stay optional for `{src}`, got:\n{rust}"
        );
    }

    let out = compile("function f(a: i32[]): i64 { return a.length; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("__ts_aot_array_len (& a)"),
        "plain array length must borrow its receiver (slice) for both property and method dispatch, got:\n{rust}"
    );
}

#[test]
fn global_name_collision_is_rejected_before_global_name_can_diverge() {
    let out = compile(
        "var counter: i64 = 0; \
         function counter(): i64 { return 0; } \
         export function bump(): i64 { counter = counter + 1; return counter; }",
    );
    assert!(
        out.has_errors(),
        "a global/function name collision must be rejected; got no errors: {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0503"),
        "collision must surface E0503 so a global's MIR decl name cannot diverge from its \
         reference name (which would break the mutable-global `unsafe` lookup); got: {combined}"
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
        &CompileOptions::default(),
    );
    assert!(!out.has_errors());
    let rust = out
        .artifact(EmitStage::Rust)
        .expect("artifact(rust) returns the rust_source field");
    assert_ne!(rust, String::new());
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let assign_x = main_body
        .find("x =")
        .expect("x assignment must appear in __tla_main body");
    let second = main_body
        .find("2 + 2")
        .expect("second inlined expr `2 + 2` must appear in __tla_main body");
    assert!(
        first < assign_x && assign_x < second,
        "source order must be preserved in __tla_main body: first ({first}) < assign_x ({assign_x}) < second ({second}); got __tla_main body:\n{main_body}"
    );
}

#[test]
fn tla_module_mode_top_level_expr_can_reference_runtime_let_binding() {
    let opts = CompileOptions::default();
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
    assert!(
        rust.contains("static mut x"),
        "runtime top-level `let x` must become a module-level mutable global; got rust:\n{rust}"
    );
    let assign_x = main_body
        .find("x =")
        .expect("x assignment must appear in __tla_main body");
    let x_ref = main_body
        .find("unsafe { x }")
        .expect("inlined expr `x;` must appear in __tla_main body (after the assignment)");
    assert!(
        assign_x < x_ref,
        "the assignment to `x` must be emitted before the inlined `x;` reference; got body:\n{main_body}"
    );
}

#[test]
fn tla_module_mode_rejects_user_declared_main_function() {
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
    let opts = CompileOptions::default();
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
fn tla_module_mode_assigns_runtime_let_bindings_to_module_globals() {
    let opts = CompileOptions::default();
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
    let assign_a = main_body
        .find("a =")
        .expect("a assignment must appear in __tla_main body");
    let assign_b = main_body
        .find("b =")
        .expect("b assignment must appear in __tla_main body");
    let assign_c = main_body
        .find("c =")
        .expect("c assignment must appear in __tla_main body");
    assert!(
        assign_a < assign_b && assign_b < assign_c,
        "source order must be preserved: a ({assign_a}) < b ({assign_b}) < c ({assign_c}); got body:\n{main_body}"
    );
    for g in ["static mut a", "static mut b", "static mut c"] {
        assert!(
            rust.contains(g),
            "runtime let binding must become a module-level mutable global `{g}`; got rust:\n{rust}"
        );
    }
}

#[test]
fn tla_module_mode_allows_cross_scope_reference_to_runtime_let_binding() {
    let opts = CompileOptions::default();
    let out = Driver::new().compile_source(
        "test.mts",
        "let x: i32 = 1 + 2;\nfunction use_x(): i32 { return x; }\n",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "cross-scope reference to a runtime top-level binding must compile: the binding becomes a module-level mutable global; got diagnostics: {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("static mut x"),
        "runtime top-level `let x` must become a module-level mutable global; got rust:\n{rust}"
    );
    assert!(
        rust.contains("fn use_x"),
        "function `use_x` must be emitted; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_allows_cross_scope_reference_even_when_let_follows_function() {
    let opts = CompileOptions::default();
    let out = Driver::new().compile_source(
        "test.mts",
        "function use_cfg(): i32 { return cfg; }\nlet cfg: i32 = 1 + 2;\n",
        &opts,
    );
    assert!(
        !out.has_errors(),
        "cross-scope reference to a runtime top-level binding must compile regardless of declaration order; got diagnostics: {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("module mode rust emit must populate rust_source");
    assert!(
        rust.contains("static mut cfg"),
        "runtime top-level `let cfg` must become a module-level mutable global; got rust:\n{rust}"
    );
}

#[test]
fn tla_module_mode_uninitialized_let_remains_module_wide_global() {
    let opts = CompileOptions::default();
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

#[test]
fn e2e_object_literal_lowers_to_anonymous_struct() {
    let out = compile("function f(): i64 { const o = { a: 1, b: true }; return o.a; }");
    assert!(
        !out.has_errors(),
        "an object literal must lower (no E0402); got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("struct __object_lit_0"),
        "an object literal must lower to an anonymous struct, got:\n{rust}"
    );
    assert!(
        rust.contains("__object_lit_0 { a : 1 , b : true }"),
        "the anonymous struct must be built with the literal's fields, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_field_access_is_not_p0013() {
    let out = compile("function f(): i64 { const o = { a: 7 }; return o.a; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !combined.contains("P0013"),
        "field access on a lowered object literal must resolve, not fall back to P0013; got:\n{combined}"
    );
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("return o . a ;"),
        "field access must resolve to the struct field, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_type_assertion_uses_target_struct() {
    let out = compile(
        "class Point { x: i64; y: i64; } function f(): Point { return { x: 1, y: 2 } as Point; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Point { x : 1 , y : 2 }"),
        "a type-asserted object literal must lower to the target struct, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "a type-asserted object literal must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_typed_let_uses_target_struct() {
    let out = compile(
        "class Point { x: i64; y: i64; } function f(): i64 { let p: Point = { x: 1, y: 2 }; return p.x; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Point { x : 1 , y : 2 }"),
        "a typed `let` object literal must lower to the target struct, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "a typed `let` object literal must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_assignment_uses_target_struct() {
    let out = compile(
        "class Point { x: i64; y: i64; } function f(): i64 { let p: Point = { x: 0, y: 0 }; p = { x: 1, y: 2 }; return p.x; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Point { x : 1 , y : 2 }"),
        "an assigned object literal must lower to the target struct, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "an assigned object literal must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_return_uses_function_return_type() {
    let out =
        compile("class Point { x: i64; y: i64; } function f(): Point { return { x: 1, y: 2 }; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Point { x : 1 , y : 2 }"),
        "a returned object literal must lower to the function's return struct, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "a returned object literal must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_call_argument_uses_param_type() {
    let out = compile(
        "class Point { x: i64; y: i64; } \
         function g(p: Point): i64 { return p.x; } \
         function f(): i64 { return g({ x: 1, y: 2 }); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("g (Point { x : 1 , y : 2 })"),
        "an object-literal argument must lower to the parameter's struct, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "an object-literal argument must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_nested_object_literal_field_type_is_inferred() {
    let out = compile("function f(): i64 { const o = { a: { x: 1 } }; return 0; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("pub a : __object_lit_"),
        "the outer struct field must reference the inferred nested struct type, got:\n{rust}"
    );
}

#[test]
fn e2e_typed_nested_object_literal_uses_field_type() {
    let out = compile(
        "class Inner { a: i64; } class Outer { inner: Inner; } \
         function f(): i64 { const o: Outer = { inner: { a: 1 } }; return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Inner { a : 1 }"),
        "the nested literal must lower to the declared field struct `Inner`, got:\n{rust}"
    );
    assert!(
        !rust.contains("__object_lit_"),
        "the nested literal must not synthesize an anonymous struct, got:\n{rust}"
    );
}

#[test]
fn e2e_object_literal_missing_target_field_stays_e0402() {
    let out = compile(
        "class Point { x: i64; y: i64; } \
         function f(): i64 { const p: Point = { x: 1 }; return p.x; }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0402"),
        "a typed object literal missing a struct field must not lower; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0402"),
        "a field-count mismatch must keep the object literal unsupported (E0402); got:\n{combined}"
    );
}

#[test]
fn e2e_object_literal_with_duplicate_keys_stays_e0402() {
    let out = compile("function f(): i64 { const o = { a: 1, a: 2 }; return 0; }");
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0402"),
        "duplicate object-literal keys must not lower to an invalid struct; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0402"),
        "duplicate keys must keep the object literal unsupported (E0402); got:\n{combined}"
    );
}

#[test]
fn e2e_object_literal_with_primitive_target_stays_e0402() {
    let out = compile("function f(): i64 { const n: i64 = { a: 1 }; return n; }");
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0402"),
        "an object literal targeting a primitive type must not lower; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0402"),
        "an object literal targeting a primitive type must keep E0402; got:\n{combined}"
    );
}

#[test]
fn e2e_object_literal_type_assertion_let_sets_local_type() {
    let out = compile(
        "class Point { x: i64; y: i64; } \
         function f(): i64 { const p = { x: 1, y: 2 } as Point; return p.x; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("Point { x : 1 , y : 2 }"),
        "the asserted object literal must lower to the target struct, got:\n{rust}"
    );
    assert!(
        rust.contains("return p . x ;"),
        "field access must resolve after a type-asserted let, got:\n{rust}"
    );
}

#[test]
fn e2e_accessor_ordinary_name_collision_stays_e0300() {
    let out = compile(
        "class C { get value(): i64 { return 1; } \
         __get_value(): i64 { return 2; } } \
         function f(): i64 { return 0; }",
    );
    assert!(
        out.has_errors(),
        "an ordinary method named like a lowered accessor name must be rejected; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0300"),
        "an accessor/ordinary name collision must report E0300; got:\n{combined}"
    );
}

#[test]
fn e2e_static_method_this_is_reported_as_unsupported() {
    let out = compile("class C { static f(): i64 { return this; } }");
    assert!(
        !out.has_errors(),
        "`this` in a static method lowers to `undefined`, which must not be a hard error; \
         got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("P0014"),
        "`this` in a static method must emit the P0014 unsupported-`this` diagnostic; \
         got:\n{combined}"
    );
}

#[test]
fn e2e_instance_method_this_still_resolves_to_self() {
    let out = compile("class C { x: i64 = 0; f(): i64 { return this.x; } }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("self . x"),
        "instance method `this.x` must still resolve to the receiver field, got:\n{rust}"
    );
}

#[test]
fn e2e_top_level_this_is_reported_as_unsupported() {
    let out = compile("function g(): i64 { return this; }");
    assert!(
        !out.has_errors(),
        "top-level `this` lowers to `undefined`, which must not be a hard error; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("P0014"),
        "`this` outside an instance method must emit the P0014 unsupported-`this` diagnostic; \
         got:\n{combined}"
    );
}

#[test]
fn e2e_unsupported_this_field_access_cascades_to_error() {
    let out = compile("function g(): i64 { return this.x; }");
    assert!(
        out.has_errors(),
        "member access on unsupported `this` must cascade to a hard error; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("P0014"),
        "the unsupported-`this` warning must be present before the cascade; got:\n{combined}"
    );
    assert!(
        combined.contains("P0011"),
        "member access on unsupported `this` must cascade to P0011; got:\n{combined}"
    );
}

#[test]
fn e2e_method_on_class_typed_global_resolves_via_declared_type() {
    let out = compile(
        "class C { x: i64 = 0; foo(): i64 { return this.x; } get y(): i64 { return this.x; } } \
         let obj: C; \
         function f(): i64 { return obj.foo() + obj.y; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("C__foo (obj)"),
        "a method call on a class-typed global must resolve via the declared global type, got:\n{rust}"
    );
    assert!(
        rust.contains("C__get_y (obj)"),
        "an accessor read on a class-typed global must resolve via the declared global type, got:\n{rust}"
    );
}

#[test]
fn e2e_static_accessor_dispatches_without_receiver() {
    let out = compile(
        "class C { static get y(): i64 { return 1; } static set y(v: i64) {} } \
         function f(): i64 { C.y = 5; return C.y; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("C__get_y ()"),
        "a static getter must dispatch without a receiver, got:\n{rust}"
    );
    assert!(
        rust.contains("C__set_y (__tmp"),
        "a static setter must dispatch without a receiver (value only), got:\n{rust}"
    );
}

#[test]
fn e2e_setter_assignment_checks_value_compatibility() {
    let out = compile(
        "class C { x: i64 = 0; set y(v: i64) { this.x = v; } } \
         function f(c: C): i64 { c.y = \"s\"; return 0; }",
    );
    assert!(
        out.has_errors(),
        "assigning a string to a numeric setter must be rejected; got {:?}",
        out.diagnostics
    );
    let combined: String = out
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("E0408"),
        "a string assigned to a numeric setter must report E0408; got:\n{combined}"
    );
}

#[test]
fn e2e_setter_assignment_evaluates_to_assigned_value() {
    let out = compile(
        "class C { x: i64 = 0; set y(v: i64) { this.x = v; } } \
         function f(c: C): i64 { let z: i64 = 0; z = (c.y = 5); return z; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("C__set_y (c ,"),
        "the setter must be invoked, got:\n{rust}"
    );
    assert!(
        !rust.contains("z = C__set_y"),
        "the assignment must evaluate to the assigned value, not the setter's result, got:\n{rust}"
    );
}

#[test]
fn e2e_accessor_compound_update_routes_through_getter_and_setter() {
    let out = compile(
        "class C { x: i64 = 0; get y(): i64 { return this.x; } set y(v: i64) { this.x = v; } } \
         function f(c: C): i64 { let a: i64 = (c.y += 1); let b: i64 = (c.y++); return a + b; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("C__get_y (c)"),
        "a compound accessor update must read through the getter, got:\n{rust}"
    );
    assert!(
        rust.contains("C__set_y (c ,"),
        "a compound accessor update must write through the setter, got:\n{rust}"
    );
}

#[test]
fn e2e_accessor_compound_update_keeps_global_receiver_without_temp() {
    let out = compile(
        "class C { x: i64 = 0; get y(): i64 { return this.x; } set y(v: i64) { this.x = v; } } \
         let g: C; \
         function f(): i64 { return g.y += 1; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("C__get_y (g)"),
        "a place-like (global) receiver must be forwarded without a by-value temp, got:\n{rust}"
    );
    assert!(
        rust.contains("C__set_y (g ,"),
        "a place-like (global) receiver must be forwarded to the setter as-is, got:\n{rust}"
    );
}

#[test]
fn e2e_accessor_compound_update_preserves_prefix_and_postfix_results() {
    fn setter_value_arg(rust: &str) -> String {
        let call = rust.rsplit("C__set_y (").next().expect("setter call");
        let args = call.split(')').next().expect("setter args");
        args.rsplit(", ").next().unwrap().trim().to_owned()
    }
    fn returned_local(rust: &str) -> String {
        rust.rsplit("return ")
            .next()
            .expect("return")
            .split(' ')
            .next()
            .unwrap()
            .to_owned()
    }

    let prefix = compile(
        "class C { x: i64 = 0; get y(): i64 { return this.x; } set y(v: i64) { this.x = v; } } \
         function f(c: C): i64 { return ++c.y; }",
    );
    assert!(!prefix.has_errors(), "got {:?}", prefix.diagnostics);
    let prefix_rust = prefix.rust_source.expect("rust_source");
    assert_eq!(
        returned_local(&prefix_rust),
        setter_value_arg(&prefix_rust),
        "a prefix update must evaluate to the new value passed to the setter, got:\n{prefix_rust}"
    );

    let postfix = compile(
        "class C { x: i64 = 0; get y(): i64 { return this.x; } set y(v: i64) { this.x = v; } } \
         function f(c: C): i64 { return c.y++; }",
    );
    assert!(!postfix.has_errors(), "got {:?}", postfix.diagnostics);
    let postfix_rust = postfix.rust_source.expect("rust_source");
    assert_ne!(
        returned_local(&postfix_rust),
        setter_value_arg(&postfix_rust),
        "a postfix update must evaluate to the previous value, not the new setter argument, got:\n{postfix_rust}"
    );
}

#[test]
fn e2e_accessor_compound_update_rejects_partial_accessors() {
    fn codes(out: &DriverOutput) -> String {
        out.diagnostics
            .iter()
            .map(|d| format!("{d:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    let read_only = compile(
        "class C { get y(): i64 { return 1; } } \
         function f(c: C): i64 { return c.y += 1; }",
    );
    assert!(read_only.has_errors(), "got {:?}", read_only.diagnostics);
    assert!(
        codes(&read_only).contains("E0411"),
        "a getter-only accessor must reject a compound update with E0411, got:\n{}",
        codes(&read_only)
    );

    let write_only = compile(
        "class C { set y(v: i64) {} } \
         function f(c: C): i64 { return c.y += 1; }",
    );
    assert!(write_only.has_errors(), "got {:?}", write_only.diagnostics);
    assert!(
        codes(&write_only).contains("E0412"),
        "a setter-only accessor must reject a compound update with E0412, got:\n{}",
        codes(&write_only)
    );
}

#[test]
fn e2e_destructuring_arrow_parameter_binds_property_value() {
    let out = compile(
        "class P { x: i64 = 0; } \
         function run(): void { const f = ({ x }: P) => x; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("let x : i64 = __tmp0 . x"),
        "a destructured arrow parameter must initialize the bound name from the argument's \
         property, got:\n{rust}"
    );
}

#[test]
fn e2e_destructuring_function_parameter_binds_property_value() {
    let out = compile(
        "class P { x: i64 = 0; } \
         function g({ x }: P): i64 { return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("let x : i64 = __tmp0 . x"),
        "a destructured function parameter must initialize the bound name from the argument's \
         property, got:\n{rust}"
    );
}

#[test]
fn e2e_destructuring_method_parameter_binds_property_value() {
    let out = compile(
        "class P { x: i64 = 0; } \
         class C { m({ x }: P): i64 { return x; } }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("let x : i64 = __tmp1 . x"),
        "a destructured method parameter must read the property from the value parameter \
         (after the synthetic `this`), got:\n{rust}"
    );
}
