use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::has_potential_side_effects;
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

fn is_bigint_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.resolve(ty), Some(Type::BigInt))
}

fn expr_yields_bigint(expr: &HirExpr, types: &TypeTable) -> bool {
    match expr {
        HirExpr::BigInt { ty, .. } => is_bigint_type(types, *ty),
        HirExpr::Binary { lhs, rhs, .. } => {
            expr_yields_bigint(lhs, types) && expr_yields_bigint(rhs, types)
        }
        HirExpr::Unary { expr, .. } => expr_yields_bigint(expr, types),
        HirExpr::Local { ty, .. } | HirExpr::Global { ty, .. } => is_bigint_type(types, *ty),
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
            ctx.error(
                "P0005",
                format!(
                    "mixed BigInt and non-BigInt operands are not allowed in `{}` (per spec, \
                     BigInt arithmetic rejects mixed types); rhs was {:?}, lhs was {:?}",
                    op_name_for_bigint(op),
                    types.resolve(rhs.ty()),
                    types.resolve(lhs.ty()),
                ),
                lhs.span(),
            );
            let lhs_mir =
                self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
            return lhs_mir;
        }
        match op {
            HirBinaryOp::In => {
                let lhs_mir =
                    self.convert_expr(lhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let rhs_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpIn,
                    args: vec![lhs_mir, rhs_mir],
                    dest: Some(dest),
                    ty,
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
                        ctx.error(
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
