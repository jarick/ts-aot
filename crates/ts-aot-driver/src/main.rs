use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use ts_aot_driver::cli::{Cli, print_help_for};
use ts_aot_driver::{Driver, DriverOutput, EmitStage, severity_label};

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();

    match Cli::try_parse_from(&args) {
        Ok(Cli::Compile(c)) => run(&c.into()),
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
    write_artifact(
        &out,
        parsed.opts.emit,
        parsed.output.as_deref(),
        &mut std::io::stdout(),
    )
}

fn write_artifact(
    out: &DriverOutput,
    emit: EmitStage,
    output: Option<&str>,
    stdout: &mut dyn Write,
) -> Result<(), String> {
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

    use super::write_artifact;

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
            .compile_file(input, &CompileOptions { emit })
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
    fn parsed_args_construction_round_trips_input_and_output() {
        let parsed = ParsedArgs {
            input: "input.ts".to_owned(),
            output: Some("out.rs".to_owned()),
            opts: CompileOptions {
                emit: EmitStage::Mir,
            },
        };
        assert_eq!(parsed.input, "input.ts");
        assert_eq!(parsed.output.as_deref(), Some("out.rs"));
        assert_eq!(parsed.opts.emit, EmitStage::Mir);
    }
}
