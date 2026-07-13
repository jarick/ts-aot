use std::collections::HashMap;

use ts_aot_core::{LocalId, Span, StructId, TypeId};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirPlaceBase, MirStmt, RuntimeOp};

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
                owner,
                field,
                field_name,
                ty,
                ..
            } => {
                let resolved_field =
                    self.resolve_field_id(owner, field_name, *field, shared_struct_ids, ctx);
                MirExpr::Field {
                    base: Box::new(self.convert_expr(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        ctx,
                    )),
                    field: resolved_field,
                    ty: *ty,
                }
            }
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
                if callee_id == PLACEHOLDER_FUNCTION
                    && let HirCallee::Indirect(inner) = callee
                {
                    let callee_value =
                        self.convert_expr(inner, out, shared_struct_ids, shared_next_struct, ctx);
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, *ty);
                    let mut runtime_args = Vec::with_capacity(1 + mir_args.len());
                    runtime_args.push(callee_value);
                    runtime_args.extend(mir_args);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::CallIndirect,
                        args: runtime_args,
                        dest: Some(dest),
                        ty: *ty,
                    });
                    return MirExpr::Local(dest);
                }
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
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, ctx);
                MirExpr::Await {
                    expr: Box::new(inner),
                    ty: *ty,
                }
            }
            HirExpr::Yield { expr, ty } => {
                let inner = expr
                    .as_ref()
                    .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, ctx))
                    .map(Box::new);
                MirExpr::Yield {
                    expr: inner,
                    ty: *ty,
                }
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
                    let temp_ty = mir_expr_ty(&non_place_mir);
                    self.push_temp_local(temp, temp_ty);
                    out.push(MirStmt::Let {
                        local: temp,
                        ty: temp_ty,
                        init: Some(non_place_mir),
                        mutable: false,
                    });
                    temp
                });
                let value_mir =
                    self.convert_expr(value, out, shared_struct_ids, shared_next_struct, ctx);
                let value_temp = self.fresh_local();
                self.push_temp_local(value_temp, *ty);
                out.push(MirStmt::Let {
                    local: value_temp,
                    ty: *ty,
                    init: Some(value_mir),
                    mutable: false,
                });
                if let Some(place) = target_place {
                    out.push(MirStmt::Assign {
                        target: place,
                        value: MirExpr::Local(value_temp),
                    });
                }
                let _ = ty;
                MirExpr::Local(value_temp)
            }
            HirExpr::CompoundUpdate {
                target,
                op,
                rhs,
                post,
                ty,
            } => {
                let target_mir =
                    self.convert_expr(target, out, shared_struct_ids, shared_next_struct, ctx);
                let target_place = mir_expr_to_place(target_mir, ctx, |non_place_mir| {
                    let temp = self.fresh_local();
                    let temp_ty = mir_expr_ty(&non_place_mir);
                    self.push_temp_local(temp, temp_ty);
                    out.push(MirStmt::Let {
                        local: temp,
                        ty: temp_ty,
                        init: Some(non_place_mir),
                        mutable: false,
                    });
                    temp
                });

                let Some(place) = target_place else {
                    return MirExpr::Unit;
                };

                let place = self.ensure_place_pure_components(place, out);

                let old_temp = self.fresh_local();
                self.push_temp_local(old_temp, *ty);
                let load_expr = mir_place_to_expr(place.clone());
                out.push(MirStmt::Let {
                    local: old_temp,
                    ty: *ty,
                    init: Some(load_expr),
                    mutable: false,
                });

                let rhs_mir =
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, ctx);

                if *post {
                    let post_new_value = MirExpr::Binary {
                        op: convert_binop(*op, ctx),
                        left: Box::new(MirExpr::Local(old_temp)),
                        right: Box::new(rhs_mir),
                        ty: *ty,
                    };
                    out.push(MirStmt::Assign {
                        target: place,
                        value: post_new_value,
                    });
                    MirExpr::Local(old_temp)
                } else {
                    let new_temp = self.fresh_local();
                    self.push_temp_local(new_temp, *ty);
                    let new_value = MirExpr::Binary {
                        op: convert_binop(*op, ctx),
                        left: Box::new(MirExpr::Local(old_temp)),
                        right: Box::new(rhs_mir),
                        ty: *ty,
                    };
                    out.push(MirStmt::Let {
                        local: new_temp,
                        ty: *ty,
                        init: Some(new_value),
                        mutable: false,
                    });
                    out.push(MirStmt::Assign {
                        target: place,
                        value: MirExpr::Local(new_temp),
                    });
                    MirExpr::Local(new_temp)
                }
            }
        }
    }
}

