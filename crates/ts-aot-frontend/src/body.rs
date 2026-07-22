use oxc_ast::ast::FunctionBody;
use ts_aot_core::{Diagnostic, LocalId, Type, TypeId};
use ts_aot_ir_hir::{HirParam, HirStmt};

use crate::scope::BodyScope;
use crate::skeleton::SkeletonBuilder;
use crate::util::core_span_from_oxc;

const UNSUPPORTED_BODY_CODE: &str = "E0500";

impl SkeletonBuilder<'_, '_> {
    pub(crate) fn walk_function_body(
        &mut self,
        body: Option<&FunctionBody<'_>>,
        params: &[HirParam],
        is_generator: bool,
    ) -> Vec<HirStmt> {
        let Some(body) = body else {
            return Vec::new();
        };
        self.is_generator_stack.push(is_generator);
        let param_count = u32::try_from(params.len()).unwrap_or(u32::MAX);
        let mut scope = BodyScope::new(param_count);
        for (i, p) in params.iter().enumerate() {
            let id = LocalId::from_raw(u32::try_from(i).unwrap_or(u32::MAX));
            scope.declare_param(p.name.as_str(), id, p.ty);
        }
        let result = self.walk_stmts(&body.statements, &mut scope);
        self.is_generator_stack.pop();
        result
    }

    pub(crate) fn current_function_is_generator(&self) -> bool {
        self.is_generator_stack
            .last()
            .copied()
            .expect("is_generator_stack must be non-empty at every call site (walk_function_body pushes before invoking the walker)")
    }

    pub(crate) fn error_ty(&mut self) -> TypeId {
        self.types.intern(&Type::Error)
    }

    pub(crate) fn report_unwalked(&mut self, message: &str, span: oxc_span::Span) {
        self.diagnostics.push(Diagnostic::warning(
            UNSUPPORTED_BODY_CODE,
            message,
            core_span_from_oxc(span),
        ));
    }
}
