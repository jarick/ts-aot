use std::collections::HashMap;

use ts_aot_core::{Atom, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::ops::{convert_binop, convert_unaryop};

mod place;
mod util;

use place::{mir_expr_to_place, mir_place_to_expr};
use util::{has_potential_side_effects, hir_expr_type_id, mir_expr_ty};

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
            HirExpr::Unit(_) => MirExpr::Unit,
            HirExpr::Bool(b, _) => MirExpr::Bool(*b),
            HirExpr::Int(v, _) => MirExpr::Int {
                value: i128::from(*v),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Float(bits, _) => MirExpr::Float {
                value: f64::from_bits(*bits),
                ty: TypeId::from_raw(0),
            },
            HirExpr::String(id, _) => MirExpr::String {
                id: id.clone(),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Null(_) => MirExpr::Null {
                ty: TypeId::from_raw(0),
            },
            HirExpr::Undefined(_) => MirExpr::Unit,
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
            HirExpr::Call {
                callee, args, ty, ..
            } => {
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
                    if let Some(callee_ty) = hir_expr_type_id(inner.as_ref())
                        && let Some(Type::Fn { .. }) = types.resolve(callee_ty)
                    {
                        ctx.error(
                            "E0405",
                            "function-typed value cannot be called in Phase 4 — \
                             Type::Fn lowers to `()` and `()` is not callable. \
                             Use a named function declaration or call through a known callee instead.",
                            Span::new(0, 0),
                        );
                        return MirExpr::Unit;
                    }
                    if let HirExpr::Field {
                        owner, field_name, ..
                    } = inner.as_ref()
                        && is_global_object_reference(owner)
                        && matches!(
                            field_name.as_str(),
                            "getPrototypeOf" | "keys" | "setPrototypeOf"
                        )
                    {
                        ctx.error(
                            "E0404",
                            format!(
                                "`Object.{}()` is not supported in strict AOT mode. \
                                 Use static alternatives: struct fields for known types, \
                                 or compile-time constants for prototype introspection.",
                                field_name
                            ),
                            Span::new(0, 0),
                        );
                        return MirExpr::Unit;
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
            HirExpr::Binary {
                op, lhs, rhs, ty, ..
            } => match op {
                HirBinaryOp::In => {
                    let lhs_mir = self.convert_expr(
                        lhs,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
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
            HirExpr::Unary { op, expr, ty, .. } => match op {
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
            HirExpr::StructLiteral { ty, fields, .. } => {
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
            HirExpr::ObjectLiteral { .. } => {
                ctx.error(
                    "E0402",
                    "object literals (`{}`) are not supported in strict AOT mode. \
                     Use an explicit struct constructor or factory function instead.",
                    Span::new(0, 0),
                );
                MirExpr::Unit
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
            HirExpr::RegExp {
                pattern, flags, ty, ..
            } => MirExpr::RegExp {
                pattern: pattern.to_string(),
                flags: flags.to_string(),
                ty: *ty,
            },
            HirExpr::BigInt { value, ty, .. } => MirExpr::BigInt {
                value: value.to_string(),
                ty: *ty,
            },
            HirExpr::Import { source, ty, .. } => {
                let mut sub_out = Vec::new();
                let source_mir = self.convert_expr(
                    source,
                    &mut sub_out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                out.extend(sub_out);
                MirExpr::Import {
                    source: Box::new(source_mir),
                    ty: *ty,
                }
            }
            HirExpr::Ternary {
                cond,
                then_branch,
                else_branch,
                ty,
                ..
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
            HirExpr::ArrayLiteral { elements, ty, .. } => {
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
            HirExpr::Await { expr, ty, .. } => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                MirExpr::Await {
                    expr: Box::new(inner),
                    ty: *ty,
                }
            }
            HirExpr::Yield { expr, ty, .. } => {
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
                    let mut call_args: Vec<MirExpr> = Vec::with_capacity(1 + expressions.len());
                    call_args.push(MirExpr::TemplateStringsArray {
                        cooked: cooked_atoms,
                        ty: *ty,
                    });
                    for e in expressions.iter() {
                        call_args.push(self.convert_expr(
                            e,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        ));
                    }
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
            HirExpr::New {
                callee, args, ty, ..
            } => {
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
            HirExpr::OptionalChain { base, ty: _, .. } => {
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
            HirExpr::TypeAssertion { expr, target, .. } => {
                let inner =
                    self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
                let _ = target;
                inner
            }
            HirExpr::Assignment {
                target, value, ty, ..
            } => {
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
                ..
            } => {
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
}

fn is_global_object_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Object")
}
