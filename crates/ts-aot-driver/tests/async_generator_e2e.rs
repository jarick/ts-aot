use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ts_aot_driver::{CompileOptions, Driver, EmitStage};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn skip_e2e() -> bool {
    std::env::var_os("TS_AOT_SKIP_E2E_CARGO").is_some()
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn rustc() -> String {
    std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned())
}

fn runtime_rlib() -> std::path::PathBuf {
    let root = workspace_root();
    let output = Command::new(cargo())
        .args(["build", "-p", "ts-aot-runtime", "--message-format=json"])
        .current_dir(&root)
        .output()
        .expect("cargo must be available on PATH");
    assert!(
        output.status.success(),
        "building ts-aot-runtime must succeed"
    );
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" || message["target"]["name"] != "ts_aot_runtime"
        {
            continue;
        }
        let Some(filenames) = message["filenames"].as_array() else {
            continue;
        };
        for name in filenames.iter().filter_map(serde_json::Value::as_str) {
            let path = std::path::Path::new(name);
            if name.contains("libts_aot_runtime")
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rlib"))
            {
                return path.to_path_buf();
            }
        }
    }
    panic!("cargo did not report a libts_aot_runtime rlib artifact");
}

fn runtime_deps_dir(rlib: &std::path::Path) -> std::path::PathBuf {
    let parent = rlib
        .parent()
        .expect("the runtime rlib must live in a directory");
    if parent.file_name().is_some_and(|name| name == "deps") {
        parent.to_path_buf()
    } else {
        parent.join("deps")
    }
}

fn compile_generated_rust_against_runtime(source: &str) {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ts_aot_async_gen_e2e_{}_{}_{}",
        std::process::id(),
        seq,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let rs = dir.join("main.rs");
    let exe = dir.join("main.exe");
    std::fs::write(&rs, format!("extern crate ts_aot_runtime;\n{source}"))
        .expect("write generated rust");
    let rlib = runtime_rlib();
    let deps = runtime_deps_dir(&rlib);
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
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        status.success(),
        "generated Rust must build against the runtime; source:\n{source}"
    );
}

fn run_i64_entry(source: &str) -> String {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ts_aot_async_gen_run_{}_{}_{}",
        std::process::id(),
        seq,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let rs = dir.join("main.rs");
    let exe = dir.join("main.exe");
    let with_main = source.replace("fn main () { }", "fn main () { println!(\"{}\", f()); }");
    std::fs::write(&rs, format!("extern crate ts_aot_runtime;\n{with_main}"))
        .expect("write generated rust");
    let rlib = runtime_rlib();
    let deps = runtime_deps_dir(&rlib);
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
        "generated Rust must build; source:\n{with_main}"
    );
    let output = Command::new(&exe).output().expect("run generated binary");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn compile_rust(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

#[test]
fn throwing_async_generator_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust("async function* g(): i64 { throw 1; }");
    assert!(
        rust.contains("__ts_aot_async_generator_throw"),
        "throwing async generator must lower to the runtime throw helper, got:\n{rust}"
    );
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn conditional_return_in_catch_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust(
        "function f(c: boolean): i64 { try { throw 1; } catch (e) { if (c) { return 1; } } return 2; }",
    );
    assert!(
        rust.contains("catch_unwind"),
        "a catch with a conditional return must still emit a catch_unwind loop, got:\n{rust}"
    );
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn continue_inside_switch_rechecks_loop_condition() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust(
        "function f(): i64 { let i: i64 = 0; while (i < 5) { switch (1) { case 1: i = i + 1; continue; } } return i; }",
    );
    assert_eq!(
        run_i64_entry(&rust),
        "5",
        "a `continue` inside a switch must reach the enclosing loop's condition re-check, got:\n{rust}"
    );
}

#[test]
fn async_explicit_return_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    compile_generated_rust_against_runtime(&compile_rust(
        "async function f(): Promise<i64> { return 5; }",
    ));
    compile_generated_rust_against_runtime(&compile_rust(
        "async function f(): Promise<void> { return; }",
    ));
    compile_generated_rust_against_runtime(&compile_rust(
        "async function f(): Promise<i64> { try { return 5; } catch (e) { return 6; } }",
    ));
}

#[test]
fn async_void_fallthrough_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust("async function f(): Promise<void> { const x: i64 = 1; }");
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn function_ending_in_try_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust("function f(): i64 { try { return 1; } catch (e) { return 2; } }");
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn throwing_closure_value_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust(
        "function f(): i64 { const g: () => i64 = (): i64 => { try { throw 1; } catch (e) { return 5; } return 0; }; return g(); }",
    );
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn closure_with_try_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust(
        "function f(): i64 { const g: () => i64 = (): i64 => { let i: i64 = 0; while (true) { try { i = i + 1; if (i == 3) { break; } } catch (e) { } } return i; }; return g(); }",
    );
    compile_generated_rust_against_runtime(&rust);
}

#[test]
fn try_catch_async_generator_generated_rust_links_runtime() {
    if skip_e2e() {
        return;
    }
    let rust = compile_rust(
        "async function* g(p: Promise<i64>): i64 { try { const v: i64 = await p; } catch (e) { } return 1; }",
    );
    assert!(
        rust.contains("catch_unwind") && rust.contains("__ts_aot_await_value_raw"),
        "a caught await inside an async generator must lower to a catch_unwind loop using the raw await helper, got:\n{rust}"
    );
    compile_generated_rust_against_runtime(&rust);
}
