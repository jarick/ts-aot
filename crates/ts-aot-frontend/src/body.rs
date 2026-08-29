use oxc_ast::ast::{FunctionBody, Statement};
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
        let result = self.walk_block_with_predeclare(&body.statements, &mut scope);
        self.is_generator_stack.pop();
        result
    }

    pub(crate) fn walk_block_with_predeclare(
        &mut self,
        stmts: &[oxc_ast::ast::Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        predeclare_forward_declarations(stmts, scope, self.error_ty());
        self.walk_stmts(stmts, scope)
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

fn predeclare_forward_declarations(
    stmts: &[Statement<'_>],
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    for stmt in stmts {
        if let Some(decl) = stmt.as_declaration() {
            predeclare_in_decl(decl, scope, error_ty);
        }
    }
}

fn predeclare_in_decl(
    decl: &oxc_ast::ast::Declaration<'_>,
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    match decl {
        oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
            if let Some(id) = f.id.as_ref() {
                scope.predeclare(id.name.as_str(), error_ty);
            }
        }
        oxc_ast::ast::Declaration::VariableDeclaration(v) => {
            for d in &v.declarations {
                predeclare_binding_pattern(&d.id, scope, error_ty);
            }
        }
        _ => {}
    }
}

fn predeclare_binding_pattern(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    use oxc_ast::ast::BindingPattern;
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            scope.predeclare(id.name.as_str(), error_ty);
        }
        BindingPattern::AssignmentPattern(ap) => {
            predeclare_binding_pattern(&ap.left, scope, error_ty);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                predeclare_binding_pattern(&prop.value, scope, error_ty);
            }
            if let Some(rest) = &obj.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for p in arr.elements.iter().flatten() {
                predeclare_binding_pattern(p, scope, error_ty);
            }
            if let Some(rest) = &arr.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty);
            }
        }
    }
}
