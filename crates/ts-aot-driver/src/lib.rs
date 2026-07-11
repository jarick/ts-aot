pub mod cli;

mod pipeline;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

pub use ts_aot_core::{DiagnosticBag, Severity};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EmitStage {
    #[default]
    Rust,
    Hir,
    Mir,
}

impl EmitStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Hir => "hir",
            Self::Mir => "mir",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompileOptions {
    pub emit: EmitStage,
}

#[derive(Debug, Default)]
pub struct DriverOutput {
    pub rust_source: Option<String>,
    pub hir_text: Option<String>,
    pub mir_text: Option<String>,
    pub diagnostics: DiagnosticBag,
}

impl DriverOutput {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    #[must_use]
    pub fn artifact(&self, stage: EmitStage) -> Option<&str> {
        match stage {
            EmitStage::Rust => self.rust_source.as_deref(),
            EmitStage::Hir => self.hir_text.as_deref(),
            EmitStage::Mir => self.mir_text.as_deref(),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DriverError {
    Io {
        path: String,
        source: std::io::Error,
    },
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "read {path}: {source}"),
        }
    }
}

impl std::error::Error for DriverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Default)]
pub struct Driver;

impl Driver {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn compile_source(&self, name: &str, source: &str, opts: &CompileOptions) -> DriverOutput {
        pipeline::run(name, source, opts)
    }

    pub fn compile_file(
        &self,
        path: impl AsRef<Path>,
        opts: &CompileOptions,
    ) -> Result<DriverOutput, DriverError> {
        let path = path.as_ref();
        let display = path.display().to_string();
        let source = fs::read_to_string(path).map_err(|e| DriverError::Io {
            path: display.clone(),
            source: e,
        })?;
        Ok(self.compile_source(&display, &source, opts))
    }
}

#[must_use]
pub fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    }
}
