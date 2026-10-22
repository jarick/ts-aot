use oxc_ast::ast::{FunctionBody, Statement};
use ts_aot_core::{Atom, TypeId};
use ts_aot_ir_hir::{HirExpr, HirParam, HirStmt};

use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv};

pub(crate) struct FunctionBodyInput<'a, 'b> {
    pub body: Option<&'a FunctionBody<'b>>,
    pub params: &'a [HirParam],
    pub extra_locals: &'a [(Atom, TypeId, HirExpr)],
    pub param_base: u32,
}

impl SkeletonBuilder {
    pub(crate) fn walk_function_body(
        &mut self,
        senv: &mut SkeletonEnv,
        input: &FunctionBodyInput<'_, '_>,
        is_generator: bool,
        inherit_this_scope: bool,
    ) -> (Vec<HirStmt>, u32) {
        let body = input.body;
        let params = input.params;
        let extra_locals = input.extra_locals;
        let param_base = input.param_base;
        let base = param_base.max(
            params
                .iter()
                .map(|p| p.id.raw())
                .max()
                .map_or(0, |m| m.saturating_add(1)),
        );
        let Some(body) = body else {
            return (Vec::new(), base);
        };
        let saved_static = self.in_static_method;
        if !inherit_this_scope {
            self.in_static_method = false;
        }
        self.is_generator_stack.push(is_generator);
        let mut scope = BodyScope::new(base);
        for p in params {
            scope.declare_param(p.name.as_str(), p.id, p.ty);
        }
        let mut prelude: Vec<HirStmt> = Vec::with_capacity(extra_locals.len());
        for (name, ty, init) in extra_locals {
            let id = scope.declare(name.as_str(), *ty);
            prelude.push(HirStmt::Let {
                id,
                name: name.clone(),
                ty: *ty,
                init: Some(init.clone()),
            });
        }
        let mut result = self.walk_block_with_predeclare(senv, &body.statements, &mut scope);
        self.is_generator_stack.pop();
        self.in_static_method = saved_static;
        if !prelude.is_empty() {
            prelude.append(&mut result);
            result = prelude;
        }
        let used = scope.peek_next();
        (result, used)
    }

    pub(crate) fn walk_block_with_predeclare(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[oxc_ast::ast::Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        predeclare_forward_declarations(stmts, scope, senv.error_ty());
        self.walk_stmts(senv, stmts, scope)
    }

    pub(crate) fn current_function_is_generator(&self) -> bool {
        self.is_generator_stack
            .last()
            .copied()
            .expect("is_generator_stack must be non-empty at every call site (walk_function_body pushes before invoking the walker)")
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
