use oxc_ast::ast::{
    Argument, AssignmentExpression, AssignmentTarget, BinaryExpression, CallExpression, Expression,
    LogicalExpression, MemberExpression, SimpleAssignmentTarget, UnaryExpression, UpdateExpression,
};
use oxc_span::GetSpan;
use oxc_syntax::operator::UpdateOperator;
use ts_aot_core::{Atom, FieldId};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr};

use super::ops::{
    CompoundOp, compound_op, map_binary_op, map_logical_op, map_unary_op, number_to_hir,
};
use super::scope::BodyScope;
use crate::frontend::skeleton::SkeletonBuilder;

impl SkeletonBuilder<'_, '_> {
    pub(super) fn walk_expr(&mut self, e: &Expression<'_>, scope: &mut BodyScope) -> HirExpr {
        match e {
            Expression::BooleanLiteral(b) => HirExpr::Bool(b.value),
            Expression::NumberLiteral(n) => number_to_hir(n.value),
            Expression::StringLiteral(s) => HirExpr::String(Atom::from(s.value.as_str())),
            Expression::NullLiteral(_) => HirExpr::Null,
            Expression::Identifier(id) => self.ident_to_expr(id.name.as_str(), scope),
            Expression::ThisExpression(_) => {
                if let Some((id, ty)) = scope.lookup("this") {
                    HirExpr::Local { id, ty }
                } else {
                    let ty = self.error_ty();
                    HirExpr::Global {
                        name: Atom::from("this"),
                        ty,
                    }
                }
            }
            Expression::ParenthesizedExpression(p) => self.walk_expr(&p.expression, scope),
            Expression::BinaryExpression(b) => self.walk_binary(b, scope),
            Expression::LogicalExpression(l) => self.walk_logical(l, scope),
            Expression::UnaryExpression(unary) => self.walk_unary(unary, scope),
            Expression::UpdateExpression(update) => self.walk_update(update, scope),
            Expression::CallExpression(call) => self.walk_call(call, scope),
            Expression::MemberExpression(m) => self.walk_member(m, scope),
            Expression::AssignmentExpression(a) => self.walk_assignment(a, scope),
            Expression::AwaitExpression(a) => {
                let inner = self.walk_expr(&a.argument, scope);
                let ty = self.error_ty();
                HirExpr::Await {
                    expr: Box::new(inner),
                    ty,
                }
            }
            other => {
                self.report_unwalked(
                    "expression form is not supported by the body walker",
                    other.span(),
                );
                HirExpr::Unit
            }
        }
    }

    fn ident_to_expr(&mut self, name: &str, scope: &BodyScope) -> HirExpr {
        if name == "undefined" {
            return HirExpr::Undefined;
        }
        if let Some((id, ty)) = scope.lookup(name) {
            HirExpr::Local { id, ty }
        } else {
            let ty = self.error_ty();
            HirExpr::Global {
                name: Atom::from(name),
                ty,
            }
        }
    }

