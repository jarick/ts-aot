use std::collections::HashMap;

use oxc_ast::ast::{
    BindingPattern, Declaration, Expression, ForInStatement, ForOfStatement, ForStatement,
    ForStatementInit, ForStatementLeft, SimpleAssignmentTarget, Statement, SwitchStatement,
    TryStatement, VariableDeclaration, VariableDeclarationKind, match_assignment_target,
    match_declaration, match_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{Atom, LocalId, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirCatchBinding, HirCatchClause, HirExpr, HirStmt, HirSwitchCase,
};

use crate::body::LoopEffects;
use crate::ops::{label_atom, left_span};
use crate::scope::BodyScope;
use crate::skeleton::{
    ClassValueFieldBase, SkeletonBuilder, SkeletonEnv, SwitchCaseEvent, SwitchCaseEventKind,
};
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
        self.walk_child_maybe_conditional(senv, s, scope, true)
    }

    fn walk_child_maybe_conditional(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        scope: &mut BodyScope,
        conditional: bool,
    ) -> Box<HirStmt> {
        let mut inner = Vec::new();
        let braceless = !matches!(s, Statement::BlockStatement(_));
        if braceless {
            self.suppress_narrowing += 1;
        }
        if conditional {
            self.enter_class_value_branch();
        }
        self.walk_stmt(senv, s, &mut inner, scope);
        if conditional {
            self.leave_class_value_branch();
        }
        if braceless {
            self.suppress_narrowing -= 1;
        }
        if inner.len() == 1 {
            Box::new(inner.into_iter().next().expect("len checked to be 1"))
        } else {
            Box::new(HirStmt::Block(inner))
        }
    }

    pub(super) fn redirect_narrowed_target(
        &mut self,
        target: HirExpr,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let HirExpr::Local { id, .. } = &target else {
            return target;
        };
        let narrowed_id = *id;
        let Some((storage_id, storage_ty)) = self.narrowed_storage(narrowed_id) else {
            return target;
        };
        let mut target = target;
        if let HirExpr::Local { id, ty, .. } = &mut target {
            *id = storage_id;
            *ty = storage_ty;
        }
        scope.widen_id(narrowed_id);
        target
    }

    fn widen_before_loop(&mut self, scope: &mut BodyScope, effects: &LoopEffects) {
        if effects.has_effect {
            self.constant_int_locals.clear();
        } else {
            for name in &effects.writes {
                if let Some(id) = scope.visible_binding(name) {
                    self.constant_int_locals
                        .remove(&self.class_value_storage(id));
                }
            }
        }
        if effects.has_effect {
            for name in scope.names() {
                Self::widen_binding(scope, &name);
            }
            return;
        }
        for name in &effects.writes {
            Self::widen_binding(scope, name);
        }
    }

    fn widen_binding(scope: &mut BodyScope, name: &str) {
        let Some(id) = scope.visible_binding(name) else {
            return;
        };
        scope.widen_id(id);
    }

    fn while_effects(test: &Expression<'_>, body: &Statement<'_>) -> LoopEffects {
        let mut effects = LoopEffects::default();
        effects.scan_expr(test);
        effects.scan_stmt(body);
        effects
    }

    fn for_effects(f: &ForStatement<'_>) -> LoopEffects {
        let mut effects = LoopEffects::default();
        if let Some(init) = &f.init {
            effects.scan_for_init(init);
        }
        if let Some(test) = &f.test {
            effects.scan_expr(test);
        }
        if let Some(update) = &f.update {
            effects.scan_expr(update);
        }
        effects.scan_stmt(&f.body);
        effects
    }

    fn for_binding_effects(
        left: &ForStatementLeft<'_>,
        right: &Expression<'_>,
        body: &Statement<'_>,
    ) -> LoopEffects {
        let mut effects = LoopEffects::default();
        effects.scan_for_left(left);
        effects.scan_expr(right);
        effects.scan_stmt(body);
        effects
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
                let previous_statement_expr = self.own_statement_expr;
                self.own_statement_expr = true;
                let mut expr = self.walk_expr(senv, &e.expression, scope);
                self.own_statement_expr = previous_statement_expr;
                let narrowing = self.narrow_after_assignment(senv, &e.expression, &mut expr, scope);
                let copy_back = self.finish_statement_expression(&expr, senv);
                out.push(HirStmt::Expr { expr });
                if let Some(stmt) = narrowing {
                    out.push(stmt);
                }
                if let Some(stmt) = copy_back {
                    out.push(stmt);
                }
            }
            Statement::ReturnStatement(r) => {
                let value = r.argument.as_ref().map(|e| self.walk_expr(senv, e, scope));
                self.note_may_exit(false);
                out.push(HirStmt::Return { value });
            }
            Statement::IfStatement(i) => {
                let cond = self.walk_expr(senv, &i.test, scope);
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                let constant = match &cond {
                    HirExpr::Bool(true, _) => Some(true),
                    HirExpr::Bool(false, _) => Some(false),
                    // An immediately preceding boolean initializer cannot have been
                    // changed before this direct local read. Preserve dead-branch facts.
                    HirExpr::Local { id, .. } => match out.last() {
                        Some(HirStmt::Let {
                            id: declared,
                            init: Some(HirExpr::Bool(value, _)),
                            ..
                        }) if declared == id => Some(*value),
                        _ => None,
                    },
                    _ => None,
                };
                let joins = constant.is_none() && i.alternate.is_some();
                let saved_fields = if joins {
                    Some(self.snapshot_class_value_field_state())
                } else {
                    None
                };
                let mut branch_fields = Vec::new();
                let then_start = self.class_value_poison_writes.len();
                let then = if constant == Some(false) {
                    self.walk_child(senv, &i.consequent, scope)
                } else {
                    self.walk_child_maybe_conditional(
                        senv,
                        &i.consequent,
                        scope,
                        constant.is_none(),
                    )
                };
                if saved_fields.is_some() {
                    branch_fields.push(self.snapshot_class_value_field_state());
                }
                let then_constants = self.snapshot_constant_int_locals();
                if constant != Some(true) {
                    self.restore_constant_int_locals(&saved_constants);
                }
                let else_start = self.class_value_poison_writes.len();
                if let Some(saved) = &saved_fields {
                    self.restore_class_value_field_state(saved);
                }
                let otherwise = i.alternate.as_ref().map(|a| {
                    if constant == Some(true) {
                        self.walk_child(senv, a, scope)
                    } else {
                        self.walk_child_maybe_conditional(senv, a, scope, constant.is_none())
                    }
                });
                if saved_fields.is_some() {
                    branch_fields.push(self.snapshot_class_value_field_state());
                }
                if constant == Some(true) {
                    self.restore_constant_int_locals(&then_constants);
                } else if constant.is_none() {
                    self.intersect_constant_int_locals(&then_constants);
                }
                self.join_class_value_field_states(&branch_fields);
                if joins {
                    let else_end = self.class_value_poison_writes.len();
                    let branch_depth = self.conditional_depth + 1;
                    self.join_class_value_field_poison(
                        &[(then_start, else_start), (else_start, else_end)],
                        &branch_fields,
                        saved_fields.as_ref().expect("joins implies a snapshot"),
                        branch_depth,
                    );
                }
                self.merge_unknown_class_value_locals(saved_unknown);
                out.push(HirStmt::If {
                    cond,
                    then,
                    otherwise,
                });
            }
            Statement::WhileStatement(w) => {
                self.widen_before_loop(scope, &Self::while_effects(&w.test, &w.body));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let cond = self.walk_expr(senv, &w.test, scope);
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                let body = self.walk_child(senv, &w.body, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
                out.push(HirStmt::While { cond, body });
            }
            Statement::DoWhileStatement(d) => {
                self.widen_before_loop(scope, &Self::while_effects(&d.test, &d.body));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                let body = self.walk_child_maybe_conditional(senv, &d.body, scope, false);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                let cond = self.walk_expr(senv, &d.test, scope);
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
                out.push(HirStmt::DoWhile { body, cond });
            }
            Statement::ForStatement(f) => {
                self.widen_before_loop(scope, &Self::for_effects(f));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_c_for(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::ForOfStatement(f) => {
                self.widen_before_loop(
                    scope,
                    &Self::for_binding_effects(&f.left, &f.right, &f.body),
                );
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_for_of(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::ForInStatement(f) => {
                self.widen_before_loop(
                    scope,
                    &Self::for_binding_effects(&f.left, &f.right, &f.body),
                );
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_for_in(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::SwitchStatement(sw) => {
                let saved_unknown = self.unknown_class_value_locals.clone();
                self.walk_switch(senv, sw, out, scope);
                self.merge_unknown_class_value_locals(saved_unknown);
            }
            Statement::TryStatement(t) => {
                let saved_unknown = self.unknown_class_value_locals.clone();
                self.walk_try(senv, t, out, scope);
                self.merge_unknown_class_value_locals(saved_unknown);
            }
            Statement::ThrowStatement(t) => {
                let expr = self.walk_expr(senv, &t.argument, scope);
                self.note_may_throw();
                out.push(HirStmt::Throw { expr });
            }
            Statement::BreakStatement(b) => {
                if b.label.is_some() {
                    senv.report_unwalked(
                        "labeled `break` is not supported by the body walker",
                        b.span,
                    );
                }
                self.note_may_exit(true);
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
                self.note_may_exit(true);
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
        let runs_user_code = f.r#await
            || !matches!(
                senv.types.resolve(iter.ty()),
                Some(Type::Array { .. } | Type::String)
            );
        if runs_user_code {
            self.invalidate_captured_narrowings(scope);
        }
        scope.push();
        let binding = self.for_binding(senv, &f.left, scope);
        let body = self.walk_child(senv, &f.body, scope);
        scope.pop();
        if f.r#await {
            out.push(HirStmt::ForAwaitOf {
                binding,
                iter,
                body,
            });
        } else {
            out.push(HirStmt::ForOf {
                binding,
                iter,
                body,
            });
        }
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
        let disc_is_class = self.is_class_value_expr(&disc, scope.body_id());
        let disc_constant = match &disc {
            HirExpr::Local { id, .. } => self
                .constant_int_locals
                .get(&self.class_value_storage(*id))
                .copied(),
            _ => None,
        };
        scope.push();
        for c in &sw.cases {
            crate::body::predeclare_forward_declarations(&c.consequent, scope, senv.error_ty());
        }
        let outer: HashMap<Atom, LocalId> = scope
            .names()
            .into_iter()
            .filter_map(|name| {
                scope
                    .visible_binding(name.as_str())
                    .map(|id| (Atom::from(name.as_str()), id))
            })
            .collect();
        self.push_switch_case_frame(scope.depth(), outer);
        let unconditional_default = sw.cases.len() == 1 && sw.cases[0].test.is_none();
        let has_default = sw.cases.iter().any(|c| c.test.is_none());
        let ends_every_case = sw.cases.iter().all(|c| case_covers_its_end(&c.consequent));
        let joins = !unconditional_default && has_default && ends_every_case;
        let saved_fields = if joins {
            Some(self.snapshot_class_value_field_state())
        } else {
            None
        };
        self.class_value_breakable_depth += 1;
        let mut branch_fields = Vec::new();
        let mut case_entries: Vec<crate::skeleton::ClassValueFieldState> = Vec::new();
        if let Some(saved) = &saved_fields {
            case_entries.push(saved.clone());
        }
        let mut cases = Vec::with_capacity(sw.cases.len());
        let mut case_writes = Vec::with_capacity(sw.cases.len());
        let mut group_start: Option<usize> = None;
        for c in &sw.cases {
            self.start_switch_case();
            if !case_entries.is_empty() {
                self.join_class_value_field_states(&case_entries);
            }
            let case_start = self.class_value_poison_writes.len();
            group_start.get_or_insert(case_start);
            if !unconditional_default {
                self.enter_class_value_branch();
            }
            let test = c.test.as_ref().map(|e| self.walk_expr(senv, e, scope));
            let checkpoint = scope.narrowing_checkpoint();
            let reachable = reachable_case_prefix(&c.consequent);
            let body = self.walk_stmts(senv, &c.consequent[..reachable], scope);
            scope.discard_narrowings_since(checkpoint);
            if !unconditional_default {
                self.leave_class_value_branch();
            }
            let case_end = self.class_value_poison_writes.len();
            cases.push(HirSwitchCase { test, body });
            if !c.consequent.is_empty() {
                if saved_fields.is_some() {
                    case_entries.push(self.snapshot_class_value_field_state());
                }
                if ends_with_a_terminator(&c.consequent) {
                    let start = group_start.take().unwrap_or(case_start);
                    case_writes.push((start, case_end));
                    if saved_fields.is_some() {
                        branch_fields.push(self.snapshot_class_value_field_state());
                    }
                }
            }
        }
        let case_frame = self.pop_switch_case_frame();
        if let Some(frame) = case_frame {
            check_switch_case_tdz(senv, &disc, disc_constant, &cases, &frame.events, sw.span);
        }
        if let Some(start) = group_start {
            case_writes.push((start, self.class_value_poison_writes.len()));
            if saved_fields.is_some() {
                let state = self.snapshot_class_value_field_state();
                case_entries.push(state.clone());
                branch_fields.push(state);
            }
        }
        self.join_class_value_field_states(&branch_fields);
        if joins {
            let branch_depth = self.conditional_depth + 1;
            self.join_class_value_field_poison(
                &case_writes,
                &branch_fields,
                saved_fields.as_ref().expect("joins implies a snapshot"),
                branch_depth,
            );
        }
        self.class_value_breakable_depth -= 1;
        for index in (0..cases.len().saturating_sub(1)).rev() {
            if !ends_with_a_terminator(&sw.cases[index].consequent) {
                let next = cases[index + 1].body.clone();
                cases[index].body.extend(next);
            }
        }
        if disc_is_class {
            for case in &cases {
                if case
                    .test
                    .as_ref()
                    .is_some_and(|test| self.is_class_value_expr(test, scope.body_id()))
                {
                    senv.report_unsupported(
                        "E0419",
                        "a class value used as a `switch` case is not supported; compare it with \
                         `===` instead",
                        sw.span,
                    );
                }
            }
            if cases
                .iter()
                .filter_map(|case| case.test.as_ref())
                .any(|test| {
                    !matches!(
                        test,
                        HirExpr::Int(..)
                            | HirExpr::Float(..)
                            | HirExpr::Bool(..)
                            | HirExpr::Null(..)
                            | HirExpr::Undefined(..)
                            | HirExpr::String(..)
                    )
                })
            {
                senv.report_unsupported(
                    "E0419",
                    "effectful switch tests on a class value are not supported",
                    sw.span,
                );
            }
            cases.retain(|case| {
                case.test
                    .as_ref()
                    .is_none_or(|test| self.is_class_value_expr(test, scope.body_id()))
            });
        }
        scope.pop();
        out.push(HirStmt::Switch { disc, cases });
    }

    fn walk_try(
        &mut self,
        senv: &mut SkeletonEnv,
        t: &TryStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        scope.push();
        let saved_skips = self.try_body_skips.replace(false);
        let saved_fields = t
            .handler
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        let entry_fields = t
            .finalizer
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        let writes_before = self.class_value_poison_writes.len();
        let exits_before = self.class_value_exits.len();
        let body = HirStmt::Block(self.walk_block_with_predeclare(senv, &t.block.body, scope));
        let body_may_skip = self.try_body_skips == Some(true);
        let body_fields = saved_fields
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        scope.pop();

        self.try_body_skips = Some(false);
        if let Some(saved) = &saved_fields {
            self.restore_class_value_field_state(saved);
        }
        let catch = t.handler.as_ref().map(|handler| {
            scope.push();
            self.enter_class_value_branch();
            let binding = handler.param.as_ref().and_then(|p| {
                let Some(name) = binding_pattern_name(&p.pattern) else {
                    senv.report_unwalked(
                        "destructuring catch bindings are not supported by the body walker",
                        p.span,
                    );
                    return None;
                };
                let ty = if p.type_annotation.is_some() {
                    self.resolve_ts_type_from_annotation(senv, p.type_annotation.as_deref())
                } else {
                    senv.error_ty()
                };
                let id = scope.declare(name.as_str(), ty);
                Some(HirCatchBinding::new(id, Atom::from(name.as_str()), ty))
            });
            let cbody =
                HirStmt::Block(self.walk_block_with_predeclare(senv, &handler.body.body, scope));
            self.leave_class_value_branch();
            scope.pop();
            HirCatchClause::new(binding, Box::new(cbody))
        });
        let catch_may_skip = self.try_body_skips == Some(true);

        match body_fields {
            Some(body_fields) if body_may_skip => {
                let catch_fields = self.snapshot_class_value_field_state();
                self.join_class_value_field_states(&[body_fields, catch_fields]);
            }
            Some(body_fields) => {
                self.restore_class_value_field_state(&body_fields);
            }
            None => {}
        }

        self.try_body_skips = Some(false);
        if let Some(entry_fields) = &entry_fields {
            self.degrade_class_value_fields_written_since(
                writes_before,
                exits_before,
                entry_fields,
            );
        }
        let finally = t.finalizer.as_ref().map(|f| {
            scope.push();
            let block = HirStmt::Block(self.walk_block_with_predeclare(senv, &f.body, scope));
            scope.pop();
            Box::new(block)
        });
        let finally_may_skip = self.try_body_skips == Some(true);

        let escapes = (t.handler.is_none() && body_may_skip) || catch_may_skip || finally_may_skip;
        self.try_body_skips = saved_skips.map(|previous| previous || escapes);

        out.push(HirStmt::Try {
            body: Box::new(body),
            catch,
            finally,
        });
    }

    fn walk_var_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        v: &VariableDeclaration<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        for d in &v.declarations {
            let simple_binding = matches!(&d.id, BindingPattern::BindingIdentifier(_));
            let previous_statement_expr = self.own_statement_expr;
            if simple_binding {
                self.own_statement_expr = true;
            }
            let init = d.init.as_ref().map(|e| self.walk_expr(senv, e, scope));
            self.own_statement_expr = previous_statement_expr;
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
            let mut ty = if d.type_annotation.is_some() {
                self.resolve_ts_type_from_annotation(senv, d.type_annotation.as_deref())
            } else {
                senv.error_ty()
            };
            let mut init = init;
            if d.type_annotation.is_none()
                && let Some(HirExpr::Closure { ty: fn_ty, .. }) = &init
            {
                ty = *fn_ty;
            }
            if d.type_annotation.is_none()
                && let Some(HirExpr::New { ty: new_ty, .. }) = &init
            {
                ty = *new_ty;
            }
            if d.type_annotation.is_none()
                && matches!(senv.types.resolve(ty), None | Some(Type::Error))
            {
                let inferred = init.as_ref().and_then(|e| {
                    crate::expr::literal_hir_ty(senv.types, e).or_else(|| {
                        e.has_static_type()
                            .then(|| e.ty())
                            .filter(|t| !matches!(senv.types.resolve(*t), None | Some(Type::Error)))
                    })
                });
                if let Some(t) = inferred {
                    ty = t;
                }
            }
            if let Some(HirExpr::ArrayLiteral { ty: arr_ty, .. }) = init.as_mut()
                && matches!(senv.types.resolve(ty), Some(Type::Array { .. }))
            {
                *arr_ty = ty;
            }
            if init.is_none()
                && d.type_annotation.is_some()
                && matches!(v.kind, VariableDeclarationKind::Var)
                && self.switch_case_frames.is_empty()
            {
                ty = crate::util::with_undefined(senv.types, ty);
            }
            let class_value = match &init {
                Some(HirExpr::DefaultConstruct {
                    ty: class_ty, span, ..
                }) => Some((*class_ty, *span)),
                _ => None,
            };
            let generic_class = matches!(
                &init,
                Some(HirExpr::DefaultConstruct { ty, .. })
                    if senv.program.declarations.iter().any(|d| matches!(
                        d,
                        ts_aot_ir_hir::HirDecl::Class(c)
                            if c.ty == *ty && !c.type_params.is_empty()
                    ))
            );
            if let Some((class_ty, span)) = class_value
                && !generic_class
            {
                let tag = self.class_value_tag(class_ty);
                init = Some(HirExpr::Int(tag, span));
                ty = senv.types.intern(&ts_aot_core::Type::I64);
            }
            let id = scope.declare(name.as_str(), ty);
            let switch_kind = if matches!(v.kind, VariableDeclarationKind::Var) {
                SwitchCaseEventKind::VarDecl
            } else {
                SwitchCaseEventKind::Decl
            };
            self.note_switch_case_declaration(
                id,
                &Atom::from(name.as_str()),
                scope.depth(),
                switch_kind,
            );
            if let Some(bodies) = init.as_ref().and_then(crate::skeleton::closure_bodies_of) {
                for body in bodies {
                    scope.set_closure_body(id, body);
                }
            } else {
                match &init {
                    Some(HirExpr::Local { id: source, .. }) => {
                        match scope.closure_bodies_of_local(*source) {
                            Some(bodies) => {
                                for body in bodies {
                                    scope.set_closure_body(id, body);
                                }
                            }
                            None => scope.poison_closure_body(id),
                        }
                    }
                    Some(init) if crate::skeleton::may_hold_a_callable(init) => {
                        scope.poison_closure_body(id);
                    }
                    _ => {}
                }
            }
            if let Some((class_ty, _)) = class_value {
                self.class_value_locals.insert(id, vec![class_ty]);
            }
            let fresh_base =
                ClassValueFieldBase::Local(scope.body_id(), self.class_value_storage(id));
            if matches!(init, Some(HirExpr::New { .. })) {
                self.note_fresh_field_base(fresh_base);
            } else {
                self.forget_fresh_field_base(&fresh_base);
            }
            if let Some(HirExpr::Local { id: source, .. }) = &init {
                let storage = self.class_value_storage(*source);
                if let Some(classes) = self.class_value_locals.get(&storage).cloned() {
                    self.record_loop_class_value_copies(&[(storage, classes.clone())], d.span);
                    self.class_value_locals.insert(id, classes);
                }
                if self.unknown_class_value_locals.contains_key(&storage) {
                    self.unknown_class_value_locals
                        .insert(id, name.as_str().to_owned());
                }
            } else if let Some(HirExpr::Global { name: source, .. }) = &init {
                if let Some(storage) = self.captured_local_id(source.as_str()) {
                    if let Some(classes) = self.captured_class_values(source.as_str(), d.span) {
                        self.class_value_locals.insert(id, classes);
                    }
                    if self.unknown_class_value_locals.contains_key(&storage) {
                        self.unknown_class_value_locals
                            .insert(id, name.as_str().to_owned());
                    }
                } else if let Some(classes) = self.class_value_globals.get(source.as_str()).cloned()
                {
                    self.record_class_value_global_copy(&[source.as_str()], d.span);
                    self.class_value_locals.insert(id, classes);
                }
            }
            match (&v.kind, init.as_ref()) {
                (VariableDeclarationKind::Const, Some(init_expr)) if class_value.is_none() => {
                    let facts = self.class_value_poison_facts(scope.body_id(), init_expr);
                    self.class_value_poisoned_locals
                        .insert((scope.body_id(), id), facts);
                }
                _ => {
                    self.class_value_poisoned_locals
                        .remove(&(scope.body_id(), id));
                }
            }
            if class_value.is_none()
                && matches!(
                    v.kind,
                    VariableDeclarationKind::Const | VariableDeclarationKind::Let
                )
                && let Some(HirExpr::Int(value, _)) = init.as_ref()
            {
                self.constant_int_locals.insert(id, *value);
            } else {
                self.constant_int_locals.remove(&id);
            }
            let copy_back = match (simple_binding, init.as_ref()) {
                (true, Some(init)) => self.finish_statement_expression(init, senv),
                _ => None,
            };
            out.push(HirStmt::Let {
                id,
                name: Atom::from(name.as_str()),
                ty,
                init,
            });
            if let Some(stmt) = copy_back {
                out.push(stmt);
            }
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
        self.widen_before_loop(scope, &Self::for_effects(f));
        self.enter_class_value_branch();
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
        self.leave_class_value_branch();
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
            let ty = if d.type_annotation.is_some() {
                self.resolve_ts_type_from_annotation(senv, d.type_annotation.as_deref())
            } else {
                senv.error_ty()
            };
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
impl SkeletonBuilder {
    fn narrow_after_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        ast: &Expression<'_>,
        walked: &mut HirExpr,
        scope: &mut BodyScope,
    ) -> Option<HirStmt> {
        let Expression::AssignmentExpression(assign) = ast else {
            return None;
        };
        let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(ident)) =
            assign.left.as_simple_assignment_target()
        else {
            return None;
        };
        let name = ident.name.as_str().to_owned();
        if let HirExpr::CompoundUpdate { op, .. } = walked
            && !matches!(
                op,
                HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
            )
        {
            return None;
        }
        if self.suppress_narrowing > 0 {
            return None;
        }
        match walked {
            HirExpr::Assignment { .. } => {}
            HirExpr::CompoundUpdate { op, .. } if !matches!(op, HirBinaryOp::And) => {}
            _ => return None,
        }
        let current = scope.type_of_visible(&name)?;
        let inner = non_nullish_part(current, senv.types)?;
        let (storage_id, storage_ty) = assignment_target_local(walked)?;
        let can_narrow = {
            let walked: &HirExpr = walked;
            let assigned = match walked {
                HirExpr::Assignment { value, .. } => value.as_ref(),
                HirExpr::CompoundUpdate { rhs, .. } => rhs.as_ref(),
                _ => return None,
            };
            assigned_is_non_nullish(assigned, senv.types)
                && assignment_can_narrow(inner, assigned, senv.types)
        };
        if !can_narrow {
            scope.widen(&name);
            return None;
        }
        let span = core_span_from_oxc(assign.span);
        let id = LocalId::from_raw(scope.peek_next());
        scope.set_next(id.raw().saturating_add(1));
        let init = HirExpr::TypeAssertion {
            expr: Box::new(HirExpr::Local {
                id: storage_id,
                ty: storage_ty,
                span,
            }),
            target: inner,
            span,
        };
        scope.narrow(&name, id, inner);
        if let Some(frame) = self.narrowed_stack.last_mut() {
            frame.insert(id, (storage_id, storage_ty));
        }
        let visible = scope.names();
        let mut uid = senv.next_destructured_id.get();
        let mut binding_name = Atom::from(format!("_narrowed_{uid}"));
        while self.declared_name_is_taken(binding_name.as_str())
            || visible
                .iter()
                .any(|visible| visible == binding_name.as_str())
        {
            uid += 1;
            binding_name = Atom::from(format!("_narrowed_{uid}"));
        }
        senv.next_destructured_id.set(uid + 1);
        Some(HirStmt::Let {
            id,
            name: binding_name,
            ty: inner,
            init: Some(init),
        })
    }

    fn statement_narrowed_update(
        &self,
        walked: &HirExpr,
    ) -> Option<(LocalId, TypeId, LocalId, TypeId, Span)> {
        let HirExpr::CompoundUpdate {
            op, target, span, ..
        } = walked
        else {
            return None;
        };
        if matches!(
            op,
            HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
        ) {
            return None;
        }
        let HirExpr::Local { id, ty, .. } = target.as_ref() else {
            return None;
        };
        let (storage_id, storage_ty) = self.narrowed_storage(*id)?;
        Some((*id, *ty, storage_id, storage_ty, *span))
    }

    fn finish_statement_expression(
        &mut self,
        walked: &HirExpr,
        senv: &mut SkeletonEnv,
    ) -> Option<HirStmt> {
        let root = self.statement_narrowed_update(walked);
        let root_key = root.map(|(narrowed_id, _, _, _, span)| (narrowed_id, span));
        let mut claimed = false;
        for update in std::mem::take(&mut self.pending_narrowed_updates) {
            let is_root = !claimed
                && root_key.is_some_and(|(narrowed_id, span)| {
                    narrowed_id == update.narrowed_id && span == update.span
                });
            if is_root {
                claimed = true;
                continue;
            }
            senv.report_narrowed_update(update.span);
        }
        let (narrowed_id, narrowed_ty, storage_id, storage_ty, span) = root?;
        Some(HirStmt::Expr {
            expr: HirExpr::Assignment {
                target: Box::new(HirExpr::Local {
                    id: storage_id,
                    ty: storage_ty,
                    span,
                }),
                value: Box::new(HirExpr::Local {
                    id: narrowed_id,
                    ty: narrowed_ty,
                    span,
                }),
                ty: storage_ty,
                span,
            },
        })
    }
}

pub(super) fn assignment_target_local(walked: &HirExpr) -> Option<(LocalId, TypeId)> {
    let (HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. }) = walked
    else {
        return None;
    };
    match target.as_ref() {
        HirExpr::Local { id, ty, .. } => Some((*id, *ty)),
        _ => None,
    }
}

fn non_nullish_part(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty) {
        Some(Type::Optional { inner, .. }) => match types.resolve(*inner) {
            None | Some(Type::Error) => None,
            Some(_) => Some(*inner),
        },
        Some(Type::Union { variants })
            if variants
                .iter()
                .any(|v| matches!(types.resolve(*v), Some(Type::Null | Type::Void))) =>
        {
            None
        }
        None | Some(Type::Error | Type::Null | Type::Void) => None,
        Some(_) => Some(ty),
    }
}

fn assigned_is_non_nullish(assigned: &HirExpr, types: &TypeTable) -> bool {
    match assigned {
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_) => false,
        HirExpr::Int(..) | HirExpr::Float(..) | HirExpr::String(..) | HirExpr::Bool(..) => true,
        other => non_nullish_part(other.ty(), types).is_some(),
    }
}

fn assignment_can_narrow(inner: TypeId, assigned: &HirExpr, types: &TypeTable) -> bool {
    match assigned {
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_) => false,
        HirExpr::Int(..) | HirExpr::Float(..) => is_numeric_ty(inner, types),
        HirExpr::String(..) => matches!(types.resolve(inner), Some(Type::String)),
        HirExpr::Bool(..) => matches!(types.resolve(inner), Some(Type::Bool)),
        other => {
            let assigned_ty = other.ty();
            if types.resolve(inner) == types.resolve(assigned_ty) {
                return true;
            }
            is_numeric_ty(inner, types) && is_numeric_ty(assigned_ty, types)
        }
    }
}

fn is_numeric_ty(ty: TypeId, types: &TypeTable) -> bool {
    matches!(
        types.resolve(ty),
        Some(
            Type::F32
                | Type::F64
                | Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
        )
    )
}

enum SwitchStart {
    At(usize),
    NoCase,
    Unknown,
}

fn constant_switch_start(
    disc: &HirExpr,
    known: Option<i64>,
    cases: &[HirSwitchCase],
) -> SwitchStart {
    let matches_test = |case: &HirSwitchCase| match (disc, case.test.as_ref()) {
        (HirExpr::Int(value, _), Some(HirExpr::Int(test, _))) => value == test,
        (HirExpr::String(value, _), Some(HirExpr::String(test, _))) => value == test,
        _ => false,
    };
    if matches!(disc, HirExpr::Int(..) | HirExpr::String(..)) {
        if let Some(index) = cases.iter().position(matches_test) {
            return SwitchStart::At(index);
        }
        return default_switch_start(cases);
    }
    let Some(known) = known else {
        return SwitchStart::Unknown;
    };
    match cases
        .iter()
        .position(|case| matches!(&case.test, Some(HirExpr::Int(test, _)) if *test == known))
    {
        Some(index) => SwitchStart::At(index),
        None => default_switch_start(cases),
    }
}

fn default_switch_start(cases: &[HirSwitchCase]) -> SwitchStart {
    cases
        .iter()
        .position(|case| case.test.is_none())
        .map_or(SwitchStart::NoCase, SwitchStart::At)
}

fn hir_case_ends_early(stmts: &[HirStmt]) -> bool {
    match stmts.last() {
        Some(
            HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Return { .. }
            | HirStmt::Throw { .. },
        ) => true,
        Some(HirStmt::Block(inner)) => hir_case_ends_early(inner),
        _ => false,
    }
}

fn check_switch_case_tdz(
    senv: &mut SkeletonEnv<'_>,
    disc: &HirExpr,
    known: Option<i64>,
    cases: &[HirSwitchCase],
    events: &[Vec<SwitchCaseEvent>],
    span: oxc_span::Span,
) {
    let mut declared: Vec<(LocalId, Atom, Option<LocalId>)> = Vec::new();
    let mut var_declared: Vec<(LocalId, Atom)> = Vec::new();
    for case_events in events {
        for SwitchCaseEvent {
            local,
            kind,
            name,
            shadowed,
        } in case_events
        {
            let Some(name) = name else {
                continue;
            };
            match kind {
                SwitchCaseEventKind::Decl => {
                    if !declared.iter().any(|(other, _, _)| other == local) {
                        declared.push((*local, name.clone(), *shadowed));
                    }
                }
                SwitchCaseEventKind::VarDecl => {
                    if !var_declared.iter().any(|(other, _)| other == local) {
                        var_declared.push((*local, name.clone()));
                    }
                }
                SwitchCaseEventKind::Use => {}
            }
        }
    }
    if declared.is_empty() && var_declared.is_empty() {
        return;
    }
    let starts: Vec<usize> = match constant_switch_start(disc, known, cases) {
        SwitchStart::At(index) => vec![index],
        SwitchStart::NoCase => Vec::new(),
        SwitchStart::Unknown => (0..cases.len()).collect(),
    };
    let span = core_span_from_oxc(span);
    for (id, name, shadowed) in declared {
        let mut reported = false;
        for &start in &starts {
            let mut initialized = false;
            for (index, case) in cases.iter().enumerate().skip(start) {
                if let Some(case_events) = events.get(index) {
                    for SwitchCaseEvent { local, kind, .. } in case_events {
                        let same_local = *local == id;
                        let shadow_use =
                            !same_local && shadowed.is_some_and(|outer| outer == *local);
                        if !same_local && !shadow_use {
                            continue;
                        }
                        if *kind == SwitchCaseEventKind::Decl {
                            if same_local {
                                initialized = true;
                            }
                        } else if *kind == SwitchCaseEventKind::Use && !initialized {
                            senv.report_unsupported(
                                "E0419",
                                format!(
                                    "`{name}` is declared in a `switch` case whose declaration \
                                     may not run before this use; that is a temporal-dead-zone \
                                     error at run time, so declare it before the `switch`"
                                )
                                .as_str(),
                                span,
                            );
                            reported = true;
                            break;
                        }
                    }
                }
                if reported || hir_case_ends_early(&case.body) {
                    break;
                }
            }
            if reported {
                break;
            }
        }
    }
    for (id, name) in var_declared {
        let Some(ty) = case_local_type(cases, id) else {
            continue;
        };
        if switch_case_local_is_hoistable(ty, senv.types) {
            continue;
        }
        let mut reported = false;
        for &start in &starts {
            let mut initialized = false;
            for (index, case) in cases.iter().enumerate().skip(start) {
                if let Some(case_events) = events.get(index) {
                    for SwitchCaseEvent { local, kind, .. } in case_events {
                        if *local != id {
                            continue;
                        }
                        match kind {
                            SwitchCaseEventKind::VarDecl => initialized = true,
                            SwitchCaseEventKind::Use if !initialized => {
                                senv.report_unsupported(
                                    "E0419",
                                    format!(
                                        "`{name}` is declared with `var` in a `switch` case \
                                         whose declaration may not run before this use, and its \
                                         type has no default value, so declare it before the \
                                         `switch`"
                                    )
                                    .as_str(),
                                    span,
                                );
                                reported = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                if reported || hir_case_ends_early(&case.body) {
                    break;
                }
            }
            if reported {
                break;
            }
        }
    }
}

fn case_local_type(cases: &[HirSwitchCase], id: LocalId) -> Option<TypeId> {
    cases.iter().find_map(|case| {
        case.body.iter().find_map(|stmt| match stmt {
            HirStmt::Let { id: local, ty, .. } if *local == id => Some(*ty),
            _ => None,
        })
    })
}

fn switch_case_local_is_hoistable(ty: TypeId, types: &TypeTable) -> bool {
    match types.resolve(ty) {
        Some(Type::String | Type::Array { .. } | Type::Fn { .. }) => false,
        Some(Type::Struct { .. } | Type::Named { .. }) => false,
        Some(
            Type::Optional { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner }
            | Type::Generator { inner, .. }
            | Type::Promise { ok: inner, .. }
            | Type::WeakMap { value: inner, .. },
        ) => switch_case_local_is_hoistable(*inner, types),
        Some(Type::Result { ok, err }) => {
            switch_case_local_is_hoistable(*ok, types)
                && switch_case_local_is_hoistable(*err, types)
        }
        _ => true,
    }
}

fn ends_with_a_terminator(stmts: &[Statement<'_>]) -> bool {
    match stmts.last() {
        Some(
            Statement::BreakStatement(_)
            | Statement::ReturnStatement(_)
            | Statement::ThrowStatement(_)
            | Statement::ContinueStatement(_),
        ) => true,
        Some(Statement::BlockStatement(block)) => ends_with_a_terminator(&block.body),
        _ => false,
    }
}

fn statement_always_terminates(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::BreakStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_always_terminates),
        Statement::IfStatement(i) => i.alternate.as_ref().is_some_and(|alternate| {
            statement_always_terminates(&i.consequent) && statement_always_terminates(alternate)
        }),
        _ => false,
    }
}

fn statement_leaves_case(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::BreakStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_leaves_case),
        Statement::IfStatement(i) => {
            statement_leaves_case(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|alternate| statement_leaves_case(alternate))
        }
        Statement::WhileStatement(w) => statement_contains_return_or_throw(&w.body),
        Statement::DoWhileStatement(d) => statement_contains_return_or_throw(&d.body),
        Statement::ForStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForInStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForOfStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(statement_contains_return_or_throw)),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(statement_leaves_case)
                || t.handler
                    .as_ref()
                    .is_some_and(|handler| handler.body.body.iter().any(statement_leaves_case))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|finalizer| finalizer.body.iter().any(statement_leaves_case))
        }
        Statement::LabeledStatement(l) => statement_leaves_case(&l.body),
        _ => false,
    }
}

