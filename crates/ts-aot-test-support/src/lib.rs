use std::path::{Path, PathBuf};
use std::process::Output;

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

pub fn check_generated_crate(package: &str, rust: &str) {
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
pub fn run_generated_crate(package: &str, rust: &str, main: &str) -> String {
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
