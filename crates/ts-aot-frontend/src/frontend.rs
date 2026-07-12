use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;
use ts_aot_core::{Diagnostic, DiagnosticBag, ModuleId, Span as CoreSpan, TypeTable};
use ts_aot_ir_hir::HirProgram;

use crate::skeleton::SkeletonBuilder;
use crate::util::source_type_for;

const PARSE_PANIC_CODE: &str = "E0100";
const PARSE_ERROR_CODE: &str = "E0200";

#[derive(Debug, Clone, Default)]
pub struct FrontendPass;

impl FrontendPass {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn run(&self, name: &str, source: &str) -> FrontendOutput {
        self.run_with_types(name, source, &mut TypeTable::new())
    }

    #[must_use]
    pub fn run_with_types(
        &self,
        name: &str,
        source: &str,
        types: &mut TypeTable,
    ) -> FrontendOutput {
        let allocator = Allocator::default();
        let source_type = source_type_for(name);

        let parser = Parser::new(&allocator, source, source_type);
        let ret = parser.parse();

        let mut diagnostics = DiagnosticBag::new();
        let module = ModuleId::from_raw(0);
        let mut program = HirProgram::new(module);

        let end = u32::try_from(source.len()).unwrap_or(u32::MAX);
        let fallback_span = CoreSpan::new(0, end);

        if ret.panicked {
            diagnostics.push(Diagnostic::error(
                PARSE_PANIC_CODE,
                "internal parser panic",
                fallback_span,
            ));
            program.diagnostics.extend(diagnostics.iter().cloned());
            return FrontendOutput {
                program,
                diagnostics,
            };
        }

        for oxc_err in &ret.diagnostics {
            diagnostics.push(Diagnostic::error(
                PARSE_ERROR_CODE,
                format!("parse error: {oxc_err}"),
                fallback_span,
            ));
        }

        let oxc_program: &Program<'_> = &ret.program;
        SkeletonBuilder {
            source,
            types,
            diagnostics: &mut diagnostics,
            program: &mut program,
            next_generic_param: 0,
            resolved_aliases: HashMap::new(),
        }
        .build(oxc_program);

        program.diagnostics.extend(diagnostics.iter().cloned());
        FrontendOutput {
            program,
            diagnostics,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrontendOutput {
    pub program: HirProgram,
    pub diagnostics: DiagnosticBag,
}
