use ts_aot_core::Span;
use ts_aot_ir_hir::{HirCallee, HirExpr, ObjectLiteralField};

use super::ClosureLowerer;

impl ClosureLowerer<'_> {
    pub(super) fn walk_expr(&mut self, expr: &mut HirExpr) {
        if let HirExpr::Closure {
            id,
            params,
            body,
            captures,
            ty,
            span,
            ..
        } = expr
        {
            if !captures.is_empty() {
                self.ctx.warning(
                    "P0007",
                    "capturing closures are not yet supported by lower_closures; \
                     closure is left intact and will fail downstream in HIR→MIR",
                    Span::new(0, 0),
                );
                self.stats.deferred_capturing += 1;
                return;
            }

            *expr = self.lift_non_capturing_closure(*id, params, body, *ty, *span);
            return;
        }

        match expr {
            HirExpr::Call {
                callee, args, span, ..
            } => {
                self.walk_callee(callee, *span);
                for a in args {
                    self.walk_expr(a);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            HirExpr::Unary { expr: e, .. } => self.walk_expr(e),
            HirExpr::Field { owner, .. } => self.walk_expr(owner),
            HirExpr::Index { owner, index, .. } => {
                self.walk_expr(owner);
                self.walk_expr(index);
            }
            HirExpr::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    self.walk_expr(e);
                }
            }
            HirExpr::ArrayLiteral { elements, .. } => {
                for e in elements {
                    self.walk_expr(e);
                }
            }
            HirExpr::ObjectLiteral { fields, .. } => {
                for f in fields {
                    let value = match f {
                        ObjectLiteralField::Property { value, .. } => value,
                        ObjectLiteralField::Spread(value) => value,
                    };
                    self.walk_expr(value);
                }
            }
            HirExpr::Ternary {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(cond);
                self.walk_expr(then_branch);
                self.walk_expr(else_branch);
            }
            HirExpr::Sequence { exprs, .. } => {
                for e in exprs {
                    self.walk_expr(e);
                }
            }
            HirExpr::Await { expr: e, .. } | HirExpr::TypeAssertion { expr: e, .. } => {
                self.walk_expr(e);
            }
            HirExpr::Yield { expr: Some(e), .. } => self.walk_expr(e),
            HirExpr::Template {
                tag, expressions, ..
            } => {
                if let Some(t) = tag {
                    self.walk_expr(t);
                }
                for p in expressions {
                    self.walk_expr(p);
                }
            }
            HirExpr::New { callee, args, .. } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a);
                }
            }
            HirExpr::OptionalChain { base, .. } => self.walk_expr(base),
            HirExpr::Assignment { target, value, .. } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            HirExpr::CompoundUpdate { target, rhs, .. } => {
                self.walk_expr(target);
                self.walk_expr(rhs);
            }
            HirExpr::Unit(_)
            | HirExpr::Bool(_, _)
            | HirExpr::Int(_, _)
            | HirExpr::Float(_, _)
            | HirExpr::String(_, _)
            | HirExpr::Null(_)
            | HirExpr::Undefined(_)
            | HirExpr::Local { .. }
            | HirExpr::Global { .. }
            | HirExpr::Yield { expr: None, .. }
            | HirExpr::RegExp { .. }
            | HirExpr::BigInt { .. } => {}
            HirExpr::Import { source, .. } => self.walk_expr(source),
            HirExpr::Closure { .. } => {}
        }
    }

    pub(super) fn walk_callee(&mut self, callee: &mut HirCallee, span: Span) {
        if matches!(callee, HirCallee::Closure(_)) {
            super::closure_lift::rewrite_closure_callee(
                callee,
                span,
                &self.closure_names,
                self.new_decls,
                self.ctx,
            );
            return;
        }
        if let HirCallee::Indirect(inner) = callee {
            self.walk_expr(inner);
        }
    }
}
