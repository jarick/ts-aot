use oxc_ast::ast::{
    BindingPattern, Declaration, ForInStatement, ForOfStatement, ForStatement, ForStatementInit,
    ForStatementLeft, Statement, SwitchStatement, VariableDeclaration, match_assignment_target,
    match_declaration, match_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{Atom, LocalId, Span, Type};
use ts_aot_ir_hir::{HirCallee, HirExpr, HirStmt, HirSwitchCase};

use crate::ops::{label_atom, left_span};
use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::util::{binding_pattern_name, core_span_from_oxc};

impl SkeletonEnv<'_> {
    pub(crate) fn expand_array_destructuring(
        &mut self,
        array_pat: &oxc_ast::ast::ArrayPattern<'_>,
        init: HirExpr,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let init_ty_id = init.ty();
        let Some(Type::Array { element }) = self.types.resolve(init_ty_id) else {
            self.report_unwalked(
                "array destructuring requires an array type on the right-hand side",
                array_pat.span,
            );
            return;
        };
        let element_ty_id = *element;
        let span = core_span_from_oxc(array_pat.span);
        let destructured_uid = self.next_destructured_id.get();
        self.next_destructured_id.set(destructured_uid + 1);
        let temp_name = Atom::from(format!("_destructured_{destructured_uid}"));
        let temp_id = scope.declare(temp_name.as_str(), init_ty_id);
        out.push(HirStmt::Let {
            id: temp_id,
            name: temp_name,
            ty: init_ty_id,
            init: Some(init),
        });
        let temp_ref = HirExpr::Local {
            id: temp_id,
            ty: init_ty_id,
            span,
        };
        let runtime_name = Atom::from("__ts_aot_array_get_or_default");
        for (idx, element) in array_pat.elements.iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            let BindingPattern::BindingIdentifier(id) = element else {
                self.report_unwalked(
                    "array destructuring element must be a simple binding name (no nested patterns, no defaults, no rename)",
                    element.span(),
                );
                continue;
            };
            let name_str = id.name.to_compact_str();
            let new_id = scope.declare(name_str.as_str(), element_ty_id);
            let init_call = HirExpr::Call {
                callee: HirCallee::Runtime {
                    name: runtime_name.clone(),
                    ty: element_ty_id,
                },
                args: vec![
                    temp_ref.clone(),
                    HirExpr::Int(
                        i64::try_from(idx).expect("array index fits in i64 (usize overflow)"),
                        span,
                    ),
                ],
                type_args: Vec::new(),
                ty: element_ty_id,
                span,
            };
            out.push(HirStmt::Let {
                id: new_id,
                name: Atom::from(name_str.as_str()),
                ty: element_ty_id,
                init: Some(init_call),
            });
        }
    }
}

impl SkeletonBuilder {
    pub(crate) fn walk_stmts(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        let mut out = Vec::new();
        for s in stmts {
            self.walk_stmt(senv, s, &mut out, scope);
        }
        out
    }

    fn walk_child(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        scope: &mut BodyScope,
    ) -> Box<HirStmt> {
        let mut inner = Vec::new();
        self.walk_stmt(senv, s, &mut inner, scope);
        if inner.len() == 1 {
            Box::new(inner.into_iter().next().expect("len checked to be 1"))
        } else {
            Box::new(HirStmt::Block(inner))
        }
    }

