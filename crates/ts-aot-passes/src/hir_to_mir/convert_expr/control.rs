use std::collections::HashMap;

use ts_aot_core::{LocalId, NullishKinds, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::place::{mir_expr_to_place, mir_place_to_expr};
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, hir_expr_type_id, is_pure_place, mir_expr_ty,
};
use crate::hir_to_mir::converter::ExprConverter;
use crate::hir_to_mir::nullish::{LOGICAL_DIAG, NULLISH_NARROWING_DIAG};
use crate::hir_to_mir::ops::convert_binop;

impl ExprConverter {
    pub(super) fn materialize_non_place(
        &mut self,
        non_place_mir: MirExpr,
        out: &mut Vec<MirStmt>,
    ) -> LocalId {
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
    }

    fn convert_accessor_receiver(
        &mut self,
        owner: &HirExpr,
        is_static: bool,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if is_static {
            return None;
        }
        let recv = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        if is_pure_place(&recv) {
            Some(recv)
        } else {
            Some(MirExpr::Local(self.materialize_non_place(recv, out)))
        }
    }

    pub(super) fn convert_ternary(
        &mut self,
        cond: &HirExpr,
        then_branch: &HirExpr,
        else_branch: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        // The condition is a truthiness test on its own value: it may be an
        // optional even when the conditional's result is not, so the enclosing
        // expectation must not leak into it (`(x && y) ? a : b`).
        let cond_mir = self.convert_without_expected(
            cond,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let mut then_stmts: Vec<MirStmt> = Vec::new();
        let then_value = self.convert_expr(
            then_branch,
            &mut then_stmts,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let mut else_stmts: Vec<MirStmt> = Vec::new();
        let else_value = self.convert_expr(
            else_branch,
            &mut else_stmts,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        // HIR does not type every expression (a call or a member read carries
        // `Error` until the callee is lowered, and a *recursive* call is only
        // resolvable through the declaration), but the converted branch value
        // carries its own type - which is what makes `n <= 1 ? 1 : n * fact(n - 1)`
        // work.
        let then_ty = self
            .expr_type(then_branch, types)
            .or_else(|| self.mir_expr_static_ty(&then_value, types));
        let else_ty = self
            .expr_type(else_branch, types)
            .or_else(|| self.mir_expr_static_ty(&else_value, types));
        let dest_ty = if matches!(types.resolve(ty), None | Some(Type::Error)) {
            self.joined_branch_ty(then_branch, else_branch, then_ty, else_ty, ty, types, ctx)
        } else {
            ty
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, dest_ty);
        out.push(MirStmt::Let {
            local: dest,
            ty: dest_ty,
            init: None,
            mutable: true,
        });
        then_stmts.push(MirStmt::Assign {
            target: MirPlace::Local { id: dest },
            value: cast_numeric_branch_to_dest(then_value, then_ty, dest_ty, types),
        });
        else_stmts.push(MirStmt::Assign {
            target: MirPlace::Local { id: dest },
            value: cast_numeric_branch_to_dest(else_value, else_ty, dest_ty, types),
        });
        out.push(MirStmt::If {
            cond: cond_mir,
            then_block: MirBlock { stmts: then_stmts },
            else_block: Some(MirBlock { stmts: else_stmts }),
        });
        MirExpr::Local(dest)
    }

    /// The result type of a conditional whose HIR node carries none, recovered
    /// from its branches.
    ///
    /// A `null`/`undefined` branch has no type of its own: the AOT target stores
    /// it in the `Option<T>` of the *other* branch, so `c ? x : undefined` is
    /// `T | undefined`. The surrounding nullable annotation (a return type, a
    /// parameter or a `let`) is preferred when there is one, so `c ? 1 :
    /// undefined` typed `number | undefined` becomes `Option<f64>` rather than
    /// `Option<i64>`.
    #[allow(clippy::too_many_arguments)]
    fn joined_branch_ty(
        &self,
        then_branch: &HirExpr,
        else_branch: &HirExpr,
        then_ty: Option<TypeId>,
        else_ty: Option<TypeId>,
        fallback: TypeId,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> TypeId {
        if is_nullish_branch(then_branch) || is_nullish_branch(else_branch) {
            let literal = if is_nullish_branch(then_branch) {
                then_branch
            } else {
                else_branch
            };
            let other = if is_nullish_branch(then_branch) {
                else_ty
            } else {
                then_ty
            };
            let candidate = other.map(|other| self.nullable_of(other, literal, types));
            if let Some(expected) = self.expected_ty
                && !matches!(types.resolve(expected), None | Some(Type::Error))
                && is_nullable(types.resolve(expected), types)
            {
                // A union has no Rust representation of its own: keep the
                // `Option<T>` recovered from the branches (`i64 | null |
                // undefined` is still `Option<i64>`), which is also what the
                // `join_expr_types` path produced before.
                let union_expected = matches!(types.resolve(expected), Some(Type::Union { .. }));
                if union_expected {
                    if let Some(candidate) = candidate {
                        if branch_matches_expected(expected, candidate, types) {
                            return candidate;
                        }
                        ctx.error(
                            "E0414",
                            "cannot unify the branch types of this conditional expression with \
                             the nullable union type it is written into; add an explicit type \
                             annotation"
                                .to_owned(),
                            then_branch.span(),
                        );
                        return candidate;
                    }
                } else {
                    if candidate
                        .is_none_or(|candidate| branch_matches_expected(expected, candidate, types))
                    {
                        return expected;
                    }
                    ctx.error(
                        "E0414",
                        "cannot unify the branch types of this conditional expression with the \
                         nullable type it is written into; add an explicit type annotation"
                            .to_owned(),
                        then_branch.span(),
                    );
                    return expected;
                }
            }
            if let Some(candidate) = candidate {
                return candidate;
            }
        }
        match (then_ty, else_ty) {
            (Some(a), Some(b)) => match self.join_expr_types(a, b, types) {
                Some(joined) => joined,
                None => {
                    ctx.error(
                        "E0414",
                        "cannot unify the branch types of this conditional expression; add \
                         an explicit type annotation"
                            .to_owned(),
                        Span::new(0, 0),
                    );
                    fallback
                }
            },
            // Nothing could be recovered for a branch: without a type the
            // temporary would be emitted as `()`, which only shows up as a
            // confusing type error in the generated Rust.
            _ => {
                ctx.error(
                    "E0414",
                    "cannot determine the result type of this conditional expression from its \
                     branches; add an explicit type annotation"
                        .to_owned(),
                    Span::new(0, 0),
                );
                fallback
            }
        }
    }

    /// The optional type that carries `other` plus the nullish kind of the
    /// `null`/`undefined` literal branch. A branch that is already nullable
    /// already represents the literal.
    fn nullable_of(&self, other: TypeId, literal: &HirExpr, types: &mut TypeTable) -> TypeId {
        let nullish = match literal {
            HirExpr::Null(_) => NullishKinds::NULL_ONLY,
            _ => NullishKinds::UNDEFINED_ONLY,
        };
        // Merge the literal's kind into an existing optional instead of dropping
        // it: `c ? (x: T | undefined) : null` is `T | null | undefined`, and
        // returning the branch type as-is would make a later `=== null` answer
        // incorrectly.
        if let Some(Type::Optional {
            inner,
            nullish: existing,
        }) = types.resolve(other).cloned()
        {
            return types.intern(&Type::Optional {
                inner,
                nullish: NullishKinds {
                    null: existing.null || nullish.null,
                    undefined: existing.undefined || nullish.undefined,
                },
            });
        }
        if is_nullable(types.resolve(other), types) {
            return other;
        }
        types.intern(&Type::Optional {
            inner: other,
            nullish,
        })
    }

    pub(super) fn convert_sequence(
        &mut self,
        exprs: &[HirExpr],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let Some((last, rest)) = exprs.split_last() {
            for e in rest {
                let mir =
                    self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx);
                if has_potential_side_effects(&mir) {
                    out.push(MirStmt::Expr(mir));
                }
            }
            self.convert_expr(last, out, shared_struct_ids, shared_next_struct, types, ctx)
        } else {
            MirExpr::Unit
        }
    }

    /// Static type of an assignment target. `HirExpr::Global` references are
    /// untyped in HIR (they carry `Type::Error`), so fall back to the declared
    /// type of the module-level global so the value temp is typed correctly.
    fn assignment_target_ty(
        &self,
        target: &HirExpr,
        fallback: TypeId,
        types: &TypeTable,
    ) -> TypeId {
        if let HirExpr::Global { name, ty, .. } = target
            && matches!(types.resolve(*ty), Some(Type::Error))
            && let Some(decl_ty) = self.global_decl_type(name)
        {
            return decl_ty;
        }
        hir_expr_type_id(target).unwrap_or(fallback)
    }

    pub(super) fn convert_assignment(
        &mut self,
        target: &HirExpr,
        value: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let HirExpr::Field {
            owner, field_name, ..
        } = target
            && let Some((setter_fid, value_ty, is_static)) =
                self.resolve_accessor_setter(owner, field_name, shared_struct_ids, types)
        {
            let rhs_ty = hir_expr_type_id(value).unwrap_or(ty);
            if !is_assignment_compatible(value, rhs_ty, value_ty, types, target.span(), ctx) {
                return MirExpr::Unit;
            }
            let receiver = self.convert_accessor_receiver(
                owner,
                is_static,
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
            self.push_temp_local(value_temp, value_ty);
            let needs_cast = rhs_ty != value_ty
                && matches!(types.resolve(value_ty), Some(t) if is_primitive_cast_target(t))
                && matches!(types.resolve(rhs_ty), Some(t) if is_primitive_cast_target(t));
            let init = if needs_cast {
                MirExpr::Cast {
                    expr: Box::new(value_mir),
                    ty: value_ty,
                }
            } else {
                value_mir
            };
            out.push(MirStmt::Let {
                local: value_temp,
                ty: value_ty,
                init: Some(init),
                mutable: false,
            });
            let mut args = Vec::new();
            if let Some(receiver) = receiver {
                args.push(receiver);
            }
            args.push(MirExpr::Local(value_temp));
            out.push(MirStmt::Expr(MirExpr::Call {
                callee: setter_fid,
                args,
                ty: types.intern(&Type::Void),
            }));
            return MirExpr::Local(value_temp);
        }
        let rhs_ty = hir_expr_type_id(value).unwrap_or(ty);
        let target_ty = self.assignment_target_ty(target, ty, types);
        if !is_assignment_compatible(value, rhs_ty, target_ty, types, target.span(), ctx) {
            return MirExpr::Unit;
        }
        let value_ty = match types.resolve(rhs_ty) {
            Some(Type::Error) => target_ty,
            _ => rhs_ty,
        };
        let target_mir = self.convert_expr(
            target,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let target_place = mir_expr_to_place(target_mir, ctx, |non_place_mir| {
            self.materialize_non_place(non_place_mir, out)
        });
        let mut value_mir = self.convert_expr(
            value,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        if matches!(value_mir, MirExpr::Null { .. }) && is_nullable(types.resolve(target_ty), types)
        {
            value_mir = MirExpr::Null { ty: target_ty };
        }
        let null_typed_to_target = matches!(value_mir, MirExpr::Null { ty } if ty == target_ty);
        let value_temp = self.fresh_local();
        let needs_cast = value_ty != target_ty
            && matches!(types.resolve(target_ty), Some(t) if is_primitive_cast_target(t))
            && matches!(types.resolve(value_ty), Some(t) if is_primitive_cast_target(t));
        let init_expr = if needs_cast {
            MirExpr::Cast {
                expr: Box::new(value_mir),
                ty: target_ty,
            }
        } else {
            value_mir
        };
        let declared_ty = if needs_cast || null_typed_to_target {
            target_ty
        } else {
            value_ty
        };
        self.push_temp_local(value_temp, declared_ty);
        out.push(MirStmt::Let {
            local: value_temp,
            ty: declared_ty,
            init: Some(init_expr),
            mutable: false,
        });
        if let Some(place) = target_place {
            out.push(MirStmt::Assign {
                target: place,
                value: MirExpr::Local(value_temp),
            });
        }
        MirExpr::Local(value_temp)
    }
    pub(super) fn convert_compound_update(
        &mut self,
        target: &HirExpr,
        op: HirBinaryOp,
        rhs: &HirExpr,
        post: bool,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let Some(mir) = self.convert_accessor_compound_update(
            target,
            op,
            rhs,
            post,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }

        let target_ty = self.assignment_target_ty(target, ty, types);
        let rhs_ty = hir_expr_type_id(rhs).unwrap_or(ty);
        if !is_assignment_compatible(rhs, rhs_ty, target_ty, types, target.span(), ctx) {
            return MirExpr::Unit;
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
            self.materialize_non_place(non_place_mir, out)
        });

        let Some(place) = target_place else {
            return MirExpr::Unit;
        };

        let place = self.ensure_place_pure_components(place, out);

        let old_temp = self.fresh_local();
        self.push_temp_local(old_temp, target_ty);
        let load_expr = mir_place_to_expr(place.clone());
        out.push(MirStmt::Let {
            local: old_temp,
            ty: target_ty,
            init: Some(load_expr),
            mutable: false,
        });

        // `x &&= v`, `x ||= v` and `x ??= v` are the assignment forms of the
        // short-circuiting operators: they yield one of their operands and only
        // conditionally evaluate `v`, so they must not go through `convert_binop`
        // (that emitted Rust's `&&`/`||` for any operand type, and lowered `??`
        // as an equality comparison). The target was already loaded once into
        // `old_temp`, so the equivalent expression is `<old> <op> rhs`.
        if !post
            && matches!(
                op,
                HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
            )
        {
            // The target was already materialised into `old_temp`, which has no
            // HIR node of its own: hand the loaded value to the MIR-level
            // lowering instead of resolving a synthetic HIR local through the
            // `LocalId` map, which would allocate a fresh (never declared) local
            // for the reference.
            let old_value = MirExpr::Local(old_temp);
            let lhs_ty = self.mir_expr_static_ty(&old_value, types);
            let lowered = match op {
                HirBinaryOp::Coalesce => Some(self.convert_coalesce_operand(
                    old_value,
                    lhs_ty,
                    target.span(),
                    rhs,
                    ty,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                _ => self.convert_operand_logical_lowered(
                    op,
                    old_value,
                    None,
                    lhs_ty,
                    target.span(),
                    rhs,
                    self.expected_ty,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                ),
            };
            if let Some(lowered) = lowered {
                let value = match lowered {
                    MirExpr::Local(_) => lowered,
                    other => MirExpr::Local(self.materialize_non_place(other, out)),
                };
                out.push(MirStmt::Assign {
                    target: place,
                    value: value.clone(),
                });
                return value;
            }
        }

        let rhs_mir =
            self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);

        let needs_rhs_cast = rhs_ty != target_ty
            && matches!(types.resolve(target_ty), Some(t) if is_primitive_cast_target(t))
            && matches!(types.resolve(rhs_ty), Some(t) if is_primitive_cast_target(t));
        let rhs_for_binop = if needs_rhs_cast {
            MirExpr::Cast {
                expr: Box::new(rhs_mir),
                ty: target_ty,
            }
        } else {
            rhs_mir
        };

        if post {
            let new_temp = self.fresh_local();
            self.push_temp_local(new_temp, target_ty);
            let post_new_value = MirExpr::Binary {
                op: convert_binop(op, ctx),
                left: Box::new(MirExpr::Local(old_temp)),
                right: Box::new(rhs_for_binop),
                ty: target_ty,
            };
            out.push(MirStmt::Let {
                local: new_temp,
                ty: target_ty,
                init: Some(post_new_value),
                mutable: false,
            });
            out.push(MirStmt::Assign {
                target: place,
                value: MirExpr::Local(new_temp),
            });
            MirExpr::Local(old_temp)
        } else {
            let new_temp = self.fresh_local();
            self.push_temp_local(new_temp, target_ty);
            let new_value = MirExpr::Binary {
                op: convert_binop(op, ctx),
                left: Box::new(MirExpr::Local(old_temp)),
                right: Box::new(rhs_for_binop),
                ty: target_ty,
            };
            out.push(MirStmt::Let {
                local: new_temp,
                ty: target_ty,
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

    fn convert_accessor_compound_update(
        &mut self,
        target: &HirExpr,
        op: HirBinaryOp,
        rhs: &HirExpr,
        post: bool,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirExpr::Field {
            owner, field_name, ..
        } = target
        else {
            return None;
        };
        let getter = self.resolve_accessor_getter(owner, field_name, shared_struct_ids, types, ty);
        let setter = self.resolve_accessor_setter(owner, field_name, shared_struct_ids, types);
        let ((getter_fid, getter_ret_ty, is_static), (setter_fid, value_ty, setter_static)) =
            match (getter, setter) {
                (Some(getter), Some(setter)) => (getter, setter),
                (Some(_), None) => {
                    ctx.error(
                        "E0411",
                        format!(
                            "accessor `{field_name}` is read-only: a compound update requires \
                             both a getter and a setter"
                        ),
                        target.span(),
                    );
                    return Some(MirExpr::Unit);
                }
                (None, Some(_)) => {
                    ctx.error(
                        "E0412",
                        format!(
                            "accessor `{field_name}` is write-only: a compound update requires \
                             both a getter and a setter"
                        ),
                        target.span(),
                    );
                    return Some(MirExpr::Unit);
                }
                (None, None) => return None,
            };
        debug_assert_eq!(
            is_static, setter_static,
            "accessor getter and setter must agree on static-ness"
        );

        let rhs_ty = hir_expr_type_id(rhs).unwrap_or(ty);
        if !is_assignment_compatible(rhs, rhs_ty, value_ty, types, target.span(), ctx) {
            return Some(MirExpr::Unit);
        }

        let receiver = self.convert_accessor_receiver(
            owner,
            is_static,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );

        let old_temp = self.fresh_local();
        self.push_temp_local(old_temp, getter_ret_ty);
        let mut getter_args = Vec::new();
        if let Some(recv) = &receiver {
            getter_args.push(recv.clone());
        }
        out.push(MirStmt::Let {
            local: old_temp,
            ty: getter_ret_ty,
            init: Some(MirExpr::Call {
                callee: getter_fid,
                args: getter_args,
                ty: getter_ret_ty,
            }),
            mutable: false,
        });

        // `??=`, `&&=` and `||=` are not a Rust binary operator over the old value
        // and the right-hand side: the right-hand side runs only when the short
        // circuit reaches it, and the result is one of the two operands (never a
        // `bool` - `convert_binop` maps `??` to equality, and Rust's `&&`/`||`
        // demand `bool`s). The MIR-level lowering that already implements this for
        // a materialised target takes the getter's value as its left operand, so
        // the accessor path reuses it and hands only the lowered result to the
        // setter.
        if matches!(
            op,
            HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
        ) {
            let operator = match op {
                HirBinaryOp::Coalesce => "??=",
                HirBinaryOp::And => "&&=",
                _ => "||=",
            };
            let old_value = MirExpr::Local(old_temp);
            let lhs_ty = self.mir_expr_static_ty(&old_value, types);
            let lowered = match op {
                HirBinaryOp::Coalesce => Some(self.convert_coalesce_operand(
                    old_value,
                    lhs_ty,
                    target.span(),
                    rhs,
                    value_ty,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                _ => self.convert_operand_logical_lowered(
                    op,
                    old_value,
                    None,
                    lhs_ty,
                    target.span(),
                    rhs,
                    Some(value_ty),
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                ),
            };
            let Some(lowered) = lowered else {
                // Falling through to the plain binary expression would evaluate the
                // right-hand side eagerly and store a comparison result, so report
                // instead of emitting that.
                ctx.error(
                    LOGICAL_DIAG,
                    format!(
                        "the `{operator}` compound update of accessor `{field_name}` cannot be \
                         lowered without evaluating its right-hand side eagerly"
                    ),
                    target.span(),
                );
                return Some(MirExpr::Unit);
            };
            let new_value = match lowered {
                MirExpr::Local(_) => lowered,
                other => MirExpr::Local(self.materialize_non_place(other, out)),
            };
            // The setter has its own value type: a lowering that met the operands
            // in a wider numeric type is cast back to it, the way the arithmetic
            // path casts its right-hand side.
            let new_value = cast_numeric_branch_to_dest(
                new_value.clone(),
                self.mir_expr_static_ty(&new_value, types),
                value_ty,
                types,
            );

            // `obj.w &&= 3` over a `number | null` accessor pair keeps the nullish
            // left operand, and a `number` setter cannot take it. The lowering
            // reports a nullish result that a non-nullish expectation rejects, but
            // only for the value it hands back: check what the setter would receive.
            let setter_takes_nullish = matches!(
                types.resolve(value_ty),
                Some(Type::Null | Type::Void | Type::Optional { .. } | Type::Error)
            );
            let lowered_ty = self.mir_expr_static_ty(&new_value, types);
            let lowered_is_nullish = matches!(
                lowered_ty.and_then(|ty| types.resolve(ty)),
                Some(Type::Null | Type::Void | Type::Optional { .. })
            );
            if !setter_takes_nullish && lowered_is_nullish {
                ctx.error(
                    NULLISH_NARROWING_DIAG,
                    format!(
                        "the `{operator}` compound update of accessor `{field_name}` can keep a \
                         nullish value, which its setter does not accept"
                    ),
                    target.span(),
                );
                return Some(MirExpr::Unit);
            }

            let mut setter_args = Vec::new();
            if let Some(recv) = receiver {
                setter_args.push(recv);
            }
            setter_args.push(new_value.clone());
            out.push(MirStmt::Expr(MirExpr::Call {
                callee: setter_fid,
                args: setter_args,
                ty: types.intern(&Type::Void),
            }));

            return Some(if post {
                MirExpr::Local(old_temp)
            } else {
                new_value
            });
        }

        let rhs_mir =
            self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);
        let needs_rhs_cast = rhs_ty != value_ty
            && matches!(types.resolve(value_ty), Some(t) if is_primitive_cast_target(t))
            && matches!(types.resolve(rhs_ty), Some(t) if is_primitive_cast_target(t));
        let rhs_for_binop = if needs_rhs_cast {
            MirExpr::Cast {
                expr: Box::new(rhs_mir),
                ty: value_ty,
            }
        } else {
            rhs_mir
        };
        let needs_old_cast = getter_ret_ty != value_ty
            && matches!(types.resolve(value_ty), Some(t) if is_primitive_cast_target(t))
            && matches!(types.resolve(getter_ret_ty), Some(t) if is_primitive_cast_target(t));
        let old_for_binop = if needs_old_cast {
            MirExpr::Cast {
                expr: Box::new(MirExpr::Local(old_temp)),
                ty: value_ty,
            }
        } else {
            MirExpr::Local(old_temp)
        };

        let new_temp = self.fresh_local();
        self.push_temp_local(new_temp, value_ty);
        out.push(MirStmt::Let {
            local: new_temp,
            ty: value_ty,
            init: Some(MirExpr::Binary {
                op: convert_binop(op, ctx),
                left: Box::new(old_for_binop),
                right: Box::new(rhs_for_binop),
                ty: value_ty,
            }),
            mutable: false,
        });

        let mut setter_args = Vec::new();
        if let Some(recv) = receiver {
            setter_args.push(recv);
        }
        setter_args.push(MirExpr::Local(new_temp));
        out.push(MirStmt::Expr(MirExpr::Call {
            callee: setter_fid,
            args: setter_args,
            ty: types.intern(&Type::Void),
        }));

        if post {
            Some(MirExpr::Local(old_temp))
        } else {
            Some(MirExpr::Local(new_temp))
        }
    }
}

fn cast_numeric_branch_to_dest(
    value: MirExpr,
    branch_ty: Option<TypeId>,
    dest_ty: TypeId,
    types: &TypeTable,
) -> MirExpr {
    let Some(branch_ty) = branch_ty else {
        return value;
    };
    if branch_ty == dest_ty {
        return value;
    }
    if matches!(types.resolve(branch_ty), Some(t) if is_primitive_cast_target(t))
        && matches!(types.resolve(dest_ty), Some(t) if is_primitive_cast_target(t))
    {
        return MirExpr::Cast {
            expr: Box::new(value),
            ty: dest_ty,
        };
    }
    value
}

fn is_primitive_cast_target(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeCategory {
    Numeric,
    String,
    Bool,
    Void,
    Null,
    Other,
}

fn classify(ty: Option<&Type>) -> TypeCategory {
    match ty {
        Some(
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64,
        ) => TypeCategory::Numeric,
        Some(Type::String) => TypeCategory::String,
        Some(Type::Bool) => TypeCategory::Bool,
        Some(Type::Void) => TypeCategory::Void,
        Some(Type::Null) => TypeCategory::Null,
        _ => TypeCategory::Other,
    }
}

fn is_nullable(ty: Option<&Type>, types: &TypeTable) -> bool {
    match ty {
        Some(Type::Null) => true,
        Some(Type::Optional { .. }) => true,
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|vid| is_nullable(types.resolve(*vid), types)),
        _ => false,
    }
}

/// Whether a branch of a conditional is the `null`/`undefined` literal itself
/// (the value the optional representation absorbs).
fn is_nullish_branch(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Null(_) | HirExpr::Undefined(_) | HirExpr::Unit(_)
    )
}

/// Whether the nullable type recovered from the branches can be written into
/// `expected` (the surrounding annotation): the same optional, or two optional
/// numerics the backend coerces.
///
/// `expected` may be a union that carries the nullish variants directly
/// (`i64 | null | undefined`): only its non-nullish, non-optional value types
/// matter for the comparison.
fn branch_matches_expected(expected: TypeId, candidate: TypeId, types: &TypeTable) -> bool {
    if types.resolve(expected) == types.resolve(candidate) {
        return true;
    }
    let Some(Type::Optional {
        inner: candidate_inner,
        ..
    }) = types.resolve(candidate)
    else {
        return false;
    };
    let candidate_inner = *candidate_inner;
    // A partially known optional (`T | undefined` whose `T` is still `Error`)
    // cannot prove an incompatibility: trust the annotation.
    if matches!(types.resolve(candidate_inner), None | Some(Type::Error)) {
        return true;
    }
    expected_value_types(expected, types)
        .into_iter()
        .any(|value_ty| {
            types.resolve(value_ty) == types.resolve(candidate_inner)
                || (matches!(types.resolve(value_ty), Some(t) if is_primitive_cast_target(t))
                    && matches!(types.resolve(candidate_inner), Some(t) if is_primitive_cast_target(t)))
        })
}

/// The value types an expectation can hold: `T` for `T | null` (and every
/// non-nullish variant of a union).
fn expected_value_types(expected: TypeId, types: &TypeTable) -> Vec<TypeId> {
    match types.resolve(expected) {
        Some(Type::Optional { inner, .. }) => vec![*inner],
        Some(Type::Union { variants }) => variants
            .iter()
            .flat_map(|variant| match types.resolve(*variant) {
                Some(Type::Optional { inner, .. }) => vec![*inner],
                Some(Type::Null | Type::Void) => Vec::new(),
                _ => vec![*variant],
            })
            .collect(),
        _ => vec![expected],
    }
}

fn classify_hir(value: &HirExpr, value_ty: TypeId, types: &TypeTable) -> TypeCategory {
    match value {
        HirExpr::String(_, _) => TypeCategory::String,
        HirExpr::Bool(_, _) => TypeCategory::Bool,
        HirExpr::Int(_, _) | HirExpr::Float(_, _) => TypeCategory::Numeric,
        HirExpr::Null(_) => TypeCategory::Null,
        HirExpr::Unit(_) | HirExpr::Undefined(_) => TypeCategory::Void,
        _ => classify(types.resolve(value_ty)),
    }
}

fn report_compat_error(ctx: &mut PassContext, code: &str, message: &str, span: Span) -> bool {
    ctx.error(code, message.to_owned(), span);
    false
}

fn report_compat_warning(ctx: &mut PassContext, code: &str, message: &str, span: Span) -> bool {
    ctx.warning(code, message.to_owned(), span);
    true
}

fn is_assignment_compatible(
    value: &HirExpr,
    value_ty: TypeId,
    target_ty: TypeId,
    types: &TypeTable,
    span: Span,
    ctx: &mut PassContext,
) -> bool {
    let target_resolved = types.resolve(target_ty);
    let value_cat = classify_hir(value, value_ty, types);
    if value_cat == TypeCategory::Void
        && !matches!(target_resolved, Some(Type::Void))
        // `undefined` (Unit) is the natural representation of the nullish part
        // of an optional target, so `let x: number | undefined = undefined` is
        // not a Unit-to-value coercion.
        && !is_nullable(target_resolved, types)
    {
        return report_compat_warning(
            ctx,
            "E0409",
            "void/Unit value assigned to a typed target; this AOT target has no implicit \
             Unit-to-value coercion, the value is lowered as-is",
            span,
        );
    }

    if target_resolved.is_none() || matches!(target_resolved, Some(Type::Error)) {
        return true;
    }
    if !crate::hir_to_mir::nullish::check_nullish_write(
        value,
        target_ty,
        types,
        span,
        ctx,
        "the assignment target",
        true,
    ) {
        return false;
    }
    let target_cat = classify(target_resolved);

    if target_cat == value_cat {
        return true;
    }
    if target_cat == TypeCategory::Numeric && value_cat == TypeCategory::Numeric {
        return true;
    }
    if value_cat == TypeCategory::Null && is_nullable(types.resolve(target_ty), types) {
        return true;
    }
    match (target_cat, value_cat) {
        (TypeCategory::Void, _) => report_compat_warning(
            ctx,
            "E0409",
            "a value is assigned to a void-typed target; this AOT target has no implicit \
             value-to-Unit coercion, the value is lowered as-is",
            span,
        ),
        (TypeCategory::Numeric, TypeCategory::String)
        | (TypeCategory::String, TypeCategory::Numeric) => report_compat_error(
            ctx,
            "E0408",
            "string and numeric values cannot be assigned to each other; \
             this AOT target has no implicit string-to-number coercion",
            span,
        ),
        (TypeCategory::String, TypeCategory::Bool) | (TypeCategory::Bool, TypeCategory::String) => {
            report_compat_error(
                ctx,
                "E0410",
                "string and boolean values cannot be assigned to each other; \
             this AOT target has no implicit string-to-boolean coercion",
                span,
            )
        }
        (TypeCategory::Numeric, TypeCategory::Bool)
        | (TypeCategory::Bool, TypeCategory::Numeric) => report_compat_error(
            ctx,
            "E0410",
            "boolean value cannot be assigned to a numeric target (and vice versa); \
             this AOT target has no implicit boolean-to-number coercion",
            span,
        ),
        (_, TypeCategory::Null) | (TypeCategory::Null, _) => report_compat_warning(
            ctx,
            "E0410",
            "null assigned to a non-null type; this AOT target has no implicit null \
             coercion, the value is lowered as-is",
            span,
        ),
        _ => true,
    }
}
