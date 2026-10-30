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
