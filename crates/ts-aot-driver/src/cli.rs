use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, ValueEnum};

use crate::{CompileOptions, EmitStage};

pub const PROGRAM_NAME: &str = "ts-aot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmitStageArg {
    Rust,
    Hir,
    Mir,
}

impl From<EmitStageArg> for EmitStage {
    fn from(arg: EmitStageArg) -> Self {
        match arg {
            EmitStageArg::Rust => EmitStage::Rust,
            EmitStageArg::Hir => EmitStage::Hir,
            EmitStageArg::Mir => EmitStage::Mir,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = PROGRAM_NAME,
    version = VERSION,
    about = "Compile a TypeScript source file to Rust",
    long_about = None,
    disable_help_subcommand = true,
    disable_version_flag = false,
)]
pub enum Cli {
    Compile(CompileArgs),
    Help {
        #[arg(value_name = "COMMAND")]
        command: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct CompileArgs {
    pub file: PathBuf,

    #[arg(short = 'e', long, value_enum, default_value_t = EmitStageArg::Rust)]
    pub emit: EmitStageArg,

    #[arg(short = 'o', long = "output", value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    pub input: String,
    pub output: Option<String>,
    pub opts: CompileOptions,
}

impl From<CompileArgs> for ParsedArgs {
    fn from(args: CompileArgs) -> Self {
        Self {
            input: args.file.display().to_string(),
            output: args.output.map(|p| p.display().to_string()),
            opts: CompileOptions {
                emit: args.emit.into(),
            },
        }
    }
}

pub fn parse_args<I: IntoIterator<Item = OsString>>(args: I) -> Result<ParsedArgs, clap::Error> {
    let cli = Cli::try_parse_from(args)?;
    match cli {
        Cli::Compile(c) => Ok(c.into()),
        Cli::Help { .. } => Err(clap::Error::raw(clap::error::ErrorKind::DisplayHelp, "")),
    }
}

pub fn print_help_for(command: Option<&str>) {
    let mut cmd = Cli::command();
    if let Some(name) = command
        && let Some(sub) = cmd.find_subcommand_mut(name)
    {
        let _ = sub.print_long_help();
    } else {
        let _ = cmd.print_long_help();
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::error::ErrorKind;

    use super::{EmitStageArg, ParsedArgs, parse_args};

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn run(args: &[&str]) -> Result<ParsedArgs, clap::Error> {
        parse_args(args.iter().map(|s| os(s)))
    }

    #[test]
    fn compile_with_file_returns_rust_emit_by_default() {
        let parsed = run(&["ts-aot", "compile", "input.ts"]).unwrap();
        assert_eq!(parsed.input, "input.ts");
        assert_eq!(parsed.opts.emit.as_str(), "rust");
        assert!(parsed.output.is_none());
    }

    #[test]
    fn compile_with_emit_hir_flag() {
        let parsed = run(&["ts-aot", "compile", "input.ts", "--emit", "hir"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "hir");
    }

    #[test]
    fn compile_with_emit_mir_flag() {
        let parsed = run(&["ts-aot", "compile", "input.ts", "--emit", "mir"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "mir");
    }

    #[test]
    fn compile_with_emit_equals_form() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "--emit=rust"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "rust");
    }

    #[test]
    fn compile_with_emit_short_form() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "-e", "mir"]).unwrap();
        assert_eq!(parsed.opts.emit.as_str(), "mir");
    }

    #[test]
    fn compile_with_output_short_flag() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "-o", "out.rs"]).unwrap();
        assert_eq!(parsed.output.as_deref(), Some("out.rs"));
    }

    #[test]
    fn compile_with_output_long_flag() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "--output", "out.rs"]).unwrap();
        assert_eq!(parsed.output.as_deref(), Some("out.rs"));
    }

    #[test]
    fn compile_with_output_equals_form() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "--output=out.rs"]).unwrap();
        assert_eq!(parsed.output.as_deref(), Some("out.rs"));
    }

    #[test]
    fn compile_with_output_equals_form_allows_dash_prefixed_path() {
        let parsed = run(&["ts-aot", "compile", "in.ts", "--output=--weird.rs"]).unwrap();
        assert_eq!(parsed.output.as_deref(), Some("--weird.rs"));
    }

    #[test]
    fn compile_output_rejects_dash_prefixed_value() {
        let err = run(&["ts-aot", "compile", "in.ts", "--output", "--emit-mir"]).unwrap_err();
        assert!(
            matches!(
                err.kind(),
                ErrorKind::UnknownArgument | ErrorKind::InvalidValue
            ),
            "expected unknown arg or invalid value, got: {:?}",
            err.kind()
        );
    }

    #[test]
    fn compile_with_directory_separator_in_path() {
        let parsed = run(&["ts-aot", "compile", "subdir/file.ts"]).unwrap();
        assert_eq!(parsed.input, "subdir/file.ts");
    }

    #[test]
    fn compile_with_double_dash_separator() {
        let parsed = run(&["ts-aot", "compile", "--", "--weird.ts"]).unwrap();
        assert_eq!(parsed.input, "--weird.ts");
    }

    #[test]
    fn compile_emits_diagnostic_kind_for_unknown_flag() {
        let err = run(&["ts-aot", "compile", "in.ts", "--no-such-flag"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn compile_emits_diagnostic_kind_for_invalid_emit() {
        let err = run(&["ts-aot", "compile", "in.ts", "--emit", "wasm"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn compile_requires_file_argument() {
        let err = run(&["ts-aot", "compile"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn help_subcommand_returns_display_help() {
        let err = run(&["ts-aot", "help"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn help_compile_subcommand_returns_display_help() {
        let err = run(&["ts-aot", "help", "compile"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_short_flag_returns_display_version() {
        let err = run(&["ts-aot", "-V"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn version_long_flag_returns_display_version() {
        let err = run(&["ts-aot", "--version"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn help_short_flag_returns_display_help() {
        let err = run(&["ts-aot", "-h"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn help_long_flag_returns_display_help() {
        let err = run(&["ts-aot", "--help"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn help_long_flag_inside_compile_returns_display_help() {
        let err = run(&["ts-aot", "compile", "--help"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn help_short_flag_inside_compile_returns_display_help() {
        let err = run(&["ts-aot", "compile", "-h"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_module_exposes_program_constants() {
        assert_eq!(super::PROGRAM_NAME, "ts-aot");
        assert!(!super::VERSION.is_empty());
    }

    #[test]
    fn emit_stage_arg_maps_to_emit_stage() {
        use crate::EmitStage;
        assert_eq!(EmitStage::from(EmitStageArg::Rust), EmitStage::Rust);
        assert_eq!(EmitStage::from(EmitStageArg::Hir), EmitStage::Hir);
        assert_eq!(EmitStage::from(EmitStageArg::Mir), EmitStage::Mir);
    }
}
