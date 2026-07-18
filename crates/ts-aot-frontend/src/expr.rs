use oxc_ast::ast::{
    Argument, AssignmentExpression, AssignmentTarget, BinaryExpression, CallExpression, Expression,
    LogicalExpression, SimpleAssignmentTarget, TaggedTemplateExpression, TemplateLiteral,
    UnaryExpression, UpdateExpression, match_assignment_target, match_assignment_target_pattern,
    match_expression, match_member_expression,
};
use oxc_span::GetSpan;
use oxc_syntax::operator::UpdateOperator;
use ts_aot_core::{Atom, FieldId};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr};

use crate::ops::{
    CompoundOp, compound_op, map_binary_op, map_logical_op, map_unary_op, number_to_hir,
};
use crate::scope::BodyScope;
use crate::skeleton::SkeletonBuilder;

impl SkeletonBuilder<'_, '_> {
    pub(crate) fn walk_expr(&mut self, e: &Expression<'_>, scope: &mut BodyScope) -> HirExpr {
        match e {
            Expression::BooleanLiteral(b) => HirExpr::Bool(b.value),
            Expression::NumericLiteral(n) => number_to_hir(n.value),
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
            other @ match_member_expression!(Expression) => {
                self.walk_member(other.to_member_expression(), scope)
            }
            Expression::AssignmentExpression(a) => self.walk_assignment(a, scope),
            Expression::AwaitExpression(a) => {
                let inner = self.walk_expr(&a.argument, scope);
                let ty = self.error_ty();
                HirExpr::Await {
                    expr: Box::new(inner),
                    ty,
                }
            }
            Expression::TemplateLiteral(t) => self.walk_template_literal(t, scope),
            Expression::TaggedTemplateExpression(t) => {
                self.walk_tagged_template_expression(t, scope)
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

    fn walk_template_parts(
        &mut self,
        quasis: &[oxc_ast::ast::TemplateElement<'_>],
        expressions: &[Expression<'_>],
        scope: &mut BodyScope,
    ) -> (Vec<HirExpr>, Vec<Option<Atom>>, Vec<Option<Atom>>) {
        let mut exprs = Vec::with_capacity(expressions.len());
        let mut cooked_parts = Vec::with_capacity(quasis.len());
        let mut raw_parts = Vec::with_capacity(quasis.len());
        for (i, q) in quasis.iter().enumerate() {
            let cooked = q.value.cooked.as_ref().map(|s| Atom::from(s.as_str()));
            let raw = Some(Atom::from(q.value.raw.as_str()));
            cooked_parts.push(cooked);
            raw_parts.push(raw);
            if i < expressions.len() {
                exprs.push(self.walk_expr(&expressions[i], scope));
            }
        }
        (exprs, cooked_parts, raw_parts)
    }

    fn walk_template_literal(&mut self, t: &TemplateLiteral<'_>, scope: &mut BodyScope) -> HirExpr {
        let (expressions, cooked_parts, raw_parts) =
            self.walk_template_parts(&t.quasis, &t.expressions, scope);
        let ty = self.error_ty();
        HirExpr::Template {
            tag: None,
            expressions,
            cooked_parts,
            raw_parts,
            ty,
        }
    }

    fn walk_tagged_template_expression(
        &mut self,
        t: &TaggedTemplateExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let tag = self.walk_expr(&t.tag, scope);
        let (expressions, cooked_parts, raw_parts) =
            self.walk_template_parts(&t.quasi.quasis, &t.quasi.expressions, scope);
        let ty = self.error_ty();
        HirExpr::Template {
            tag: Some(Box::new(tag)),
            expressions,
            cooked_parts,
            raw_parts,
            ty,
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
                arg @ match_expression!(Argument) => {
                    args.push(self.walk_expr(arg.to_expression(), scope));
                }
                _ => {
                    self.report_unwalked("spread argument is not supported", arg.span());
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

    fn walk_member(
        &mut self,
        m: &oxc_ast::ast::MemberExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::MemberExpression as ME;
        match m {
            ME::StaticMemberExpression(s) => {
                let owner = self.walk_expr(&s.object, scope);
                let ty = self.error_ty();
                HirExpr::Field {
                    owner: Box::new(owner),
                    field: FieldId::from_raw(0),
                    field_name: Atom::from(s.property.name.as_str()),
                    ty,
                }
            }
            ME::ComputedMemberExpression(computed) => {
                let owner = self.walk_expr(&computed.object, scope);
                let index = self.walk_expr(&computed.expression, scope);
                let ty = self.error_ty();
                HirExpr::Index {
                    owner: Box::new(owner),
                    index: Box::new(index),
                    ty,
                }
            }
            ME::PrivateFieldExpression(p) => {
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

    fn walk_assign_target(
        &mut self,
        t: &oxc_ast::ast::AssignmentTarget<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        match t {
            t @ match_assignment_target!(AssignmentTarget) => {
                self.walk_simple_target(t.to_simple_assignment_target(), scope)
            }
            t @ match_assignment_target_pattern!(AssignmentTarget) => {
                self.report_unwalked("destructuring assignment target is not supported", t.span());
                HirExpr::Unit
            }
        }
    }

    fn walk_simple_target(
        &mut self,
        s: &oxc_ast::ast::SimpleAssignmentTarget<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::SimpleAssignmentTarget as SAT;
        match s {
            SAT::AssignmentTargetIdentifier(id) => self.ident_to_expr(id.name.as_str(), scope),
            m @ match_member_expression!(SimpleAssignmentTarget) => {
                self.walk_member(m.to_member_expression(), scope)
            }
            _ => match s.get_expression() {
                Some(inner) => self.walk_expr(inner, scope),
                None => HirExpr::Unit,
            },
        }
    }
}