    fn walk_binary(&mut self, b: &BinaryExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let lhs = self.walk_expr(&b.left, scope);
        let rhs = self.walk_expr(&b.right, scope);
        let ty = self.error_ty();
        if let Some(op) = map_binary_op(b.operator) {
            HirExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty,
            }
        } else {
            self.report_unwalked(
                "binary operator is not supported by the body walker",
                b.span,
            );
            HirExpr::Unit
        }
    }

    fn walk_logical(&mut self, l: &LogicalExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let lhs = self.walk_expr(&l.left, scope);
        let rhs = self.walk_expr(&l.right, scope);
        let ty = self.error_ty();
        HirExpr::Binary {
            op: map_logical_op(l.operator),
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
        }
    }

    fn walk_unary(&mut self, unary: &UnaryExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let inner = self.walk_expr(&unary.argument, scope);
        match map_unary_op(unary.operator) {
            Some(op) => {
                let ty = self.error_ty();
                HirExpr::Unary {
                    op,
                    expr: Box::new(inner),
                    ty,
                }
            }
            None => inner,
        }
    }

    fn walk_update(&mut self, update: &UpdateExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let target = self.walk_simple_target(&update.argument, scope);
        let op = match update.operator {
            UpdateOperator::Increment => HirBinaryOp::Add,
            UpdateOperator::Decrement => HirBinaryOp::Sub,
        };
        let ty = self.error_ty();
        HirExpr::CompoundUpdate {
            target: Box::new(target),
            op,
            rhs: Box::new(HirExpr::Int(1)),
            post: !update.prefix,
            ty,
        }
    }

    fn walk_call(&mut self, call: &CallExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let callee_expr = self.walk_expr(&call.callee, scope);
        let mut args = Vec::with_capacity(call.arguments.len());
        for arg in &call.arguments {
            match arg {
                Argument::Expression(e) => args.push(self.walk_expr(e, scope)),
                Argument::SpreadElement(s) => {
                    self.report_unwalked("spread argument is not supported", s.span);
                }
            }
        }
        let ty = self.error_ty();
        HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(callee_expr)),
            args,
            ty,
        }
    }

    fn walk_member(&mut self, m: &MemberExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        match m {
            MemberExpression::StaticMemberExpression(s) => {
                let owner = self.walk_expr(&s.object, scope);
                let ty = self.error_ty();
                HirExpr::Field {
                    owner: Box::new(owner),
                    field: FieldId::from_raw(0),
                    field_name: Atom::from(s.property.name.as_str()),
                    ty,
                }
            }
            MemberExpression::ComputedMemberExpression(computed) => {
                let owner = self.walk_expr(&computed.object, scope);
                let index = self.walk_expr(&computed.expression, scope);
                let ty = self.error_ty();
                HirExpr::Index {
                    owner: Box::new(owner),
                    index: Box::new(index),
                    ty,
                }
            }
            MemberExpression::PrivateFieldExpression(p) => {
                self.report_unwalked("private field access is not supported", p.span);
                HirExpr::Unit
            }
        }
    }

    fn walk_assignment(&mut self, a: &AssignmentExpression<'_>, scope: &mut BodyScope) -> HirExpr {
        let target = self.walk_assign_target(&a.left, scope);
        let rhs = self.walk_expr(&a.right, scope);
        let ty = self.error_ty();
        match compound_op(a.operator) {
            CompoundOp::Assign => HirExpr::Assignment {
                target: Box::new(target),
                value: Box::new(rhs),
                ty,
            },
            CompoundOp::Binary(op) => HirExpr::CompoundUpdate {
                target: Box::new(target),
                op,
                rhs: Box::new(rhs),
                post: false,
                ty,
            },
            CompoundOp::Unsupported => {
                self.report_unwalked("assignment operator is not supported", a.span);
                HirExpr::Unit
            }
        }
    }

    fn walk_assign_target(&mut self, t: &AssignmentTarget<'_>, scope: &mut BodyScope) -> HirExpr {
        match t {
            AssignmentTarget::SimpleAssignmentTarget(s) => self.walk_simple_target(s, scope),
            AssignmentTarget::AssignmentTargetPattern(p) => {
                self.report_unwalked("destructuring assignment target is not supported", p.span());
                HirExpr::Unit
            }
        }
    }

    fn walk_simple_target(
        &mut self,
        s: &SimpleAssignmentTarget<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        match s {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.ident_to_expr(id.name.as_str(), scope)
            }
            SimpleAssignmentTarget::MemberAssignmentTarget(m) => self.walk_member(m, scope),
            SimpleAssignmentTarget::TSAsExpression(_)
            | SimpleAssignmentTarget::TSSatisfiesExpression(_)
            | SimpleAssignmentTarget::TSNonNullExpression(_)
            | SimpleAssignmentTarget::TSTypeAssertion(_) => match s.get_expression() {
                Some(inner) => self.walk_expr(inner, scope),
                None => HirExpr::Unit,
            },
        }
    }
}
