use std::collections::HashMap;

use ts_aot_core::{Atom, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirUnaryOp, ObjectLiteralField};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt, RuntimeOp};

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
                    if field_name.as_str() == "__proto__" {
                        let dest = self.fresh_local();
                        self.push_temp_local(dest, dynamic_ty);
                        out.push(MirStmt::Runtime {
                            op: RuntimeOp::OpObjectProtoGet,
                            args: vec![owner_mir],
                            dest: Some(dest),
                            ty: dynamic_ty,
                        });
                        return MirExpr::Local(dest);
                    }
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
                    if let HirExpr::Field { field_name, .. } = inner.as_ref() {
                        let expected = match field_name.as_str() {
                            "getPrototypeOf" | "keys" => Some(1),
                            "setPrototypeOf" => Some(2),
                            _ => None,
                        };
                        if let Some(exp) = expected
                            && args.len() != exp
                        {
                            ctx.error(
                                "P0005",
                                format!(
                                    "Object.{} requires exactly {} argument{}, got {}",
                                    field_name.as_str(),
                                    exp,
                                    if exp == 1 { "" } else { "s" },
                                    args.len()
                                ),
                                ts_aot_core::Span::new(0, 0),
                            );
                            return MirExpr::Unit;
                        }
                    }
                    let all_args_dynamic = args.iter().all(|a| match a {
                        HirExpr::ObjectLiteral { .. } => true,
                        HirExpr::Local { ty, .. }
                        | HirExpr::Global { ty, .. }
                        | HirExpr::Field { ty, .. }
                        | HirExpr::Index { ty, .. }
                        | HirExpr::Call { ty, .. }
                        | HirExpr::Binary { ty, .. }
                        | HirExpr::Unary { ty, .. }
                        | HirExpr::StructLiteral { ty, .. }
                        | HirExpr::Ternary { ty, .. }
                        | HirExpr::Sequence { ty, .. }
                        | HirExpr::ArrayLiteral { ty, .. }
                        | HirExpr::Closure { ty, .. }
                        | HirExpr::Await { ty, .. }
                        | HirExpr::Yield { ty, .. }
                        | HirExpr::Template { ty, .. }
                        | HirExpr::New { ty, .. }
                        | HirExpr::OptionalChain { ty, .. }
                        | HirExpr::Assignment { ty, .. }
                        | HirExpr::CompoundUpdate { ty, .. } => types
                            .resolve(*ty)
                            .is_some_and(|t| is_dynamic_type(t, types)),
                        HirExpr::TypeAssertion { target, .. } => types
                            .resolve(*target)
                            .is_some_and(|t| is_dynamic_type(t, types)),
                        HirExpr::Int(_)
                        | HirExpr::Float(_)
                        | HirExpr::String(_)
                        | HirExpr::Bool(_)
                        | HirExpr::Null
                        | HirExpr::Unit
                        | HirExpr::Undefined => false,
                    });
                    if all_args_dynamic
                        && let Some(result) =
                            self.try_emit_builtin_object_call(inner.as_ref(), &mir_args, *ty, out)
                    {
                        return result;
                    }
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
                    if has_potential_side_effects(&inner) {
                        out.push(MirStmt::Expr(inner));
                    }
                    MirExpr::Bool(true)
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
            HirExpr::ObjectLiteral { fields, ty: _ } => {
                let dynamic_ty = types.intern(&Type::Dynamic);
                let dest = self.fresh_local();
                self.push_temp_local(dest, dynamic_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::OpObjectNew,
                    args: Vec::new(),
                    dest: Some(dest),
                    ty: dynamic_ty,
                });
                for field in fields {
                    let (name, value) = match field {
                        ObjectLiteralField::Property { name, value } => (name, value),
                        ObjectLiteralField::Spread(value) => {
                            ctx.error(
                                "P0005",
                                "object spread is not yet supported in HIR→MIR (planned for PR 7.7); \
                                 spread value is evaluated for side effects but not merged",
                                Span::new(0, 0),
                            );
                            let spread_value = self.convert_expr(
                                value,
                                out,
                                shared_struct_ids,
                                shared_next_struct,
                                types,
                                ctx,
                            );
                            if has_potential_side_effects(&spread_value) {
                                out.push(MirStmt::Expr(spread_value));
                            }
                            continue;
                        }
                    };
                    let value_mir = self.convert_expr(
                        value,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let value_temp = self.fresh_local();
                    self.push_temp_local(value_temp, dynamic_ty);
                    out.push(MirStmt::Let {
                        local: value_temp,
                        ty: dynamic_ty,
                        init: Some(MirExpr::DynamicFrom {
                            value: Box::new(value_mir),
                            ty: dynamic_ty,
                        }),
                        mutable: false,
                    });
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::OpObjectSet,
                        args: vec![
                            MirExpr::Local(dest),
                            MirExpr::String {
                                id: name.clone(),
                                ty: TypeId::from_raw(0),
                            },
                            MirExpr::Local(value_temp),
                        ],
                        dest: None,
                        ty: dynamic_ty,
                    });
                }
                MirExpr::Local(dest)
            }
            HirExpr::Sequence { exprs, .. } => {
                if let Some((last, rest)) = exprs.split_last() {
                    for e in rest {
                        let mir = self.convert_expr(
                            e,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        if has_potential_side_effects(&mir) {
                            out.push(MirStmt::Expr(mir));
                        }
                    }
                    self.convert_expr(last, out, shared_struct_ids, shared_next_struct, types, ctx)
                } else {
                    MirExpr::Unit
                }
            }
            HirExpr::Ternary {
                cond,
                then_branch,
                else_branch,
                ty,
            } => {
                let cond_mir =
                    self.convert_expr(cond, out, shared_struct_ids, shared_next_struct, types, ctx);
                let dest = self.fresh_local();
                self.push_temp_local(dest, *ty);
                out.push(MirStmt::Let {
                    local: dest,
                    ty: *ty,
                    init: None,
                    mutable: true,
                });
                let mut then_stmts: Vec<MirStmt> = Vec::new();
                let then_value = self.convert_expr(
                    then_branch,
                    &mut then_stmts,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                then_stmts.push(MirStmt::Assign {
                    target: MirPlace::Local { id: dest },
                    value: then_value,
                });
                let mut else_stmts: Vec<MirStmt> = Vec::new();
                let else_value = self.convert_expr(
                    else_branch,
                    &mut else_stmts,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                else_stmts.push(MirStmt::Assign {
                    target: MirPlace::Local { id: dest },
                    value: else_value,
                });
                out.push(MirStmt::If {
                    cond: cond_mir,
                    then_block: MirBlock { stmts: then_stmts },
                    else_block: Some(MirBlock { stmts: else_stmts }),
                });
                MirExpr::Local(dest)
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
            HirExpr::Template {
                tag,
                expressions,
                cooked_parts,
                raw_parts,
                ty,
                ..
            } => {
                if let Some(tag_expr) = tag {
                    let tag_mir = self.convert_expr(
                        tag_expr,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    let mut cooked_atoms: Vec<Atom> = Vec::with_capacity(cooked_parts.len());
                    for cooked_opt in cooked_parts.iter() {
                        let cooked_text = cooked_opt.as_ref().map_or("", ts_aot_core::Atom::as_str);
                        cooked_atoms.push(Atom::from(cooked_text));
                    }
                    let mut raw_atoms: Vec<Atom> = Vec::with_capacity(raw_parts.len());
                    for raw_opt in raw_parts.iter() {
                        let raw_text = raw_opt.as_ref().map_or("", ts_aot_core::Atom::as_str);
                        raw_atoms.push(Atom::from(raw_text));
                    }
                    let dynamic_ty = types.intern(&Type::Dynamic);
                    let subs_vec_ty = types.intern(&Type::Array {
                        element: dynamic_ty,
                    });
                    let subs_vec = self.fresh_local();
                    self.push_temp_local(subs_vec, subs_vec_ty);
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::DynVecNew,
                        args: vec![],
                        dest: Some(subs_vec),
                        ty: subs_vec_ty,
                    });
                    for e in expressions.iter() {
                        let sub_mir = self.convert_expr(
                            e,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        let sub_dyn = MirExpr::DynamicFrom {
                            value: Box::new(sub_mir),
                            ty: dynamic_ty,
                        };
                        out.push(MirStmt::Runtime {
                            op: RuntimeOp::DynVecAppend,
                            args: vec![MirExpr::Local(subs_vec), sub_dyn],
                            dest: None,
                            ty: dynamic_ty,
                        });
                    }
                    let call_args: Vec<MirExpr> = vec![
                        MirExpr::TemplateStringsArray {
                            cooked: cooked_atoms,
                            raw: raw_atoms,
                            ty: *ty,
                        },
                        MirExpr::Local(subs_vec),
                    ];
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, *ty);
                    out.push(MirStmt::Let {
                        local: dest,
                        ty: *ty,
                        init: Some(MirExpr::IndirectCall {
                            callee: Box::new(tag_mir),
                            args: call_args,
                            ty: *ty,
                        }),
                        mutable: false,
                    });
                    MirExpr::Local(dest)
                } else {
                    let mut parts: Vec<MirExpr> = Vec::with_capacity(expressions.len() * 2 + 1);
                    for (i, cooked_opt) in cooked_parts.iter().enumerate() {
                        let cooked_text = cooked_opt.as_ref().map_or("", ts_aot_core::Atom::as_str);
                        parts.push(MirExpr::String {
                            id: Atom::from(cooked_text),
                            ty: *ty,
                        });
                        if let Some(e) = expressions.get(i) {
                            parts.push(self.convert_expr(
                                e,
                                out,
                                shared_struct_ids,
                                shared_next_struct,
                                types,
                                ctx,
                            ));
                        }
                    }
                    if parts.is_empty() {
                        MirExpr::Unit
                    } else if parts.len() == 1 {
                        let part = parts.into_iter().next().expect("len 1");
                        let dest = self.fresh_local();
                        self.push_temp_local(dest, *ty);
                        out.push(MirStmt::Let {
                            local: dest,
                            ty: *ty,
                            init: Some(part),
                            mutable: false,
                        });
                        MirExpr::Local(dest)
                    } else {
                        let (first, rest) = parts.split_first().expect("len >= 2");
                        let first_dest = self.fresh_local();
                        self.push_temp_local(first_dest, *ty);
                        out.push(MirStmt::Let {
                            local: first_dest,
                            ty: *ty,
                            init: Some(first.clone()),
                            mutable: false,
                        });
                        let mut current = MirExpr::Local(first_dest);
                        for part in rest.iter() {
                            let dest = self.fresh_local();
                            self.push_temp_local(dest, *ty);
                            out.push(MirStmt::Runtime {
                                op: RuntimeOp::StringConcat,
                                args: vec![current, part.clone()],
                                dest: Some(dest),
                                ty: *ty,
                            });
                            current = MirExpr::Local(dest);
                        }
                        current
                    }
                }
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
                    if field_name.as_str() == "__proto__" {
                        out.push(MirStmt::Runtime {
                            op: RuntimeOp::OpObjectProtoSet,
                            args: vec![owner_mir, MirExpr::Local(value_temp)],
                            dest: None,
                            ty: dynamic_ty,
                        });
                        return MirExpr::Local(value_temp);
                    }
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
                    let is_proto = field_name.as_str() == "__proto__";
                    let old_local = self.fresh_local();
                    self.push_temp_local(old_local, dynamic_ty);
                    let get_op = if is_proto {
                        RuntimeOp::OpObjectProtoGet
                    } else {
                        RuntimeOp::OpObjectGet
                    };
                    let mut get_args = vec![MirExpr::Local(owner_temp)];
                    if !is_proto {
                        get_args.push(MirExpr::String {
                            id: field_name.clone(),
                            ty: TypeId::from_raw(0),
                        });
                    }
                    out.push(MirStmt::Runtime {
                        op: get_op,
                        args: get_args,
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
                    let set_op = if is_proto {
                        RuntimeOp::OpObjectProtoSet
                    } else {
                        RuntimeOp::OpObjectSet
                    };
                    let mut set_args = vec![MirExpr::Local(owner_temp)];
                    if !is_proto {
                        set_args.push(MirExpr::String {
                            id: field_name.clone(),
                            ty: TypeId::from_raw(0),
                        });
                    }
                    set_args.push(MirExpr::Local(new_local));
                    out.push(MirStmt::Runtime {
                        op: set_op,
                        args: set_args,
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

    fn try_emit_builtin_object_call(
        &mut self,
        inner: &HirExpr,
        mir_args: &[MirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
    ) -> Option<MirExpr> {
        let HirExpr::Field {
            owner, field_name, ..
        } = inner
        else {
            return None;
        };
        let HirExpr::Global {
            name: owner_name, ..
        } = owner.as_ref()
        else {
            return None;
        };
        if owner_name.as_str() != "Object" {
            return None;
        }
        let op = match field_name.as_str() {
            "getPrototypeOf" => RuntimeOp::OpObjectProtoGet,
            "setPrototypeOf" => RuntimeOp::OpObjectSetPrototypeOf,
            "keys" => RuntimeOp::OpObjectKeys,
            _ => return None,
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args.to_vec(),
            dest: Some(dest),
            ty,
        });
        Some(MirExpr::Local(dest))
    }
}
