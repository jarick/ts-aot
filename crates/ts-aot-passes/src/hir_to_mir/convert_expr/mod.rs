use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

mod place;
mod util;

use place::{mir_expr_to_place, mir_place_to_expr};
use util::{
    has_potential_side_effects, hir_expr_type_id, is_dynamic_owner, is_dynamic_type,
    is_string_typed, map_dynamic_op, mir_expr_ty,
};

impl ExprConverter {
    pub(super) fn convert_expr(
        &mut self,
        e: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
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
                if is_dynamic_owner(owner, types) {
                    let owner_mir = self.convert_dynamic_owner(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let dynamic_ty = types.intern(&Type::Dynamic);
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, dynamic_ty);
                    let field_name_mir = MirExpr::String {
                        id: field_name.clone(),
                        ty: TypeId::from_raw(0),
                    };
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpObjectGet,
                        args: vec![owner_mir, field_name_mir],
                        dest: Some(dest),
                        ty: dynamic_ty,
                    });
                    return MirExpr::Local(dest);
                }
                let resolved_field =
                    self.resolve_field_id(owner, field_name, *field, shared_struct_ids, ctx);
                MirExpr::Field {
                    base: Box::new(self.convert_expr(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
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
                    types,
                    ctx,
                )),
                index: Box::new(self.convert_expr(
                    index,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                ty: *ty,
            },
            HirExpr::Call { callee, args, ty } => {
                let callee_id = self.resolve_callee(callee, ctx);
                let mir_args: Vec<MirExpr> = args
                    .iter()
                    .map(|a| {
                        self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx)
                    })
                    .collect();
                if callee_id == PLACEHOLDER_FUNCTION
                    && let HirCallee::Indirect(inner) = callee
                {
                    let callee_value = self.convert_expr(
                        inner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    return MirExpr::IndirectCall {
                        callee: Box::new(callee_value),
                        args: mir_args,
                        ty: *ty,
                    };
                }
                MirExpr::Call {
                    callee: callee_id,
                    args: mir_args,
                    ty: *ty,
                }
            }
            HirExpr::Binary { op, lhs, rhs, ty } => match op {
                HirBinaryOp::In => {
                    let lhs_mir = self.convert_expr(
                        lhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    if is_dynamic_owner(rhs, types) && is_string_typed(lhs, types) {
                        let rhs_mir = self.convert_dynamic_owner(
                            rhs,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        let dest = self.fresh_local();
                        self.push_temp_local(dest, *ty);
                        out.push(MirStmt::Runtime {
                            op: RuntimeOp::OpObjectHas,
                            args: vec![rhs_mir, lhs_mir],
                            dest: Some(dest),
                            ty: *ty,
                        });
                        return MirExpr::Local(dest);
                    }
                    let rhs_mir = self.convert_expr(
                        rhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, *ty);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpIn,
                        args: vec![lhs_mir, rhs_mir],
                        dest: Some(dest),
                        ty: *ty,
                    });
                    MirExpr::Local(dest)
                }
                HirBinaryOp::InstanceOf => {
                    let value_mir = self.convert_expr(
                        lhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let target_mir = self.convert_expr(
                        rhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let target_type_id: u32 = match rhs.as_ref() {
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
                                ts_aot_core::Span::new(0, 0),
                            );
                            0
                        }
                    };
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, *ty);
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
                        ty: *ty,
                    });
                    MirExpr::Local(dest)
                }
                _ => MirExpr::Binary {
                    op: convert_binop(*op, ctx),
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
                    ty: *ty,
                },
            },
            HirExpr::Unary { op, expr, ty } => match op {
                HirUnaryOp::TypeOf => {
                    let inner = self.convert_expr(
                        expr,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let string_ty = types.intern(&ts_aot_core::Type::String);
                    MirExpr::TypeOf {
                        expr: Box::new(inner),
                        ty: string_ty,
                    }
                }
                HirUnaryOp::Void => {
                    let inner = self.convert_expr(
                        expr,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    if has_potential_side_effects(&inner) {
                        out.push(MirStmt::Expr(inner));
                    }
                    MirExpr::Unit
                }
                HirUnaryOp::Delete => {
                    if let HirExpr::Field {
                        owner, field_name, ..
                    } = expr.as_ref()
                        && is_dynamic_owner(owner, types)
                    {
                        let owner_mir = self.convert_dynamic_owner(
                            owner,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        let dynamic_ty = types.intern(&Type::Dynamic);
                        out.push(MirStmt::Runtime {
                            op: RuntimeOp::OpObjectDelete,
                            args: vec![
                                owner_mir,
                                MirExpr::String {
                                    id: field_name.clone(),
                                    ty: TypeId::from_raw(0),
                                },
                            ],
                            dest: None,
                            ty: dynamic_ty,
                        });
                        return MirExpr::Bool(true);
                    }
                    let inner = self.convert_expr(
                        expr,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    match expr.as_ref() {
                        HirExpr::Field { .. } | HirExpr::Index { .. } => {
                            let dest = self.fresh_local();
                            let bool_ty = types.intern(&ts_aot_core::Type::Bool);
                            self.push_temp_local(dest, bool_ty);
                            out.push(MirStmt::Runtime {
                                op: RuntimeOp::OpDelete,
                                args: vec![inner],
                                dest: Some(dest),
                                ty: bool_ty,
                            });
                            MirExpr::Local(dest)
                        }
                        _ => {
                            if has_potential_side_effects(&inner) {
                                out.push(MirStmt::Expr(inner));
                            }
                            MirExpr::Bool(true)
                        }
                    }
                }
                _ => MirExpr::Unary {
                    op: convert_unaryop(*op, ctx),
                    expr: Box::new(self.convert_expr(
                        expr,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )),
                    ty: *ty,
                },
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
                                    types,
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
                    .map(|e| {
                        self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx)
                    })
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
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                MirExpr::Await {
                    expr: Box::new(inner),
                    ty: *ty,
                }
            }
            HirExpr::Yield { expr, ty } => {
                let inner = expr
                    .as_ref()
                    .map(|e| {
                        self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx)
                    })
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
                        types,
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
                let callee_mir = self.convert_expr(
                    callee,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
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
                        types,
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
            HirExpr::OptionalChain { base, ty: _ } => {
                let inner =
                    self.convert_expr(base, out, shared_struct_ids, shared_next_struct, types, ctx);
                let base_ty = crate::monomorphize::hir_expr_ty(base, types)
                    .unwrap_or_else(|| mir_expr_ty(&inner));
                let inner_ty = match types.resolve(base_ty) {
                    Some(ts_aot_core::Type::Optional { inner }) => *inner,
                    _ => base_ty,
                };
                let opt_ty = types.intern(&Type::Optional { inner: inner_ty });
                MirExpr::OptionalChain {
                    base: Box::new(inner),
                    ty: opt_ty,
                }
            }
            HirExpr::TypeAssertion { expr, target } => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                let _ = target;
                inner
            }
            HirExpr::Assignment { target, value, ty } => {
                if let HirExpr::Field {
                    owner, field_name, ..
                } = target.as_ref()
                    && is_dynamic_owner(owner, types)
                {
                    let owner_mir = self.convert_dynamic_owner(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let value_mir = self.convert_expr(
                        value,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let value_temp = self.fresh_local();
                    self.push_temp_local(value_temp, *ty);
                    out.push(MirStmt::Let {
                        local: value_temp,
                        ty: *ty,
                        init: Some(MirExpr::DynamicFrom {
                            value: Box::new(value_mir),
                            ty: *ty,
                        }),
                        mutable: false,
                    });
                    let dynamic_ty = types.intern(&Type::Dynamic);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpObjectSet,
                        args: vec![
                            owner_mir,
                            MirExpr::String {
                                id: field_name.clone(),
                                ty: TypeId::from_raw(0),
                            },
                            MirExpr::Local(value_temp),
                        ],
                        dest: None,
                        ty: dynamic_ty,
                    });
                    return MirExpr::Local(value_temp);
                }
                let target_mir = self.convert_expr(
                    target,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
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
                let value_mir = self.convert_expr(
                    value,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
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
                if let HirExpr::Field {
                    owner, field_name, ..
                } = target.as_ref()
                    && is_dynamic_owner(owner, types)
                {
                    let owner_mir = self.convert_dynamic_owner(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let owner_temp = self.fresh_local();
                    let dynamic_ty = types.intern(&Type::Dynamic);
                    self.push_temp_local(owner_temp, dynamic_ty);
                    out.push(MirStmt::Let {
                        local: owner_temp,
                        ty: dynamic_ty,
                        init: Some(owner_mir),
                        mutable: true,
                    });
                    let old_local = self.fresh_local();
                    self.push_temp_local(old_local, dynamic_ty);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpObjectGet,
                        args: vec![
                            MirExpr::Local(owner_temp),
                            MirExpr::String {
                                id: field_name.clone(),
                                ty: TypeId::from_raw(0),
                            },
                        ],
                        dest: Some(old_local),
                        ty: dynamic_ty,
                    });
                    let rhs_mir = self.convert_expr(
                        rhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let new_local = self.fresh_local();
                    self.push_temp_local(new_local, dynamic_ty);
                    let Some(dynamic_op_id) = map_dynamic_op(*op) else {
                        ctx.error(
                            "P0005",
                            format!(
                                "compound update for dynamic field with operator {:?} is not yet \
                                 supported; supported operators are Add, Sub, Mul, Div, Mod. \
                                 The dynamic compound update is treated as Undefined to keep \
                                 lowering total; the original owner is unchanged",
                                op
                            ),
                            ts_aot_core::Span::new(0, 0),
                        );
                        out.push(MirStmt::Let {
                            local: new_local,
                            ty: dynamic_ty,
                            init: Some(MirExpr::DynamicFrom {
                                value: Box::new(MirExpr::Unit),
                                ty: dynamic_ty,
                            }),
                            mutable: false,
                        });
                        return if *post {
                            MirExpr::Local(old_local)
                        } else {
                            MirExpr::Local(new_local)
                        };
                    };
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpDynamicBinary,
                        args: vec![
                            MirExpr::Int {
                                value: i128::from(dynamic_op_id),
                                ty: TypeId::from_raw(0),
                            },
                            MirExpr::Local(old_local),
                            MirExpr::DynamicFrom {
                                value: Box::new(rhs_mir),
                                ty: dynamic_ty,
                            },
                        ],
                        dest: Some(new_local),
                        ty: dynamic_ty,
                    });
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpObjectSet,
                        args: vec![
                            MirExpr::Local(owner_temp),
                            MirExpr::String {
                                id: field_name.clone(),
                                ty: TypeId::from_raw(0),
                            },
                            MirExpr::Local(new_local),
                        ],
                        dest: None,
                        ty: dynamic_ty,
                    });
                    return if *post {
                        MirExpr::Local(old_local)
                    } else {
                        MirExpr::Local(new_local)
                    };
                }
                let target_mir = self.convert_expr(
                    target,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
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
                    self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);

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

    fn convert_dynamic_owner(
        &mut self,
        owner: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let dynamic_ty = types.intern(&Type::Dynamic);
        let source_mir = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let mut current = hir_expr_type_id(owner);
        let outer_is_optional = matches!(
            current.and_then(|id| types.resolve(id)),
            Some(Type::Optional { .. })
        );
        let mut mir = if outer_is_optional {
            source_mir
        } else {
            let dest = self.fresh_local();
            self.push_temp_local(dest, dynamic_ty);
            out.push(MirStmt::Let {
                local: dest,
                ty: dynamic_ty,
                init: Some(source_mir),
                mutable: true,
            });
            MirExpr::Local(dest)
        };
        while let Some(ty_id) = current {
            let Some(Type::Optional { inner }) = types.resolve(ty_id) else {
                break;
            };
            let Some(inner_ty) = types.resolve(*inner) else {
                break;
            };
            if !is_dynamic_type(inner_ty, types) {
                break;
            }
            let unwrap_dest = self.fresh_local();
            self.push_temp_local(unwrap_dest, dynamic_ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                args: vec![mir],
                dest: Some(unwrap_dest),
                ty: dynamic_ty,
            });
            mir = MirExpr::Local(unwrap_dest);
            current = Some(*inner);
        }
        mir
    }
}
