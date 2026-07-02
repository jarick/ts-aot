use std::collections::HashMap;

use ts2zig_core::{LocalId, Span, StructId, TypeId};
use ts2zig_ir_hir::HirExpr;
use ts2zig_ir_mir::{MirExpr, MirPlace, MirPlaceBase, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

impl ExprConverter {
    pub(super) fn convert_expr(
        &mut self,
        e: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        ctx: &mut PassContext,
    ) -> MirExpr {
        match e {
            HirExpr::Unit => MirExpr::Unit,
            HirExpr::Bool(b) => MirExpr::Bool(*b),
            HirExpr::Int(v) => MirExpr::Int {
                value: i128::from(*v),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Float(bits) => MirExpr::Float {
                value: f64::from_bits(*bits),
                ty: TypeId::from_raw(0),
            },
            HirExpr::String(id) => MirExpr::String {
                id: id.clone(),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Null => MirExpr::Null {
                ty: TypeId::from_raw(0),
            },
            HirExpr::Undefined => MirExpr::Unit,
            HirExpr::Local { id, .. } => self.map_local(*id),
            HirExpr::Global { name, .. } => MirExpr::Global(name.clone()),
            HirExpr::Field {
                owner, field, ty, ..
            } => MirExpr::Field {
                base: Box::new(self.convert_expr(
                    owner,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                field: *field,
                ty: *ty,
            },
            HirExpr::Index {
                owner, index, ty, ..
            } => MirExpr::Index {
                base: Box::new(self.convert_expr(
                    owner,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                index: Box::new(self.convert_expr(
                    index,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                ty: *ty,
            },
            HirExpr::Call { callee, args, ty } => {
                let callee_id = self.resolve_callee(callee, ctx);
                let mir_args: Vec<MirExpr> = args
                    .iter()
                    .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, ctx))
                    .collect();
                MirExpr::Call {
                    callee: callee_id,
                    args: mir_args,
                    ty: *ty,
                }
            }
            HirExpr::Binary { op, lhs, rhs, ty } => MirExpr::Binary {
                op: convert_binop(*op, ctx),
                left: Box::new(self.convert_expr(
                    lhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                right: Box::new(self.convert_expr(
                    rhs,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                ty: *ty,
            },
            HirExpr::Unary { op, expr, ty } => MirExpr::Unary {
                op: convert_unaryop(*op, ctx),
                expr: Box::new(self.convert_expr(
                    expr,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    ctx,
                )),
                ty: *ty,
            },
            HirExpr::StructLiteral { ty, fields } => {
                let struct_id =
                    self.lookup_or_alloc_struct_id(*ty, shared_struct_ids, shared_next_struct);
                MirExpr::StructLiteral {
                    struct_id,
                    fields: fields
                        .iter()
                        .map(|(fid, e)| {
                            (
                                *fid,
                                self.convert_expr(
                                    e,
                                    out,
                                    shared_struct_ids,
                                    shared_next_struct,
                                    ctx,
                                ),
                            )
                        })
                        .collect(),
                    ty: *ty,
                }
            }
            HirExpr::ArrayLiteral { elements, ty } => {
                let args: Vec<MirExpr> = elements
                    .iter()
                    .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, ctx))
                    .collect();
                let dest = self.fresh_local();
                self.push_temp_local(dest, *ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayCreate,
                    args,
                    dest: Some(dest),
                    ty: *ty,
                });
                MirExpr::Local(dest)
            }
            HirExpr::Closure { ty, .. } => {
                ctx.error(
                    "P0005",
                    "closure expressions are not yet supported in HIR→MIR",
                    Span::new(0, 0),
                );
                let _ = ty;
                MirExpr::Unit
            }
            HirExpr::Await { expr, ty } => {
                let (dest, next_state) = self.push_await();
                self.push_temp_local(dest, *ty);
                let promise =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, ctx);
                out.push(MirStmt::Await {
                    promise,
                    dest,
                    next_state,
                    ty: *ty,
                });
                MirExpr::Local(dest)
            }
            HirExpr::Yield { expr, ty } => {
                let dest = self.fresh_local();
                self.push_temp_local(dest, *ty);
                let value_expr = expr
                    .as_ref()
                    .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, ctx))
                    .unwrap_or(MirExpr::Unit);
                out.push(MirStmt::Let {
                    local: dest,
                    ty: *ty,
                    init: None,
                    mutable: true,
                });
                out.push(MirStmt::Assign {
                    target: MirPlace::Local { id: dest },
                    value: value_expr,
                });
                out.push(MirStmt::SetState { value: -1 });
                out.push(MirStmt::Expr(MirExpr::Local(dest)));
                MirExpr::Local(dest)
            }
            HirExpr::Template { parts, ty, .. } => {
                let mut args: Vec<MirExpr> = Vec::with_capacity(parts.len());
                for p in parts {
                    args.push(self.convert_expr(
                        p,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        ctx,
                    ));
                }
                let dest = self.fresh_local();
                self.push_temp_local(dest, *ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    args,
                    dest: Some(dest),
                    ty: *ty,
                });
                MirExpr::Local(dest)
            }
            HirExpr::New { callee, args, ty } => {
                let callee_mir =
                    self.convert_expr(callee, out, shared_struct_ids, shared_next_struct, ctx);
                out.push(MirStmt::Expr(callee_mir));
                let struct_id =
                    self.lookup_or_alloc_struct_id(*ty, shared_struct_ids, shared_next_struct);
                let alloc_id = self.fresh_local();
                self.push_temp_local(alloc_id, *ty);
                out.push(MirStmt::Let {
                    local: alloc_id,
                    ty: *ty,
                    init: Some(MirExpr::StructLiteral {
                        struct_id,
                        fields: Vec::new(),
                        ty: *ty,
                    }),
                    mutable: true,
                });
                let ctor_callee = PLACEHOLDER_FUNCTION;
                let mut ctor_args: Vec<MirExpr> = Vec::with_capacity(args.len() + 1);
                ctor_args.push(MirExpr::Local(alloc_id));
                for a in args {
                    ctor_args.push(self.convert_expr(
                        a,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        ctx,
                    ));
                }
                out.push(MirStmt::Expr(MirExpr::Call {
                    callee: ctor_callee,
                    args: ctor_args,
                    ty: *ty,
                }));
                MirExpr::Local(alloc_id)
            }
            HirExpr::OptionalChain { base, ty } => {
                ctx.error(
                    "P0005",
                    "optional chaining (?.) is not yet supported in HIR→MIR",
                    Span::new(0, 0),
                );
                let inner =
                    self.convert_expr(base, out, shared_struct_ids, shared_next_struct, ctx);
                let _ = (ty, inner);
                MirExpr::Unit
            }
            HirExpr::TypeAssertion { expr, target } => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, ctx);
                let _ = target;
                inner
            }
            HirExpr::Assignment { target, value, ty } => {
                let target_mir =
                    self.convert_expr(target, out, shared_struct_ids, shared_next_struct, ctx);
                let target_place = mir_expr_to_place(target_mir, ctx, |non_place_mir| {
                    let temp = self.fresh_local();
                    self.push_temp_local(temp, TypeId::from_raw(0));
                    out.push(MirStmt::Let {
                        local: temp,
                        ty: TypeId::from_raw(0),
                        init: Some(non_place_mir),
                        mutable: false,
                    });
                    temp
                });
                let value_mir =
                    self.convert_expr(value, out, shared_struct_ids, shared_next_struct, ctx);
                if let Some(place) = target_place {
                    out.push(MirStmt::Assign {
                        target: place,
                        value: value_mir.clone(),
                    });
                }
                let _ = ty;
                value_mir
            }
        }
    }
}

fn mir_expr_to_place<F>(e: MirExpr, ctx: &mut PassContext, materialize: F) -> Option<MirPlace>
where
    F: FnMut(MirExpr) -> LocalId,
{
    match e {
        MirExpr::Local(id) => Some(MirPlace::Local { id }),
        MirExpr::Field { base, field, ty } => {
            let base_pb = mir_expr_to_place_base(*base, ctx, materialize)?;
            Some(MirPlace::Field {
                base: Box::new(base_pb),
                field,
                ty,
            })
        }
        MirExpr::Index { base, index, ty } => Some(MirPlace::Index { base, index, ty }),
        _ => {
            ctx.error(
                "P0006",
                "expression is not a valid assignment target",
                Span::new(0, 0),
            );
            None
        }
    }
}

fn mir_expr_to_place_base<F>(
    e: MirExpr,
    ctx: &mut PassContext,
    materialize: F,
) -> Option<MirPlaceBase>
where
    F: FnMut(MirExpr) -> LocalId,
{
    let mut materialize = materialize;
    materialize_place_base(e, ctx, &mut materialize)
}

#[allow(clippy::only_used_in_recursion)]
fn materialize_place_base<F>(
    e: MirExpr,
    ctx: &mut PassContext,
    materialize: &mut F,
) -> Option<MirPlaceBase>
where
    F: FnMut(MirExpr) -> LocalId,
{
    match e {
        MirExpr::Local(id) => Some(MirPlaceBase::Local(id)),
        MirExpr::Field { base, field, ty } => {
            let inner = materialize_place_base(*base, ctx, materialize)?;
            Some(MirPlaceBase::Field {
                base: Box::new(inner),
                field,
                ty,
            })
        }
        MirExpr::Index { base, index, ty } => Some(MirPlaceBase::Index { base, index, ty }),
        non_place => Some(MirPlaceBase::Local(materialize(non_place))),
    }
}
