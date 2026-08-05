use std::collections::HashMap;

use ts_aot_core::{Span, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::has_potential_side_effects;
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

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
}
