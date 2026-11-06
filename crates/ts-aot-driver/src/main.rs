use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use ts_aot_driver::cli::{Cli, print_help_for};
use ts_aot_driver::{
    Driver, DriverOutput, EmitStage, is_rust_keyword, sanitize_rust_ident, severity_label,
};

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();

    match Cli::try_parse_from(&args) {
        Ok(Cli::Compile(c)) => {
            let parsed: ts_aot_driver::cli::ParsedArgs = c.into();
            match parsed.validate() {
                Ok(()) => run(&parsed),
                Err(msg) => {
                    let _ = writeln!(std::io::stderr(), "{msg}");
                    ExitCode::from(2)
                }
            }
        }
        Ok(Cli::Help { command }) => {
            print_help_for(command.as_deref());
            ExitCode::SUCCESS
        }
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                let _ = err.print();
                ExitCode::SUCCESS
            }
            _ => {
                let _ = err.print();
                ExitCode::from(2)
            }
        },
    }
}

fn run(parsed: &ts_aot_driver::cli::ParsedArgs) -> ExitCode {
    match compile_and_write(parsed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            ExitCode::from(1)
        }
    }
}

fn compile_and_write(parsed: &ts_aot_driver::cli::ParsedArgs) -> Result<(), String> {
    let out = Driver::new()
        .compile_file(&parsed.input, &parsed.opts)
        .map_err(|e| e.to_string())?;
    if parsed.opts.emit == EmitStage::Crate {
        let dir = parsed
            .output
            .as_deref()
            .ok_or_else(|| "--emit crate requires -o/--output <DIR>".to_owned())?;
        return write_crate(
            &out,
            dir,
            parsed.crate_name.as_deref(),
            parsed.runtime_path.as_deref(),
        );
    }
    write_artifact(
        &out,
        parsed.opts.emit,
        parsed.output.as_deref(),
        &mut std::io::stdout(),
    )
}

fn emit_diagnostics(out: &DriverOutput) -> Result<(), String> {
    for diag in &out.diagnostics {
        let _ = writeln!(
            std::io::stderr(),
            "{}: {}: {}",
            diag.code.as_str(),
            severity_label(diag.severity),
            diag.message,
        );
    }
    if out.has_errors() {
        return Err("compilation failed".to_owned());
    }
    Ok(())
}

fn write_crate(
    out: &DriverOutput,
    dir: &str,
    crate_name: Option<&str>,
    runtime_path: Option<&str>,
) -> Result<(), String> {
    emit_diagnostics(out)?;
    let lib_src = out
        .artifact(EmitStage::Crate)
        .ok_or_else(|| "no output produced for emit stage 'crate'".to_owned())?;

    let dir_path = Path::new(dir);
    let name = if let Some(name) = crate_name {
        if !is_valid_cargo_package_name(name) {
            return Err(format!(
                "invalid --crate-name '{name}': use ASCII letters, digits, '-' or '_', \
                 starting with a letter or '_', and not a Rust keyword or 'test'"
            ));
        }
        name.to_owned()
    } else {
        let derived = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_rust_ident)
            .ok_or_else(|| format!("cannot derive crate name from '{dir}'; pass --crate-name"))?;
        if !is_valid_cargo_package_name(&derived) {
            return Err(format!(
                "cannot derive a valid crate name from '{dir}' (got '{derived}'); \
                 pass --crate-name"
            ));
        }
        derived
    };
    let runtime = select_runtime_path(
        runtime_path,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ts-aot-runtime"),
    )?;

    let src_dir = dir_path.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("create {}: {e}", src_dir.display()))?;

    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nts-aot-runtime = {{ path = \"{}\" }}\n",
        toml_escape(&name),
        toml_escape(&runtime),
    );
    let manifest_path = dir_path.join("Cargo.toml");
    std::fs::write(&manifest_path, manifest)
        .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;
    let lib_path = src_dir.join("lib.rs");
    std::fs::write(&lib_path, lib_src).map_err(|e| format!("write {}: {e}", lib_path.display()))?;
    Ok(())
}

