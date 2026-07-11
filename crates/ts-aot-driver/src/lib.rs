pub mod cli;

mod pipeline;

#[cfg(test)]
mod tests;

use ts_aot_core::DiagnosticBag;

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
}
