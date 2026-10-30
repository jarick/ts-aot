use std::collections::HashMap;

use ts_aot_core::{Atom, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::{has_potential_side_effects, hir_expr_type_id};
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

fn is_bigint_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.resolve(ty), Some(Type::BigInt))
}

fn expr_yields_bigint(expr: &HirExpr, types: &TypeTable) -> bool {
    if is_bigint_type(types, expr.ty()) {
        return true;
    }
    match expr {
        HirExpr::Binary { lhs, rhs, .. } => {
            expr_yields_bigint(lhs, types) && expr_yields_bigint(rhs, types)
        }
        HirExpr::Unary { expr, .. } => expr_yields_bigint(expr, types),
        HirExpr::Call { args, .. } => args.iter().any(|a| expr_yields_bigint(a, types)),
        _ => false,
    }
}

fn bigint_op_for_binary(op: HirBinaryOp) -> Option<RuntimeOp> {
    match op {
        HirBinaryOp::Add => Some(RuntimeOp::BigIntAdd),
        HirBinaryOp::Sub => Some(RuntimeOp::BigIntSub),
        HirBinaryOp::Mul => Some(RuntimeOp::BigIntMul),
        HirBinaryOp::Div => Some(RuntimeOp::BigIntDiv),
        HirBinaryOp::Mod => Some(RuntimeOp::BigIntRem),
        HirBinaryOp::Eq => Some(RuntimeOp::BigIntEq),
        HirBinaryOp::Ne => Some(RuntimeOp::BigIntNeq),
        HirBinaryOp::Lt => Some(RuntimeOp::BigIntLt),
        HirBinaryOp::Le => Some(RuntimeOp::BigIntLe),
        HirBinaryOp::Gt => Some(RuntimeOp::BigIntGt),
        HirBinaryOp::Ge => Some(RuntimeOp::BigIntGe),
        HirBinaryOp::BitAnd => Some(RuntimeOp::BigIntAnd),
        HirBinaryOp::BitOr => Some(RuntimeOp::BigIntOr),
        HirBinaryOp::BitXor => Some(RuntimeOp::BigIntXor),
        HirBinaryOp::Shl => Some(RuntimeOp::BigIntShl),
        HirBinaryOp::Shr => Some(RuntimeOp::BigIntShr),
        _ => None,
    }
}

fn is_comparison_op(op: HirBinaryOp) -> bool {
    matches!(
        op,
        HirBinaryOp::Eq
            | HirBinaryOp::Ne
            | HirBinaryOp::Lt
            | HirBinaryOp::Le
            | HirBinaryOp::Gt
            | HirBinaryOp::Ge
    )
}

fn op_name_for_bigint(op: HirBinaryOp) -> &'static str {
    match op {
        HirBinaryOp::Add => "+",
        HirBinaryOp::Sub => "-",
        HirBinaryOp::Mul => "*",
        HirBinaryOp::Div => "/",
        HirBinaryOp::Mod => "%",
        HirBinaryOp::Eq => "==",
        HirBinaryOp::Ne => "!=",
        HirBinaryOp::Lt => "<",
        HirBinaryOp::Le => "<=",
        HirBinaryOp::Gt => ">",
        HirBinaryOp::Ge => ">=",
        HirBinaryOp::BitAnd => "&",
        HirBinaryOp::BitOr => "|",
        HirBinaryOp::BitXor => "^",
        HirBinaryOp::Shl => "<<",
        HirBinaryOp::Shr => ">>",
        _ => "?",
    }
}

fn hir_expr_may_reassign(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::Assignment { .. } | HirExpr::CompoundUpdate { .. } => true,
        HirExpr::Field { owner, .. }
        | HirExpr::Unary { expr: owner, .. }
        | HirExpr::Await { expr: owner, .. }
        | HirExpr::TypeAssertion { expr: owner, .. }
        | HirExpr::OptionalChain { base: owner, .. }
        | HirExpr::Yield {
            expr: Some(owner), ..
        }
        | HirExpr::Import { source: owner, .. } => hir_expr_may_reassign(owner),
        HirExpr::Index { owner, index, .. } => {
            hir_expr_may_reassign(owner) || hir_expr_may_reassign(index)
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            hir_expr_may_reassign(lhs) || hir_expr_may_reassign(rhs)
        }
        HirExpr::Call { args, .. } => args.iter().any(hir_expr_may_reassign),
        HirExpr::New { callee, args, .. } => {
            hir_expr_may_reassign(callee) || args.iter().any(hir_expr_may_reassign)
        }
        HirExpr::StructLiteral { fields, .. } => {
            fields.iter().any(|(_, v)| hir_expr_may_reassign(v))
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            fields.iter().any(|f| hir_expr_may_reassign(f.value()))
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            hir_expr_may_reassign(cond)
                || hir_expr_may_reassign(then_branch)
                || hir_expr_may_reassign(else_branch)
        }
        HirExpr::ArrayLiteral { elements, .. }
        | HirExpr::Sequence {
            exprs: elements, ..
        } => elements.iter().any(hir_expr_may_reassign),
        HirExpr::Template { expressions, .. } => expressions.iter().any(hir_expr_may_reassign),
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::Closure { .. }
        | HirExpr::Yield { expr: None, .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => false,
    }
}