fn is_valid_cargo_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return false;
    }
    !is_rust_keyword(name) && name != "test"
}

fn select_runtime_path(runtime_path: Option<&str>, default: &str) -> Result<String, String> {
    let path = if let Some(path) = runtime_path {
        resolve_runtime_path(path)?
    } else {
        if !Path::new(default).is_dir() {
            return Err(format!(
                "default runtime path '{default}' does not exist; pass --runtime-path"
            ));
        }
        default.to_owned()
    };
    Ok(path.replace('\\', "/"))
}

fn resolve_runtime_path(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if path.is_absolute() || path.has_root() {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("runtime path '{}' is not valid UTF-8", path.display()))
    } else {
        let cwd = std::env::current_dir().map_err(|e| format!("current dir: {e}"))?;
        cwd.join(path)
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("runtime path '{}' is not valid UTF-8", path.display()))
    }
}

fn toml_escape(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn write_artifact(
    out: &DriverOutput,
    emit: EmitStage,
    output: Option<&str>,
    stdout: &mut dyn Write,
) -> Result<(), String> {
    emit_diagnostics(out)?;

    let payload = out
        .artifact(emit)
        .ok_or_else(|| format!("no output produced for emit stage '{}'", emit.as_str()))?;

    if let Some(path) = output {
        std::fs::write(path, payload.as_bytes()).map_err(|e| format!("write {path}: {e}"))?;
    } else if let Err(e) = stdout.write_all(payload.as_bytes())
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(format!("write stdout: {e}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ts_aot_driver::cli::ParsedArgs;
    use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

    use super::{is_valid_cargo_package_name, select_runtime_path, write_artifact, write_crate};

    fn unique_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let pid = std::process::id();
        std::env::temp_dir().join(format!("ts_aot_main_{pid}_{nanos}_{suffix}"))
    }

    fn run_write_artifact(
        input: &str,
        output: Option<&str>,
        emit: EmitStage,
        stdout: &mut dyn Write,
    ) -> Result<(), String> {
        let out = Driver::new()
            .compile_file(input, &CompileOptions { emit, module: true })
            .map_err(|e| e.to_string())?;
        write_artifact(&out, emit, output, stdout)
    }

    #[test]
    fn write_artifact_writes_payload_to_output_file_and_skips_stdout() {
        let input_path = unique_path("file_in.ts");
        let output_path = unique_path("file_out.rs");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");

        let mut stdout = Vec::new();
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            Some(output_path.to_str().expect("utf-8 path")),
            EmitStage::Rust,
            &mut stdout,
        );
        assert!(result.is_ok(), "{result:?}");

        let on_disk = std::fs::read_to_string(&output_path).expect("read output file");
        assert!(
            on_disk.contains("fn"),
            "rust source on disk should contain a function definition; got:\n{on_disk}",
        );
        assert!(
            stdout.is_empty(),
            "stdout must remain empty when --output is set; got {} bytes",
            stdout.len(),
        );

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn write_artifact_writes_payload_to_stdout_when_no_output() {
        let input_path = unique_path("stdout_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");

        let mut stdout = Vec::new();
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            None,
            EmitStage::Rust,
            &mut stdout,
        );
        assert!(result.is_ok(), "{result:?}");

        let emitted = String::from_utf8(stdout).expect("stdout bytes should be valid utf-8");
        assert!(
            emitted.contains("fn"),
            "rust source on stdout should contain a function definition; got:\n{emitted}",
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_artifact_returns_missing_artifact_error_for_empty_output() {
        let out = DriverOutput::default();
        let mut stdout = Vec::new();
        let result = write_artifact(&out, EmitStage::Rust, None, &mut stdout);
        let err = result.expect_err("default output has no artifact for any stage");
        assert!(
            err.contains("no output produced for emit stage 'rust'"),
            "expected missing-artifact message; got: {err}",
        );
        assert!(
            stdout.is_empty(),
            "no payload should be written when the artifact is missing",
        );
    }

    #[test]
    fn write_artifact_returns_write_error_for_unwritable_output_path() {
        let input_path = unique_path("bad_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");

        let bad_output = unique_path("missing_parent_dir").join("nested_output.rs");
        let mut stdout = Vec::new();
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            Some(bad_output.to_str().expect("utf-8 path")),
            EmitStage::Rust,
            &mut stdout,
        );
        let err = result.expect_err("writing into a non-existent parent directory must fail");
        assert!(
            err.starts_with("write "),
            "error should start with the 'write ' prefix; got: {err}",
        );
        assert!(
            err.contains("nested_output.rs"),
            "error should mention the target file name; got: {err}",
        );
        assert!(
            stdout.is_empty(),
            "no payload should be written to stdout when file output fails",
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_artifact_suppresses_broken_pipe_on_stdout() {
        struct BrokenPipeWriter;
        impl Write for BrokenPipeWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let input_path = unique_path("bp_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");

        let mut stdout = BrokenPipeWriter;
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            None,
            EmitStage::Rust,
            &mut stdout,
        );
        assert!(
            result.is_ok(),
            "broken pipe on stdout must be treated as success; got: {result:?}",
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_artifact_propagates_non_broken_pipe_stdout_errors() {
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("disk full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let input_path = unique_path("fail_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");

        let mut stdout = FailingWriter;
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            None,
            EmitStage::Rust,
            &mut stdout,
        );
        let err = result.expect_err("non-broken-pipe stdout error must propagate");
        assert!(
            err.contains("write stdout: "),
            "error should be tagged 'write stdout:'; got: {err}",
        );
        assert!(
            err.contains("disk full"),
            "error should include the underlying io::Error message; got: {err}",
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_artifact_reports_compilation_failed_for_parse_error() {
        let input_path = unique_path("parse_in.ts");
        std::fs::write(&input_path, "const = 1;").expect("write input source");

        let mut stdout = Vec::new();
        let result = run_write_artifact(
            input_path.to_str().expect("utf-8 path"),
            None,
            EmitStage::Rust,
            &mut stdout,
        );
        let err = result.expect_err("a parse error must fail compilation");
        assert_eq!(err, "compilation failed");
        assert!(
            stdout.is_empty(),
            "no payload should be written when compilation fails",
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_crate_writes_manifest_and_lib_without_main() {
        let input_path = unique_path("crate_in.ts");
        std::fs::write(
            &input_path,
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_out");
        let runtime = std::env::current_dir()
            .expect("cwd")
            .join("abs-runtime")
            .to_string_lossy()
            .replace('\\', "/");

        write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            Some("my_app"),
            Some(runtime.as_str()),
        )
        .expect("write_crate should succeed");

        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(
            manifest.contains("name = \"my_app\""),
            "manifest name; got:\n{manifest}"
        );
        assert!(
            manifest.contains(&format!("ts-aot-runtime = {{ path = \"{runtime}\" }}")),
            "manifest must reference the runtime path; got:\n{manifest}"
        );
        let lib = std::fs::read_to_string(dir.join("src/lib.rs")).expect("src/lib.rs");
        assert!(
            lib.contains("pub fn add"),
            "lib must expose the export; got:\n{lib}"
        );
        assert!(
            !lib.contains("fn main ()"),
            "lib must not contain `fn main`; got:\n{lib}"
        );

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_crate_is_not_written_on_compile_error() {
        let input_path = unique_path("crate_bad.ts");
        std::fs::write(&input_path, "const = 1;").expect("write input source");
        let dir = unique_path("crate_bad_out");

        let err = write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            None,
            None,
        )
        .expect_err("a parse error must fail");
        assert_eq!(err, "compilation failed");
        assert!(
            !dir.exists(),
            "crate dir must not be created on compile error"
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[cfg(windows)]
    #[test]
    fn write_crate_normalizes_backslashes_in_runtime_path() {
        let input_path = unique_path("crate_bs_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_bs_out");

        write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            Some("bs_app"),
            Some("C:\\repo\\crates\\ts-aot-runtime"),
        )
        .expect("write_crate should succeed");

        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(
            manifest.contains("path = \"C:/repo/crates/ts-aot-runtime\""),
            "backslashes in the runtime path must be normalized to forward slashes; got:\n{manifest}"
        );

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crate_options_without_crate_emit_are_rejected() {
        let parsed = ParsedArgs {
            input: "in.ts".to_owned(),
            output: None,
            crate_name: Some("x".to_owned()),
            runtime_path: None,
            opts: CompileOptions {
                emit: EmitStage::Rust,
                module: true,
            },
        };
        let err = parsed
            .validate()
            .expect_err("crate options require --emit crate");
        assert!(err.contains("--emit crate"), "got: {err}");
    }

    #[test]
    fn cargo_package_name_validation_matches_cargo() {
        for ok in ["my-app", "my_app", "_foo", "A1", "a-b-c", "main"] {
            assert!(is_valid_cargo_package_name(ok), "{ok} should be valid");
        }
        for bad in [
            "", "2fa", "type", "self", "Self", "test", "foo bar", "café", "-foo",
        ] {
            assert!(!is_valid_cargo_package_name(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn write_crate_rejects_underivable_crate_name() {
        let input_path = unique_path("crate_underiv_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_underiv").join("test");

        let err = write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            None,
            None,
        )
        .expect_err("a dir named 'test' must not derive a valid crate name");
        assert!(
            err.contains("cannot derive a valid crate name"),
            "got: {err}"
        );
        assert!(
            !dir.exists(),
            "crate dir must not be created on invalid name"
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn select_runtime_path_rejects_missing_default() {
        let err = select_runtime_path(None, "/definitely/not/here/ts-aot-runtime")
            .expect_err("a missing default must be rejected");
        assert!(err.contains("does not exist"), "got: {err}");
    }

    #[test]
    fn write_crate_preserves_valid_explicit_crate_name() {
        let input_path = unique_path("crate_name_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_name_out");

        write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            Some("my-app"),
            None,
        )
        .expect("write_crate should succeed");

        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(
            manifest.contains("name = \"my-app\""),
            "a valid Cargo package name must be preserved verbatim; got:\n{manifest}"
        );

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_crate_rejects_invalid_explicit_crate_name() {
        let input_path = unique_path("crate_badname_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_badname_out");

        let err = write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            Some("my app"),
            None,
        )
        .expect_err("an invalid crate name must be rejected");
        assert!(err.contains("invalid --crate-name"), "got: {err}");
        assert!(
            !dir.exists(),
            "crate dir must not be created on invalid name"
        );

        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn write_crate_resolves_relative_runtime_path_against_cwd() {
        let input_path = unique_path("crate_rel_in.ts");
        std::fs::write(
            &input_path,
            "export function id(x: number): number { return x; }",
        )
        .expect("write input source");
        let dir = unique_path("crate_rel_out");

        write_crate(
            &Driver::new()
                .compile_file(
                    input_path.to_str().unwrap(),
                    &CompileOptions {
                        emit: EmitStage::Crate,
                        module: true,
                    },
                )
                .unwrap(),
            dir.to_str().unwrap(),
            Some("rel_app"),
            Some("rel/runtime"),
        )
        .expect("write_crate should succeed");

        let cwd = std::env::current_dir().expect("cwd");
        let expected = cwd.join("rel/runtime").to_string_lossy().replace('\\', "/");
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(
            manifest.contains(&format!("path = \"{expected}\"")),
            "a relative runtime path must be resolved against the invocation dir; got:\n{manifest}"
        );

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parsed_args_construction_round_trips_input_and_output() {
        let parsed = ParsedArgs {
            input: "input.ts".to_owned(),
            output: Some("out.rs".to_owned()),
            crate_name: None,
            runtime_path: None,
            opts: CompileOptions::with_emit(EmitStage::Mir),
        };
        assert_eq!(parsed.input, "input.ts");
        assert_eq!(parsed.output.as_deref(), Some("out.rs"));
        assert_eq!(parsed.opts.emit, EmitStage::Mir);
    }
}
