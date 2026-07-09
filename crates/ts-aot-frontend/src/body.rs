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
    ) -> Vec<HirStmt> {
        let Some(body) = body else {
            return Vec::new();
        };
        let param_count = u32::try_from(params.len()).unwrap_or(u32::MAX);
        let mut scope = BodyScope::new(param_count);
        for (i, p) in params.iter().enumerate() {
            let id = LocalId::from_raw(u32::try_from(i).unwrap_or(u32::MAX));
            scope.declare_param(p.name.as_str(), id, p.ty);
        }
        self.walk_stmts(&body.statements, &mut scope)
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