fn hir_expr_contains_call(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::Call { .. }
        | HirExpr::New { .. }
        | HirExpr::Await { .. }
        | HirExpr::Yield { expr: Some(_), .. }
        | HirExpr::Template { tag: Some(_), .. } => true,
        HirExpr::Field { owner, .. }
        | HirExpr::Unary { expr: owner, .. }
        | HirExpr::TypeAssertion { expr: owner, .. }
        | HirExpr::OptionalChain { base: owner, .. }
        | HirExpr::Import { source: owner, .. } => hir_expr_contains_call(owner),
        HirExpr::Index { owner, index, .. } => {
            hir_expr_contains_call(owner) || hir_expr_contains_call(index)
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            hir_expr_contains_call(lhs) || hir_expr_contains_call(rhs)
        }
        HirExpr::Assignment { target, value, .. } => {
            hir_expr_contains_call(target) || hir_expr_contains_call(value)
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            hir_expr_contains_call(target) || hir_expr_contains_call(rhs)
        }
        HirExpr::StructLiteral { fields, .. } => {
            fields.iter().any(|(_, v)| hir_expr_contains_call(v))
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            fields.iter().any(|f| hir_expr_contains_call(f.value()))
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            hir_expr_contains_call(cond)
                || hir_expr_contains_call(then_branch)
                || hir_expr_contains_call(else_branch)
        }
        HirExpr::ArrayLiteral { elements, .. }
        | HirExpr::Sequence {
            exprs: elements, ..
        } => elements.iter().any(hir_expr_contains_call),
        HirExpr::Template { expressions, .. } => expressions.iter().any(hir_expr_contains_call),
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_)
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::Closure { .. }
        | HirExpr::Yield { expr: None, .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => false,
    }
}

fn hir_is_place(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Local { .. }
            | HirExpr::Global { .. }
            | HirExpr::Field { .. }
            | HirExpr::Index { .. }
    )
}

fn is_string_operand(expr: &HirExpr, types: &TypeTable) -> bool {
    match expr {
        HirExpr::String(..) | HirExpr::Template { .. } => true,
        HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs,
            rhs,
            ..
        } => is_string_operand(lhs, types) || is_string_operand(rhs, types),
        HirExpr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_string_operand(then_branch, types) && is_string_operand(else_branch, types),
        _ => hir_expr_type_id(expr)
            .and_then(|t| types.resolve(t))
            .is_some_and(|t| matches!(t, Type::String)),
    }
}

