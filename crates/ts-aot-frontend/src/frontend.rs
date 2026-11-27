use std::cell::Cell;
use std::collections::HashSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;
use ts_aot_core::{Diagnostic, DiagnosticBag, ModuleId, Span as CoreSpan, TypeTable};
use ts_aot_ir_hir::HirProgram;

use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::util::source_type_for;

const PARSE_PANIC_CODE: &str = "E0100";
const PARSE_ERROR_CODE: &str = "E0200";

fn labeled_span_or(err: &oxc_diagnostics::OxcDiagnostic, fallback: CoreSpan) -> CoreSpan {
    let span = crate::semantic_analyze::error_span(err);
    if span.start == 0 && span.end == 0 {
        fallback
    } else {
        span
    }
}

#[derive(Debug, Clone, Default)]
pub struct FrontendPass;

impl FrontendPass {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn run(&self, name: &str, source: &str) -> FrontendOutput {
        self.run_with_types(name, source, &mut TypeTable::new(), false)
    }

    #[must_use]
    pub fn run_with_types(
        &self,
        name: &str,
        source: &str,
        types: &mut TypeTable,
        module: bool,
    ) -> FrontendOutput {
        let allocator = Allocator::default();
        let source_type = source_type_for(name, module);

        let parser = Parser::new(&allocator, source, source_type);
        let ret = parser.parse();

        let mut diagnostics = DiagnosticBag::new();
        let module_id = ModuleId::from_raw(0);
        let mut program = HirProgram::new(module_id);
        program.is_module = module;

        let end = u32::try_from(source.len()).unwrap_or(u32::MAX);
        let fallback_span = CoreSpan::new(0, end);

        if ret.panicked {
            if ret.diagnostics.is_empty() {
                diagnostics.push(Diagnostic::error(
                    PARSE_PANIC_CODE,
                    "internal parser panic",
                    fallback_span,
                ));
            } else {
                for oxc_err in &ret.diagnostics {
                    diagnostics.push(Diagnostic::error(
                        PARSE_PANIC_CODE,
                        format!("internal parser panic: {oxc_err}"),
                        labeled_span_or(oxc_err, fallback_span),
                    ));
                }
            }
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
                labeled_span_or(oxc_err, fallback_span),
            ));
        }

        let oxc_program: &Program<'_> = &ret.program;
        if source.contains("use strict") {
            for sem_err in &oxc_semantic::SemanticBuilder::new()
                .with_check_syntax_error(true)
                .build(oxc_program)
                .diagnostics
            {
                let msg = sem_err.to_string();
                if msg.contains("non-simple parameter list") {
                    diagnostics.push(Diagnostic::error(
                        PARSE_ERROR_CODE,
                        format!("parse error: {msg}"),
                        crate::semantic_analyze::error_span(sem_err),
                    ));
                }
            }
        }
        let mut senv = SkeletonEnv {
            types,
            diagnostics: &mut diagnostics,
            program: &mut program,
            next_destructured_id: Cell::new(0),
            destructured_temp_ids: HashSet::new(),
            capture_types: Vec::new(),
        };
        let source_len = u32::try_from(source.len()).unwrap_or(u32::MAX);
        SkeletonBuilder::new(source_len, module).build(&mut senv, oxc_program);

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
