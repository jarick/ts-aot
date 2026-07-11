use std::ffi::OsString;

use crate::{CompileOptions, EmitStage};

pub const PROGRAM_NAME: &str = "ts-aot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Help,
    Version,
    UnknownFlag(String),
    MissingInput,
    MultipleInputs(Vec<String>),
    InvalidEmit(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Help => f.write_str(HELP_TEXT),
            Self::Version => writeln!(f, "{PROGRAM_NAME} {VERSION}"),
            Self::UnknownFlag(flag) => {
                writeln!(f, "{PROGRAM_NAME}: unknown flag '{flag}'\n")?;
                f.write_str(HELP_TEXT)
            }
            Self::MissingInput => {
                writeln!(f, "{PROGRAM_NAME}: missing input file\n")?;
                f.write_str(HELP_TEXT)
            }
            Self::MultipleInputs(files) => {
                writeln!(f, "{PROGRAM_NAME}: multiple input files: {files:?}")?;
                f.write_str("expected exactly one positional argument")
            }
            Self::InvalidEmit(value) => {
                writeln!(f, "{PROGRAM_NAME}: invalid --emit value '{value}'")?;
                f.write_str("expected one of: rust, hir, mir")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    pub input: String,
    pub opts: CompileOptions,
}

pub fn parse_args<I: IntoIterator<Item = OsString>>(args: I) -> Result<ParsedArgs, CliError> {
    let mut input: Option<String> = None;
    let mut emit: Option<EmitStage> = None;

    let mut iter = args.into_iter();
    iter.next();

    for raw in iter {
        let arg = raw
            .into_string()
            .map_err(|os| CliError::UnknownFlag(format!("non-utf8 argument: {}", os.display())))?;

        match arg.as_str() {
            "-h" | "--help" => return Err(CliError::Help),
            "-V" | "--version" => return Err(CliError::Version),
            "--emit-rust" => emit = Some(EmitStage::Rust),
            "--emit-hir" => emit = Some(EmitStage::Hir),
            "--emit-mir" => emit = Some(EmitStage::Mir),
            s if s.starts_with("--emit=") => {
                let value = &s["--emit=".len()..];
                emit =
                    Some(parse_emit(value).ok_or_else(|| CliError::InvalidEmit(value.to_owned()))?);
            }
            s if s.starts_with("--") => return Err(CliError::UnknownFlag(s.to_owned())),
            _ => {
                if input.is_some() {
                    return Err(CliError::MultipleInputs(match input.take() {
                        Some(prev) => vec![prev, arg],
                        None => vec![arg],
                    }));
                }
                input = Some(arg);
            }
        }
    }

    let input = input.ok_or(CliError::MissingInput)?;
    Ok(ParsedArgs {
        input,
        opts: CompileOptions {
            emit: emit.unwrap_or_default(),
        },
    })
}

fn parse_emit(value: &str) -> Option<EmitStage> {
    match value {
        "rust" => Some(EmitStage::Rust),
        "hir" => Some(EmitStage::Hir),
        "mir" => Some(EmitStage::Mir),
        _ => None,
    }
}

const HELP_TEXT: &str = "Usage: ts-aot <FILE> [OPTIONS]

Compile a TypeScript source file to Rust.

Arguments:
  <FILE>                  Path to the .ts source file

Options:
      --emit=<STAGE>      Output stage: rust (default), hir, mir
      --emit-rust         Emit Rust source (default)
      --emit-hir          Emit HIR dump and stop
      --emit-mir          Emit MIR dump and stop
  -h, --help              Print this help message
  -V, --version           Print version

Diagnostics are written to stderr. On success, the requested artifact is
written to stdout. Exit code 0 on success, 1 on compilation errors, 2 on
argument errors.";

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{CliError, ParsedArgs, parse_args};

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn run(args: &[&str]) -> Result<ParsedArgs, CliError> {
        parse_args(args.iter().map(|s| os(s)))
    }

    #[test]
    fn missing_input_returns_missing_input_error() {
        let err = run(&[]).unwrap_err();
        assert!(matches!(err, CliError::MissingInput));
    }

    #[test]
    fn only_file_returns_rust_emit_by_default() {
        let parsed = run(&["ts-aot", "input.ts"]).unwrap();
        assert_eq!(parsed.input, "input.ts");
        assert_eq!(parsed.opts.emit.as_str(), "rust");
    }

    #[test]
    fn emit_hir_flag_sets_hir_stage() {
        let parsed = run(&["ts-aot", "input.ts", "--emit-hir"]).unwrap();
        assert_eq!(parsed.input, "input.ts");
        assert_eq!(parsed.opts.emit.as_str(), "hir");
    }

    #[test]
    fn emit_mir_flag_sets_mir_stage() {
        let parsed = run(&["ts-aot", "input.ts", "--emit-mir"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "mir");
    }

    #[test]
    fn emit_rust_flag_sets_rust_stage() {
        let parsed = run(&["ts-aot", "input.ts", "--emit-rust"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "rust");
    }

    #[test]
    fn emit_equals_form_accepts_rust_hir_mir() {
        for (flag, expected) in [
            ("--emit=rust", "rust"),
            ("--emit=hir", "hir"),
            ("--emit=mir", "mir"),
        ] {
            let parsed = run(&["ts-aot", "in.ts", flag]).unwrap();
            assert_eq!(parsed.opts.emit.as_str(), expected, "flag: {flag}");
        }
    }

    #[test]
    fn emit_equals_with_unknown_value_errors() {
        let err = run(&["ts-aot", "in.ts", "--emit=wasm"]).unwrap_err();
        assert!(matches!(err, CliError::InvalidEmit(v) if v == "wasm"));
    }

    #[test]
    fn unknown_long_flag_returns_unknown_flag() {
        let err = run(&["ts-aot", "in.ts", "--no-such-flag"]).unwrap_err();
        assert!(matches!(err, CliError::UnknownFlag(f) if f == "--no-such-flag"));
    }

    #[test]
    fn help_short_flag_returns_help() {
        let err = run(&["ts-aot", "-h"]).unwrap_err();
        assert!(matches!(err, CliError::Help));
    }

    #[test]
    fn help_long_flag_returns_help() {
        let err = run(&["ts-aot", "--help"]).unwrap_err();
        assert!(matches!(err, CliError::Help));
    }

    #[test]
    fn version_short_flag_returns_version() {
        let err = run(&["ts-aot", "-V"]).unwrap_err();
        assert!(matches!(err, CliError::Version));
    }

    #[test]
    fn version_long_flag_returns_version() {
        let err = run(&["ts-aot", "--version"]).unwrap_err();
        assert!(matches!(err, CliError::Version));
    }

    #[test]
    fn two_positional_inputs_returns_multiple_inputs() {
        let err = run(&["ts-aot", "a.ts", "b.ts"]).unwrap_err();
        assert!(matches!(err, CliError::MultipleInputs(_)));
    }

    #[test]
    fn file_with_directory_separator_accepted() {
        let parsed = run(&["ts-aot", "subdir/file.ts"]).unwrap();
        assert_eq!(parsed.input, "subdir/file.ts");
    }

    #[test]
    fn emit_flag_before_input_is_accepted() {
        let parsed = run(&["ts-aot", "--emit-mir", "in.ts"]).unwrap();
        assert_eq!(parsed.input, "in.ts");
        assert_eq!(parsed.opts.emit.as_str(), "mir");
    }

    #[test]
    fn help_text_mentions_emit_rust_hir_mir() {
        let help = format!("{}", CliError::Help);
        assert!(help.contains("--emit-rust"));
        assert!(help.contains("--emit-hir"));
        assert!(help.contains("--emit-mir"));
        assert!(help.contains("--version"));
    }

    #[test]
    fn version_output_contains_program_name_and_version() {
        let v = format!("{}", CliError::Version);
        assert!(v.contains(super::PROGRAM_NAME));
        assert!(v.contains(super::VERSION));
    }

    #[test]
    fn unknown_flag_display_mentions_flag_value() {
        let s = format!("{}", CliError::UnknownFlag("--bogus".to_owned()));
        assert!(s.contains("--bogus"));
    }

    #[test]
    fn invalid_emit_display_mentions_value() {
        let s = format!("{}", CliError::InvalidEmit("foo".to_owned()));
        assert!(s.contains("foo"));
        assert!(s.contains("rust"));
        assert!(s.contains("hir"));
        assert!(s.contains("mir"));
    }
}
