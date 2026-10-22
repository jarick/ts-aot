use std::process::Command;

use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

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
    let dir = std::env::temp_dir().join(format!(
        "ts_aot_closures_e2e_{tag}_{}_{}",
        std::process::id(),
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

fn rustc_compile_lib(source: &str) {
    let dir = temp_dir("lib");
    let rs = dir.join("lib.rs");
    let rlib = dir.join("lib.rlib");
    std::fs::write(&rs, source).expect("write generated rust");
    let status = Command::new(rustc())
        .args(["--edition", "2021", "--crate-type", "lib"])
        .arg(&rs)
        .arg("-o")
        .arg(&rlib)
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
    rustc_compile_lib(&source);
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
    rustc_compile_lib(&source);
    assert_eq!(rustc_run(&source, "run"), "5");
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
    rustc_compile_lib(&source);
    assert_eq!(rustc_run(&source, "run"), "1");
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
        source.contains("__ScopeEnv_") && source.contains("Rc < __ScopeEnv_"),
        "shared captures must be grouped into one Rc<ScopeEnv>; got:\n{source}"
    );
    assert_eq!(
        source.matches("struct __ScopeEnv_").count(),
        1,
        "one scope env per scope (the nested closure reuses the outer env); got:\n{source}"
    );
    if skip_e2e() {
        return;
    }
    rustc_compile_lib(&source);
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
    rustc_compile_lib(&source);
    assert_eq!(rustc_run(&source, "run"), "14");
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
    rustc_compile_lib(&source);
    assert_eq!(rustc_run(&source, "run"), "5");
}
