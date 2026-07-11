use std::io::Write;
use std::process::ExitCode;

use ts_aot_driver::cli::{CliError, parse_args};
use ts_aot_driver::{CompileOptions, Driver, EmitStage};

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();

    match parse_args(args) {
        Ok(parsed) => match run(&parsed.input, &parsed.opts) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                let _ = writeln!(std::io::stderr(), "{msg}");
                ExitCode::from(1)
            }
        },
        Err(err) => match err {
            CliError::Help | CliError::Version => {
                println!("{err}");
                ExitCode::SUCCESS
            }
            other => {
                let _ = writeln!(std::io::stderr(), "{other}");
                ExitCode::from(2)
            }
        },
    }
}

fn run(input: &str, opts: &CompileOptions) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| format!("read {input}: {e}"))?;
    let out = Driver::new().compile_source(input, &source, opts);

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

    let payload = match opts.emit {
        EmitStage::Rust => out.rust_source.as_deref(),
        EmitStage::Hir => out.hir_text.as_deref(),
        EmitStage::Mir => out.mir_text.as_deref(),
    };

    match payload {
        Some(text) => {
            print!("{text}");
            Ok(())
        }
        None => Err(format!(
            "no output produced for emit stage '{}'",
            opts.emit.as_str(),
        )),
    }
}

fn severity_label(s: ts_aot_core::Severity) -> &'static str {
    use ts_aot_core::Severity;
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    }
}
