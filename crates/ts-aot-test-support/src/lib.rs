use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

static GENERATED_CRATE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Cleanup {
    dir: PathBuf,
    target_dir: PathBuf,
    key: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        reap_target_artifacts(&self.target_dir, &self.key);
    }
}

fn remove_entry(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

fn is_metadata_hash(hash: &str) -> bool {
    hash.len() == 16
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn metadata_hash_suffixes(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| name.rsplit_once('-'))
        .map(|(_, hash)| hash)
        .filter(|hash| is_metadata_hash(hash))
        .map(|hash| format!("-{hash}"))
        .collect()
}

fn reap_target_artifacts(target_dir: &Path, key: &str) {
    let debug = target_dir.join("debug");
    let mut fingerprint_names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(debug.join(".fingerprint")) {
        let entries: Vec<_> = entries.flatten().collect();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(key) {
                continue;
            }
            fingerprint_names.push(name);
            remove_entry(&entry.path());
        }
    }
    let suffixes = metadata_hash_suffixes(&fingerprint_names);
    for dir in [
        debug.clone(),
        debug.join("deps"),
        debug.join("build"),
        debug.join("incremental"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let entries: Vec<_> = entries.flatten().collect();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.split('.').next().unwrap_or(&name);
            if name.starts_with(key) || suffixes.iter().any(|suffix| stem.ends_with(suffix)) {
                remove_entry(&entry.path());
            }
        }
    }
}

fn lib_name(package: &str) -> String {
    package.replace('-', "_")
}

fn generated_crate_manifest(unique: &str, package: &str, runtime: &str) -> String {
    let lib_name = lib_name(package);
    format!(
        "[package]\nname = \"{unique}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\nname = \"{lib_name}\"\npath = \"src/lib.rs\"\n\n\
         [workspace]\n\n[dependencies]\nts-aot-runtime = {{ path = \"{runtime}\" }}\n"
    )
}

fn prepare_generated_crate_dir(dir: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create generated crate src dir");
    src
}

fn materialize_generated_crate(package: &str, rust: &str) -> (PathBuf, String, Cleanup) {
    let pid = std::process::id();
    let counter = GENERATED_CRATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let unique = format!("{package}_{pid}_{counter}_");
    let dir = std::env::temp_dir().join(format!("ts_aot_generated_crate_{unique}"));
    let target_dir = generated_crate_target_dir();
    let cleanup = Cleanup {
        dir: dir.clone(),
        target_dir,
        key: unique.clone(),
    };
    let src = prepare_generated_crate_dir(&dir);

    let runtime = concat!(env!("CARGO_MANIFEST_DIR"), "/../ts-aot-runtime").replace('\\', "/");
    let manifest = generated_crate_manifest(&unique, package, &runtime);
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("write generated Cargo.toml");
    std::fs::write(src.join("lib.rs"), rust).expect("write generated lib.rs");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("copy workspace Cargo.lock into the generated crate");

    (dir, unique, cleanup)
}

fn generated_crate_target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("generated-crate-cargo-check"),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target"))
            .join("generated-crate-cargo-check"),
    }
}

fn run_cargo(dir: &Path, args: &[&str]) -> Output {
    let target_dir = generated_crate_target_dir();
    std::process::Command::new(env!("CARGO"))
        .args(args)
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo on the generated crate")
}

pub fn check_generated_crate(package: &str, rust: &str) {
    let (dir, _name, _cleanup) = materialize_generated_crate(package, rust);
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
    let (dir, name, _cleanup) = materialize_generated_crate(package, rust);
    std::fs::write(dir.join("src").join("main.rs"), main).expect("write generated crate main.rs");
    let output = run_cargo(&dir, &["run", "--quiet", "--bin", &name]);
    assert!(
        output.status.success(),
        "a generated crate depending only on ts-aot-runtime must build and run;\nstdout:\n{}stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[track_caller]
pub fn assert_exact_f64(actual: f64, expected: f64) {
    assert!(
        actual.to_bits() == expected.to_bits(),
        "expected {expected} ({:#018x}), got {actual} ({:#018x})",
        expected.to_bits(),
        actual.to_bits()
    );
}

#[cfg(test)]
mod tests {
    use super::{
        assert_exact_f64, generated_crate_manifest, metadata_hash_suffixes,
        prepare_generated_crate_dir,
    };

    fn panics(f: impl FnOnce() + std::panic::UnwindSafe) -> bool {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(f);
        std::panic::set_hook(previous);
        result.is_err()
    }

    #[test]
    fn assert_exact_f64_compares_bit_patterns() {
        assert_exact_f64(3.5, 3.5);
        assert_exact_f64(-0.0, -0.0);
        assert_exact_f64(f64::NAN, f64::NAN);
        let nan_payload = f64::from_bits(0x7ff8_0000_0000_0001);
        assert_exact_f64(nan_payload, nan_payload);
        assert!(panics(|| assert_exact_f64(-0.0, 0.0)));
        assert!(panics(|| assert_exact_f64(f64::NAN, 1.0)));
        assert!(panics(|| assert_exact_f64(1.0, f64::NAN)));
        assert!(panics(|| assert_exact_f64(
            f64::from_bits(0x7ff8_0000_0000_0002),
            nan_payload
        )));
    }

    #[test]
    fn only_cargo_metadata_hashes_become_reaping_suffixes() {
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-0123456789abcdef".to_owned()]),
            vec!["-0123456789abcdef".to_owned()]
        );
        assert_eq!(
            metadata_hash_suffixes(&[
                "probe_1_2_-0123456789abcdef".to_owned(),
                "probe_1_2_-fedcba9876543210".to_owned(),
            ]),
            vec![
                "-0123456789abcdef".to_owned(),
                "-fedcba9876543210".to_owned(),
            ]
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-1".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-0123456789ABCDEF".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-0123456789abcde".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-0123456789abcdef0".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_-0123456789abcdeg".to_owned()]),
            Vec::<String>::new()
        );
        assert_eq!(
            metadata_hash_suffixes(&["probe_1_2_0123456789abcdef".to_owned()]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_stale_generated_crate_dir_is_cleared_before_reuse() {
        let dir = std::env::temp_dir().join(format!(
            "ts_aot_generated_crate_stale_dir_probe_{}",
            std::process::id()
        ));
        let stale_src = dir.join("src");
        std::fs::create_dir_all(&stale_src).expect("create a stale generated crate src dir");
        std::fs::write(stale_src.join("main.rs"), "fn main() {}\n").expect("write stale main.rs");
        let src = prepare_generated_crate_dir(&dir);
        assert_eq!(src, stale_src);
        assert!(src.is_dir());
        assert!(!src.join("main.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_generated_lib_name_is_a_rust_identifier() {
        let manifest = generated_crate_manifest("probe-name_1_2_", "probe-name", "/runtime");
        assert!(manifest.contains("[package]\nname = \"probe-name_1_2_\""));
        assert!(manifest.contains("[lib]\nname = \"probe_name\"\npath = \"src/lib.rs\""));
        assert!(!manifest.contains("name = \"probe-name\""));
        let plain = generated_crate_manifest("probe_1_2_", "probe", "/runtime");
        assert!(plain.contains("[lib]\nname = \"probe\"\npath = \"src/lib.rs\""));
    }
}
