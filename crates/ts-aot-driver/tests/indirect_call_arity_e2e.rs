//! Arity of an *indirect* (computed) call.
//!
//! The right to omit a trailing argument is *declared* by the callee's
//! signature - a `?`-optional parameter or one with a default value (`?`), which
//! the frontend records positionally in `Type::Fn::optional`. It is never
//! inferred from the parameter's type admitting `undefined`: a parameter
//! declared `b: number | undefined` without `?` is required. The two tests below
//! pin both halves of that rule down through the whole pipeline, and the first
//! one also compiles and runs the generated crate to check the value.
//!
//! The driver helpers are local copies of `default_parameters_e2e.rs` (and of
//! the `ts_aot_test_support` crate it imports) so this file is self-contained.

use std::path::{Path, PathBuf};
use std::process::Output;

use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn materialize_generated_crate(package: &str, rust: &str) -> (PathBuf, Cleanup) {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("ts_aot_generated_crate_{package}_{pid}_{nanos}"));
    let cleanup = Cleanup(dir.clone());
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create generated crate src dir");

    let runtime = concat!(env!("CARGO_MANIFEST_DIR"), "/../ts-aot-runtime").replace('\\', "/");
    let manifest = format!(
        "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [workspace]\n\n[dependencies]\nts-aot-runtime = {{ path = \"{runtime}\" }}\n"
    );
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("write generated Cargo.toml");
    std::fs::write(src.join("lib.rs"), rust).expect("write generated lib.rs");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("copy workspace Cargo.lock into the generated crate");

    (dir, cleanup)
}

fn run_cargo(dir: &Path, args: &[&str]) -> Output {
    let target_dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/generated-crate-cargo-check"
    )
    .replace('\\', "/");
    std::process::Command::new(env!("CARGO"))
        .args(args)
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo on the generated crate")
}

fn check_generated_crate(package: &str, rust: &str) {
    let (dir, _cleanup) = materialize_generated_crate(package, rust);
    let output = run_cargo(&dir, &["check"]);
    assert!(
        output.status.success(),
        "a generated crate depending only on ts-aot-runtime must pass `cargo check`;\nstdout:\n{}stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[must_use]
fn run_generated_crate(package: &str, rust: &str, main: &str) -> String {
    let (dir, _cleanup) = materialize_generated_crate(package, rust);
    std::fs::write(dir.join("src").join("main.rs"), main).expect("write generated crate main.rs");
    let output = run_cargo(&dir, &["run", "--quiet", "--bin", package]);
    assert!(
        output.status.success(),
        "a generated crate depending only on ts-aot-runtime must build and run;\nstdout:\n{}stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A `?`-optional parameter of an indirect callee may be omitted: the flag in
/// `Type::Fn::optional` allows it, the missing argument becomes `None`, and the
/// callee's own `b ?? 1` supplies the value - so `f(1)` is `1 + 1 = 2`.
#[test]
fn omitting_a_question_mark_optional_parameter_of_an_indirect_callee_is_allowed() {
    const SOURCE: &str = "export function run(): number { \
             const f = (a: number, b?: number): number => a + (b ?? 1); \
             return f(1); \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    // `f(1)`: `a + (b ?? 1)` with `b` omitted is `1 + 1`.
    assert_eq!(indirect_arity_optional::run(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let norm: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("b:Option<f64>"),
        "a `?`-optional parameter is an `Option` ABI parameter; got:\n{rust}"
    );
    assert!(
        norm.contains("Option::None"),
        "the omitted argument must be passed as `None`; got:\n{rust}"
    );
    check_generated_crate("indirect_arity_optional", &rust);
    let stdout = run_generated_crate("indirect_arity_optional", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// The same call with the parameter declared `b: number | undefined` (no `?`) is
/// **required**: its type admits `undefined`, but the signature never declared
/// the right to omit the argument, so the missing argument is rejected. This is
/// the evidence that arity comes from `Type::Fn::optional`, not from the type.
#[test]
fn omitting_a_required_undefined_typed_parameter_is_rejected_with_p0004() {
    let out = compile_module(
        "export function run(): number { \
             const f = (a: number, b: number | undefined): number => a + (b ?? 1); \
             return f(1); \
         }",
    );
    assert!(
        out.has_errors(),
        "`b: number | undefined` is a required parameter, so omitting it must be rejected; \
         got diagnostics {:?}",
        out.diagnostics
    );
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "P0004")
        .map_or_else(
            || panic!("expected P0004, got {:?}", out.diagnostics),
            |d| d.message.clone(),
        );
    assert!(
        message.contains("omitting a required argument"),
        "the diagnostic must explain the missing required argument; got: {message}"
    );
    assert!(
        message.contains("requires 2"),
        "the optionality flags say both parameters are required; got: {message}"
    );
}