fn statement_contains_return_or_throw(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => {
            block.body.iter().any(statement_contains_return_or_throw)
        }
        Statement::IfStatement(i) => {
            statement_contains_return_or_throw(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|alternate| statement_contains_return_or_throw(alternate))
        }
        Statement::WhileStatement(w) => statement_contains_return_or_throw(&w.body),
        Statement::DoWhileStatement(d) => statement_contains_return_or_throw(&d.body),
        Statement::ForStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForInStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForOfStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(statement_contains_return_or_throw)),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(statement_contains_return_or_throw)
                || t.handler.as_ref().is_some_and(|handler| {
                    handler
                        .body
                        .body
                        .iter()
                        .any(statement_contains_return_or_throw)
                })
                || t.finalizer.as_ref().is_some_and(|finalizer| {
                    finalizer
                        .body
                        .iter()
                        .any(statement_contains_return_or_throw)
                })
        }
        Statement::LabeledStatement(l) => statement_contains_return_or_throw(&l.body),
        _ => false,
    }
}

fn reachable_case_prefix(stmts: &[Statement<'_>]) -> usize {
    match stmts.iter().position(statement_always_terminates) {
        Some(index) => index + 1,
        None => stmts.len(),
    }
}

fn case_covers_its_end(stmts: &[Statement<'_>]) -> bool {
    let Some(cut) = stmts.iter().position(statement_always_terminates) else {
        return true;
    };
    cut == 0 || !statement_leaves_case(&stmts[0])
}