impl ExprConverter {
    pub(super) fn convert_binary(
        &mut self,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if op == HirBinaryOp::Add
            && (is_string_operand(lhs, types) || is_string_operand(rhs, types))
        {
            let string_ty = types.intern(&Type::String);
            let lhs_mir = match lhs {
                HirExpr::Null(_) => MirExpr::String {
                    id: Atom::from("null"),
                    ty: string_ty,
                },
                HirExpr::Undefined(_) => MirExpr::String {
                    id: Atom::from("undefined"),
                    ty: string_ty,
                },
                _ => self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx),
            };
            let lhs_is_inline = hir_is_place(lhs) || hir_expr_contains_call(lhs);
            let rhs_may_change_lhs =
                hir_expr_may_reassign(rhs) || (lhs_is_inline && hir_expr_contains_call(rhs));
            let lhs_snapshot_ty = if rhs_may_change_lhs {
                hir_expr_type_id(lhs)
                    .filter(|t| !matches!(types.resolve(*t), None | Some(Type::Error)))
                    .or_else(|| is_string_operand(lhs, types).then_some(string_ty))
            } else {
                None
            };
            let lhs_operand = if let Some(lhs_temp_ty) = lhs_snapshot_ty {
                let lhs_temp = self.fresh_local();
                self.push_temp_local(lhs_temp, lhs_temp_ty);
                out.push(MirStmt::Let {
                    local: lhs_temp,
                    ty: lhs_temp_ty,
                    init: Some(lhs_mir),
                    mutable: false,
                });
                MirExpr::Local(lhs_temp)
            } else {
                lhs_mir
            };
            let rhs_mir = match rhs {
                HirExpr::Null(_) => MirExpr::String {
                    id: Atom::from("null"),
                    ty: string_ty,
                },
                HirExpr::Undefined(_) => MirExpr::String {
                    id: Atom::from("undefined"),
                    ty: string_ty,
                },
                _ => self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx),
            };
            let dest = self.fresh_local();
            self.push_temp_local(dest, string_ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                args: vec![lhs_operand, rhs_mir],
                dest: Some(dest),
                ty: string_ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        if let Some(bigint_op) = bigint_op_for_binary(op)
            && expr_yields_bigint(lhs, types)
            && expr_yields_bigint(rhs, types)
        {
            return self.convert_bigint_binary(
                bigint_op,
                op,
                lhs,
                rhs,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        } else if bigint_op_for_binary(op).is_some()
            && (expr_yields_bigint(lhs, types) || expr_yields_bigint(rhs, types))
        {
            ctx.warning(
                "P0005",
                format!(
                    "mixed BigInt and non-BigInt operands in `{}`; the operation is lowered \
                     using the left operand as-is; rhs was {:?}, lhs was {:?}",
                    op_name_for_bigint(op),
                    types.resolve(rhs.ty()),
                    types.resolve(lhs.ty()),
                ),
                lhs.span(),
            );
            let lhs_mir =
                self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
            let lhs_temp_ty = hir_expr_type_id(lhs).unwrap_or(ty);
            let lhs_temp = self.fresh_local();
            self.push_temp_local(lhs_temp, lhs_temp_ty);
            out.push(MirStmt::Let {
                local: lhs_temp,
                ty: lhs_temp_ty,
                init: Some(lhs_mir),
                mutable: false,
            });
            let rhs_mir =
                self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
            if has_potential_side_effects(&rhs_mir) {
                out.push(MirStmt::Expr(rhs_mir));
            }
            return MirExpr::Local(lhs_temp);
        }
        match op {
            HirBinaryOp::In => {
                let lhs_mir =
                    self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let rhs_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let bool_ty = types.intern(&Type::Bool);
                let dest = self.fresh_local();
                self.push_temp_local(dest, bool_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpIn,
                    args: vec![lhs_mir, rhs_mir],
                    dest: Some(dest),
                    ty: bool_ty,
                    target_ty: None,
                });
                MirExpr::Local(dest)
            }
            HirBinaryOp::InstanceOf => {
                let value_mir =
                    self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let target_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let target_type_id: u32 = match rhs {
                    HirExpr::Global { ty, .. } => {
                        shared_struct_ids.get(ty).map(|sid| sid.raw()).unwrap_or(0)
                    }
                    _ => {
                        ctx.warning(
                            "P0005",
                            "instanceof rhs must be a class reference (HirExpr::Global); \
                             dynamic constructor expressions like getConstructor() are not \
                             yet supported (PR 1.6: identity of non-Global rhs cannot be \
                             resolved at convert time). rhs is still evaluated and its side \
                             effects preserved; runtime returns false.",
                            Span::new(0, 0),
                        );
                        0
                    }
                };
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpInstanceof,
                    args: vec![
                        value_mir,
                        target_mir,
                        MirExpr::Int {
                            value: target_type_id as i128,
                            ty: TypeId::from_raw(0),
                        },
                    ],
                    dest: Some(dest),
                    ty,
                    target_ty: None,
                });
                MirExpr::Local(dest)
            }
            _ => MirExpr::Binary {
                op: convert_binop(op, ctx),
                left: Box::new(self.convert_expr(
                    lhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                right: Box::new(self.convert_expr(
                    rhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                ty,
            },
        }
    }

    fn convert_bigint_binary(
        &mut self,
        bigint_op: RuntimeOp,
        op: HirBinaryOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        _ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let lhs_mir =
            self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
        let rhs_mir =
            self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
        let result_ty = if is_comparison_op(op) {
            types.intern(&Type::Bool)
        } else {
            types.intern(&Type::BigInt)
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op: bigint_op,
            args: vec![lhs_mir, rhs_mir],
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }

    pub(super) fn convert_unary(
        &mut self,
        op: HirUnaryOp,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if expr_yields_bigint(expr, types) {
            match op {
                HirUnaryOp::Neg => {
                    return self.convert_bigint_unary(
                        RuntimeOp::BigIntNeg,
                        expr,
                        ty,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                }
                HirUnaryOp::BitNot => {
                    return self.convert_bigint_unary(
                        RuntimeOp::BigIntNot,
                        expr,
                        ty,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                }
                _ => {}
            }
        }
        match op {
            HirUnaryOp::TypeOf => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                let string_ty = types.intern(&ts_aot_core::Type::String);
                MirExpr::TypeOf {
                    expr: Box::new(inner),
                    ty: string_ty,
                }
            }
            HirUnaryOp::Void => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                if has_potential_side_effects(&inner) {
                    out.push(MirStmt::Expr(inner));
                }
                MirExpr::Unit
            }
            HirUnaryOp::Delete => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                if has_potential_side_effects(&inner) {
                    out.push(MirStmt::Expr(inner));
                }
                MirExpr::Bool(true)
            }
            _ => MirExpr::Unary {
                op: convert_unaryop(op, ctx),
                expr: Box::new(self.convert_expr(
                    expr,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                ty,
            },
        }
    }

    fn convert_bigint_unary(
        &mut self,
        bigint_op: RuntimeOp,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner_mir =
            self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: bigint_op,
            args: vec![inner_mir],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }
}