    fn walk_stmt(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        match s {
            Statement::BlockStatement(b) => {
                scope.push();
                let inner = self.walk_block_with_predeclare(senv, &b.body, scope);
                scope.pop();
                out.push(HirStmt::Block(inner));
            }
            d @ match_declaration!(Statement) => {
                let decl = d.to_declaration();
                match decl {
                    Declaration::VariableDeclaration(v) => {
                        self.walk_var_decl(senv, v, out, scope);
                    }
                    other => {
                        senv.report_unwalked(
                            "statement form is not supported by the body walker",
                            other.span(),
                        );
                    }
                }
            }
            Statement::ExpressionStatement(e) => {
                let expr = self.walk_expr(senv, &e.expression, scope);
                out.push(HirStmt::Expr { expr });
            }
            Statement::ReturnStatement(r) => {
                let value = r.argument.as_ref().map(|e| self.walk_expr(senv, e, scope));
                out.push(HirStmt::Return { value });
            }
            Statement::IfStatement(i) => {
                let cond = self.walk_expr(senv, &i.test, scope);
                let then = self.walk_child(senv, &i.consequent, scope);
                let otherwise = i
                    .alternate
                    .as_ref()
                    .map(|a| self.walk_child(senv, a, scope));
                out.push(HirStmt::If {
                    cond,
                    then,
                    otherwise,
                });
            }
            Statement::WhileStatement(w) => {
                let cond = self.walk_expr(senv, &w.test, scope);
                let body = self.walk_child(senv, &w.body, scope);
                out.push(HirStmt::While { cond, body });
            }
            Statement::DoWhileStatement(d) => {
                let body = self.walk_child(senv, &d.body, scope);
                let cond = self.walk_expr(senv, &d.test, scope);
                out.push(HirStmt::DoWhile { body, cond });
            }
            Statement::ForStatement(f) => self.walk_c_for(senv, f, out, scope),
            Statement::ForOfStatement(f) => self.walk_for_of(senv, f, out, scope),
            Statement::ForInStatement(f) => self.walk_for_in(senv, f, out, scope),
            Statement::SwitchStatement(sw) => self.walk_switch(senv, sw, out, scope),
            Statement::ThrowStatement(t) => {
                let expr = self.walk_expr(senv, &t.argument, scope);
                out.push(HirStmt::Throw { expr });
            }
            Statement::BreakStatement(b) => {
                if b.label.is_some() {
                    senv.report_unwalked(
                        "labeled `break` is not supported by the body walker",
                        b.span,
                    );
                }
                out.push(HirStmt::Break {
                    label: label_atom(b.label.as_ref().map(|l| l.name.as_str())),
                });
            }
            Statement::ContinueStatement(c) => {
                if c.label.is_some() {
                    senv.report_unwalked(
                        "labeled `continue` is not supported by the body walker",
                        c.span,
                    );
                }
                out.push(HirStmt::Continue {
                    label: label_atom(c.label.as_ref().map(|l| l.name.as_str())),
                });
            }
            Statement::LabeledStatement(l) => {
                senv.report_unwalked(
                    "labeled statements are not supported by the body walker",
                    l.span,
                );
                self.walk_stmt(senv, &l.body, out, scope);
            }
            Statement::EmptyStatement(_) | Statement::DebuggerStatement(_) => {}
            other => {
                senv.report_unwalked(
                    "statement form is not supported by the body walker",
                    other.span(),
                );
            }
        }
    }

    fn walk_for_of(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForOfStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let iter = self.walk_expr(senv, &f.right, scope);
        scope.push();
        let binding = self.for_binding(senv, &f.left, scope);
        let body = self.walk_child(senv, &f.body, scope);
        scope.pop();
        out.push(HirStmt::ForOf {
            binding,
            iter,
            body,
        });
    }

    fn walk_for_in(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForInStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let iter = self.walk_expr(senv, &f.right, scope);
        scope.push();
        let binding = self.for_binding(senv, &f.left, scope);
        let body = self.walk_child(senv, &f.body, scope);
        scope.pop();
        out.push(HirStmt::ForIn {
            binding,
            iter,
            body,
        });
    }

    fn walk_switch(
        &mut self,
        senv: &mut SkeletonEnv,
        sw: &SwitchStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let disc = self.walk_expr(senv, &sw.discriminant, scope);
        scope.push();
        let mut cases = Vec::with_capacity(sw.cases.len());
        for c in &sw.cases {
            let test = c.test.as_ref().map(|e| self.walk_expr(senv, e, scope));
            let body = self.walk_stmts(senv, &c.consequent, scope);
            cases.push(HirSwitchCase { test, body });
        }
        scope.pop();
        out.push(HirStmt::Switch { disc, cases });
    }

