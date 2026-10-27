use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

static TEMP_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

fn compile_rust(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn skip_e2e() -> bool {
    std::env::var_os("TS_AOT_SKIP_E2E_CARGO").is_some()
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let seq = TEMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ts_aot_closures_e2e_{tag}_{}_{}_{}",
        std::process::id(),
        seq,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn rustc() -> String {
    std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned())
}

fn rustc_compile_bin(source: &str) {
    let dir = temp_dir("bin_gate");
    let rs = dir.join("main.rs");
    let exe = dir.join("out");
    std::fs::write(&rs, source).expect("write generated rust");
    let status = Command::new(rustc())
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("rustc must be available on PATH");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        status.success(),
        "generated Rust for closures must compile with rustc; source:\n{source}"
    );
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn runtime_rlib() -> std::path::PathBuf {
    let root = workspace_root();
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
        .args(["build", "-p", "ts-aot-runtime", "--quiet"])
        .current_dir(&root)
        .status()
        .expect("cargo must be available on PATH");
    assert!(status.success(), "building ts-aot-runtime must succeed");
    let deps = root.join("target").join("debug").join("deps");
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&deps)
        .expect("deps dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("libts_aot_runtime-")
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("rlib"))
        })
        .collect();
    candidates.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    candidates
        .pop()
        .expect("a built libts_aot_runtime-*.rlib must exist")
}

fn rustc_run_with_runtime(source: &str, entry: &str) -> String {
    let dir = temp_dir("rt");
    let rs = dir.join("main.rs");
    let exe = dir.join("main.exe");
    let with_main = source.replace(
        "fn main () { }",
        &format!("fn main () {{ let __r = {entry} (); println! (\"{{}}\" , __r); }}"),
    );
    let source = format!("extern crate ts_aot_runtime;\nuse ts_aot_runtime::*;\n{with_main}");
    std::fs::write(&rs, &source).expect("write generated rust");
    let deps = workspace_root().join("target").join("debug").join("deps");
    let rlib = runtime_rlib();
    let status = Command::new(rustc())
        .args([
            "--edition",
            "2021",
            "-L",
            &format!("dependency={}", deps.display()),
        ])
        .arg("--extern")
        .arg(format!("ts_aot_runtime={}", rlib.display()))
        .arg(&rs)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("rustc must be available on PATH");
    assert!(
        status.success(),
        "generated Rust must build against the runtime; source:\n{source}"
    );
    let output = Command::new(&exe).output().expect("run generated binary");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn rustc_run(source: &str, entry: &str) -> String {
    let dir = temp_dir("bin");
    let rs = dir.join("main.rs");
    let exe = dir.join("main.exe");
    let with_main = source.replace(
        "fn main () { }",
        &format!("fn main () {{ let __r = {entry} (); println! (\"{{}}\" , __r); }}"),
    );
    std::fs::write(&rs, &with_main).expect("write generated rust");
    let status = Command::new(rustc())
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("rustc must be available on PATH");
    assert!(
        status.success(),
        "generated Rust for closures must build as a binary; source:\n{with_main}"
    );
    let output = Command::new(&exe).output().expect("run generated binary");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn non_capturing_closure_value_and_indirect_call_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { const g: () => i64 = (): i64 => 7; return g(); }",
    );
    assert!(
        !out.has_errors(),
        "closure sample must compile through the pipeline; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__TsFnValue0"),
        "a non-capturing closure stored in a variable must become a Fn-value enum; got:\n{source}"
    );
    assert!(
        source.contains("__ts_fn_call_0"),
        "an indirect call through a function value must use the static-dispatch helper; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "7");
}

#[test]
fn capturing_closure_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { const x: i64 = 5; const g: () => i64 = (): i64 => x; return g(); }",
    );
    assert!(
        !out.has_errors(),
        "capturing closure must compile through the pipeline; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ts_closure_"),
        "a capturing closure must be defunctionalized into a static function; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "5");
}

#[test]
fn refcell_shared_capture_self_assignment_hoists_value() {
    let out = compile_rust(
        "export function run(): i64 { let s: string = \"a\"; \
         const g: () => void = (): void => { s = s; }; \
         g(); return 0; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ts_shared_write"),
        "the assigned value must be evaluated into a local before the mutable borrow; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    assert_eq!(rustc_run_with_runtime(&source, "run"), "0");
}

#[test]
fn captured_function_parameter_compiles_and_runs() {
    let out = compile_rust(
        "export function make(x: i64): () => i64 { return (): i64 => x; } \
         export function run(): i64 { const f: () => i64 = make(9); return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Some (x)") || source.contains("Some(x)"),
        "a captured function parameter must be stored in the scope env at entry; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "9");
}

#[test]
fn mutable_capture_is_shared_with_enclosing_scope() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 0; const f: () => i64 = (): i64 => x; x = 1; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "shared mutable capture must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("std :: rc :: Rc") || source.contains("std::rc::Rc"),
        "a mutable captured local must live in shared storage (Rc<Cell<..>>); got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "1");
}