fn mir_place_to_expr(p: MirPlace) -> MirExpr {
    match p {
        MirPlace::Local { id } => MirExpr::Local(id),
        MirPlace::Field { base, field, ty } => MirExpr::Field {
            base: Box::new(mir_place_base_to_expr(*base)),
            field,
            ty,
        },
        MirPlace::Index { base, index, ty } => MirExpr::Index { base, index, ty },
    }
}

fn mir_place_base_to_expr(b: MirPlaceBase) -> MirExpr {
    match b {
        MirPlaceBase::Local(id) => MirExpr::Local(id),
        MirPlaceBase::Field { base, field, ty } => MirExpr::Field {
            base: Box::new(mir_place_base_to_expr(*base)),
            field,
            ty,
        },
        MirPlaceBase::Index { base, index, ty } => MirExpr::Index { base, index, ty },
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

impl ExprConverter {
    pub(super) fn ensure_place_pure_components(
        &mut self,
        place: MirPlace,
        out: &mut Vec<MirStmt>,
    ) -> MirPlace {
        match place {
            MirPlace::Local { id } => MirPlace::Local { id },
            MirPlace::Field { base, field, ty } => {
                let new_base = self.ensure_place_base_pure_components(*base, out);
                MirPlace::Field {
                    base: Box::new(new_base),
                    field,
                    ty,
                }
            }
            MirPlace::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirPlace::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
        }
    }

    fn ensure_place_base_pure_components(
        &mut self,
        base: MirPlaceBase,
        out: &mut Vec<MirStmt>,
    ) -> MirPlaceBase {
        match base {
            MirPlaceBase::Local(id) => MirPlaceBase::Local(id),
            MirPlaceBase::Field { base, field, ty } => {
                let inner = self.ensure_place_base_pure_components(*base, out);
                MirPlaceBase::Field {
                    base: Box::new(inner),
                    field,
                    ty,
                }
            }
            MirPlaceBase::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirPlaceBase::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
        }
    }

    fn ensure_mir_expr_pure_components(
        &mut self,
        expr: MirExpr,
        out: &mut Vec<MirStmt>,
    ) -> MirExpr {
        match expr {
            MirExpr::Local(id) => MirExpr::Local(id),
            MirExpr::Field { base, field, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                MirExpr::Field {
                    base: Box::new(new_base),
                    field,
                    ty,
                }
            }
            MirExpr::Index { base, index, ty } => {
                let new_base = self.ensure_mir_expr_pure_components(*base, out);
                let new_index = self.ensure_mir_expr_pure_components(*index, out);
                MirExpr::Index {
                    base: Box::new(new_base),
                    index: Box::new(new_index),
                    ty,
                }
            }
            other => {
                let local = self.fresh_local();
                let local_ty = mir_expr_ty(&other);
                self.push_temp_local(local, local_ty);
                out.push(MirStmt::Let {
                    local,
                    ty: local_ty,
                    init: Some(other),
                    mutable: false,
                });
                MirExpr::Local(local)
            }
        }
    }
}

fn mir_expr_ty(e: &MirExpr) -> TypeId {
    match e {
        MirExpr::Int { ty, .. }
        | MirExpr::Float { ty, .. }
        | MirExpr::String { ty, .. }
        | MirExpr::Null { ty }
        | MirExpr::Field { ty, .. }
        | MirExpr::Index { ty, .. }
        | MirExpr::Call { ty, .. }
        | MirExpr::StructLiteral { ty, .. }
        | MirExpr::ResultOk { ty, .. }
        | MirExpr::ResultErr { ty, .. }
        | MirExpr::Binary { ty, .. }
        | MirExpr::Unary { ty, .. }
        | MirExpr::Await { ty, .. }
        | MirExpr::Yield { ty, .. } => *ty,
        MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => {
            TypeId::from_raw(0)
        }
    }
}