    fn walk_var_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        v: &VariableDeclaration<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        for d in &v.declarations {
            let init = d.init.as_ref().map(|e| self.walk_expr(senv, e, scope));
            if let BindingPattern::ArrayPattern(array_pat) = &d.id {
                let Some(init_expr) = init else {
                    senv.report_unwalked(
                        "array destructuring without initializer is not yet supported",
                        d.span,
                    );
                    continue;
                };
                if array_pat.rest.is_some() {
                    senv.report_unwalked(
                        "array destructuring with `...rest` is not yet supported",
                        d.span,
                    );
                    continue;
                }
                senv.expand_array_destructuring(array_pat, init_expr, out, scope);
                continue;
            }
            let Some(name) = binding_pattern_name(&d.id) else {
                senv.report_unwalked(
                    "destructuring binding is not supported by the body walker",
                    d.span,
                );
                continue;
            };
            let ty = self.resolve_ts_type_from_annotation(senv, d.type_annotation.as_deref());
            let id = scope.declare(name.as_str(), ty);
            out.push(HirStmt::Let {
                id,
                name: Atom::from(name.as_str()),
                ty,
                init,
            });
        }
    }

    fn walk_c_for(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        scope.push();
        let mut block: Vec<HirStmt> = Vec::new();
        if let Some(init) = &f.init {
            match init {
                ForStatementInit::VariableDeclaration(v) => {
                    self.walk_var_decl(senv, v, &mut block, scope);
                }
                e @ match_expression!(ForStatementInit) => {
                    let expr = self.walk_expr(senv, e.to_expression(), scope);
                    block.push(HirStmt::Expr { expr });
                }
            }
        }
        let cond = match &f.test {
            Some(e) => self.walk_expr(senv, e, scope),
            None => HirExpr::Bool(true, Span::default()),
        };
        let mut loop_body = Vec::new();
        self.walk_stmt(senv, &f.body, &mut loop_body, scope);
        if let Some(update) = &f.update {
            let expr = self.walk_expr(senv, update, scope);
            inject_update_before_continue(&mut loop_body, &expr);
            loop_body.push(HirStmt::Expr { expr });
        }
        block.push(HirStmt::While {
            cond,
            body: Box::new(HirStmt::Block(loop_body)),
        });
        scope.pop();
        out.push(HirStmt::Block(block));
    }

    fn for_binding(
        &mut self,
        senv: &mut SkeletonEnv,
        left: &ForStatementLeft<'_>,
        scope: &mut BodyScope,
    ) -> LocalId {
        if let ForStatementLeft::VariableDeclaration(v) = left
            && let Some(d) = v.declarations.first()
            && let Some(name) = binding_pattern_name(&d.id)
        {
            let ty = self.resolve_ts_type_from_annotation(senv, d.type_annotation.as_deref());
            return scope.declare(name.as_str(), ty);
        }
        if let t @ match_assignment_target!(ForStatementLeft) = left
            && let t = t.to_assignment_target()
            && let Some(name) = extract_simple_target_name(t)
        {
            let ty = senv.error_ty();
            return scope.declare(&name, ty);
        }
        senv.report_unwalked(
            "loop binding must be a simple `let`/`const` identifier",
            left_span(left),
        );
        let ty = senv.error_ty();
        scope.declare("_", ty)
    }
}

fn extract_simple_target_name(t: &oxc_ast::ast::AssignmentTarget<'_>) -> Option<String> {
    use oxc_ast::ast::SimpleAssignmentTarget;
    match t.as_simple_assignment_target()? {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn inject_update_before_continue(stmts: &mut Vec<HirStmt>, update: &HirExpr) {
    let mut i = 0;
    while i < stmts.len() {
        if matches!(stmts[i], HirStmt::Continue { .. }) {
            stmts.insert(
                i,
                HirStmt::Expr {
                    expr: update.clone(),
                },
            );
            i += 2;
            continue;
        }
        inject_update_in_child(&mut stmts[i], update);
        i += 1;
    }
}

fn inject_update_in_child(s: &mut HirStmt, update: &HirExpr) {
    match s {
        HirStmt::Block(inner) => inject_update_before_continue(inner, update),
        HirStmt::If {
            then, otherwise, ..
        } => {
            inject_update_in_boxed(then, update);
            if let Some(otherwise) = otherwise {
                inject_update_in_boxed(otherwise, update);
            }
        }
        HirStmt::Switch { cases, .. } => {
            for c in cases {
                inject_update_before_continue(&mut c.body, update);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            inject_update_in_boxed(body, update);
            if let Some(catch) = catch {
                inject_update_in_boxed(&mut catch.body, update);
            }
            if let Some(finally) = finally {
                inject_update_in_boxed(finally, update);
            }
        }
        _ => {}
    }
}

fn inject_update_in_boxed(s: &mut Box<HirStmt>, update: &HirExpr) {
    if matches!(**s, HirStmt::Continue { .. }) {
        let cont = std::mem::replace(s.as_mut(), HirStmt::Break { label: None });
        **s = HirStmt::Block(vec![
            HirStmt::Expr {
                expr: update.clone(),
            },
            cont,
        ]);
    } else {
        inject_update_in_child(s, update);
    }
}
