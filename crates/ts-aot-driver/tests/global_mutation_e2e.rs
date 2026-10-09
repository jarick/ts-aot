use std::process::Command;

use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn compile_rust(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn rustc_check(source: &str) {
    let dir = std::env::temp_dir().join(format!(
        "ts_aot_global_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let rs = dir.join("lib.rs");
    let rlib = dir.join("lib.rlib");
    std::fs::write(&rs, source).expect("write generated rust");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let status = Command::new(rustc)
        .args(["--edition", "2021", "--crate-type", "lib"])
        .arg(&rs)
        .arg("-o")
        .arg(&rlib)
        .status()
        .expect("rustc must be available on PATH");
    assert!(
        status.success(),
        "generated Rust for a mutable global must compile with rustc; source:\n{source}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mutable_global_read_write_compiles_as_rust() {
    let out = compile_rust(
        "var counter: i64 = 0; \
         export function bump(): i64 { counter = counter + 1; return counter; }",
    );
    assert!(
        !out.has_errors(),
        "mutable global assignment must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    let source = out
        .rust_source
        .expect("emit-rust must populate rust_source for the global e2e check");
    assert!(
        source.contains("unsafe"),
        "reads/writes of a mutable `static mut` global must be wrapped in `unsafe`; got:\n{source}"
    );
    assert!(
        source.contains("static mut counter"),
        "a reassigned global must be emitted as a mutable static; got:\n{source}"
    );
    if std::env::var_os("TS_AOT_SKIP_E2E_CARGO").is_some() {
        eprintln!("skipping rustc check: TS_AOT_SKIP_E2E_CARGO is set");
        return;
    }
    rustc_check(&source);
}

#[test]
fn immutable_global_is_not_wrapped_in_unsafe() {
    let out = compile_rust("const answer: i64 = 42; export function get(): i64 { return answer; }");
    assert!(!out.has_errors(), "got {:?}", out.diagnostics);
    let source = out.rust_source.expect("rust_source");
    assert!(
        source.contains("static answer"),
        "an immutable global must be emitted as a plain `static`; got:\n{source}"
    );
    assert!(
        !source.contains("unsafe"),
        "reads of an immutable global must not require `unsafe`; got:\n{source}"
    );
}