#[test]
fn uninitialized_shared_let_resets_env_to_none() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64; const f: () => i64 = (): i64 => x; x = 7; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "uninitialized shared capture must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("set (None)") || source.contains("set(None)"),
        "an uninitialized captured `let` must reset its env field to None, not Default::default(); got:\n{source}"
    );
    assert!(
        !source.contains("= Default :: default") && !source.contains("= Default::default"),
        "an uninitialized captured `let` must not initialize its env field with Default::default(); got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "7");
}

#[test]
fn shared_array_element_read_does_not_clone_container() {
    let out = compile_rust(
        "export function run(): i64 { const a: i64[] = [1, 2]; const f: () => i64 = (): i64 => a[0]; return f(); }",
    );
    assert!(
        !out.has_errors(),
        "shared array element read must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        !source.contains(".clone () [0]") && !source.contains(".clone()[0]"),
        "a shared array element read must index the borrow instead of cloning the whole container; got:\n{source}"
    );
    assert!(
        source.contains("[0] . clone ()") || source.contains("[0].clone()"),
        "expected the element to be cloned after indexing; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    assert_eq!(rustc_run_with_runtime(&source, "run"), "1");
}

#[test]
fn nested_closures_share_one_scope_env() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const outer: () => i64 = (): i64 => { \
           const inner: () => i64 = (): i64 => x; \
           x = x + 1; \
           return inner(); \
         }; \
         return outer(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__ScopeEnv") && source.contains("Rc < __ScopeEnv"),
        "shared captures must be grouped into one Rc<ScopeEnv>; got:\n{source}"
    );
    assert_eq!(
        source.matches("struct __ScopeEnv").count(),
        1,
        "one scope env per scope (the nested closure reuses the outer env); got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "2");
}

#[test]
fn sibling_closures_share_one_scope() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 0; \
         const a: () => i64 = (): i64 => x; \
         const b: () => i64 = (): i64 => x; \
         x = 7; return a() + b(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "14");
}

#[test]
fn function_named_self_used_as_value_compiles() {
    let out = compile_rust(
        "export function self(): i64 { return 1; } \
         export function run(): i64 { const f: () => i64 = self; return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Self_"),
        "a function named `self` must not produce a reserved-word enum variant; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "1");
}

#[test]
fn closure_returning_closure_over_local_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const mk: () => (() => i64) = (): (() => i64) => { const y: i64 = x + 1; return (): i64 => y; }; \
         const g: () => i64 = mk(); return g(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "2");
}

#[test]
fn for_of_item_captured_by_closure_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { const xs: i64[] = [1, 2, 3]; let total: i64 = 0; \
         for (const n of xs) { const g: () => i64 = (): i64 => n; total = total + g(); } \
         return total; }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    if skip_e2e() {
        return;
    }
    assert_eq!(rustc_run_with_runtime(&source, "run"), "6");
}

#[test]
fn shadowed_same_name_captured_by_distinct_closures() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const f: () => i64 = (): i64 => x; \
         { let x: i64 = 2; const g: () => i64 = (): i64 => x; x = 3; \
           return f() * 10 + g(); } }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "13");
}

#[test]
fn closure_parameter_shadowing_outer_capture_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { let x: i64 = 1; \
         const outer: () => i64 = (): i64 => x; \
         const inner: (x: i64) => i64 = (x: i64): i64 => x; \
         x = 2; return outer() * 10 + inner(5); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "25");
}

#[test]
fn fn_value_enum_has_manual_debug() {
    let out = compile_rust(
        "export function run(): i64 { const f: () => i64 = (): i64 => 7; return f(); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("impl :: std :: fmt :: Debug for __TsFnValue0")
            || source.contains("impl ::std::fmt::Debug for __TsFnValue0"),
        "the FnValue enum must provide a manual Debug impl; got:\n{source}"
    );
    assert!(
        source.contains("# [derive (Clone)]") || source.contains("#[derive(Clone)]"),
        "Clone derive must be kept; got:\n{source}"
    );
}

#[test]
fn array_of_closures_compiles_and_runs() {
    let out = compile_rust(
        "export function run(): i64 { \
         const fns: Array<() => i64> = [(): i64 => 1, (): i64 => 2]; \
         return fns[1](); }",
    );
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Vec < __TsFnValue0 >") || source.contains("Vec<__TsFnValue0>"),
        "an annotated array of closures must be typed as Vec<FnValue>; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    assert_eq!(rustc_run_with_runtime(&source, "run"), "2");
}

#[test]
fn annotated_int_array_literal_is_typed() {
    let out =
        compile_rust("export function run(): i64 { const xs: i64[] = [1, 2, 3]; return xs[1]; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("Vec < i64 >") || source.contains("Vec<i64>"),
        "an annotated array literal must carry its element type; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    assert_eq!(rustc_run_with_runtime(&source, "run"), "2");
}

#[test]
fn function_value_passed_as_argument_compiles_and_runs() {
    let out = compile_rust(
        "function apply(f: () => i64): i64 { return f(); } \
         export function run(): i64 { const g: () => i64 = (): i64 => 5; return apply(g); }",
    );
    assert!(
        !out.has_errors(),
        "passing a closure as an argument must compile; got {:?}",
        out.diagnostics
    );
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("__TsFnValue0"),
        "the parameter must be typed by the Fn-value enum; got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_bin(&source);
    assert_eq!(rustc_run(&source, "run"), "5");
}
