use std::collections::HashMap;

use ts_aot_core::{LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirExpr};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::place::{mir_expr_to_place, mir_place_to_expr};
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, hir_expr_type_id, mir_expr_ty,
};
use crate::hir_to_mir::converter::ExprConverter;
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
        let cond_mir =
            self.convert_expr(cond, out, shared_struct_ids, shared_next_struct, types, ctx);
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Let {
            local: dest,
            ty,
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
        let rhs_ty = hir_expr_type_id(value).unwrap_or(ty);
        let target_ty = hir_expr_type_id(target).unwrap_or(ty);
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
        let value_mir = self.convert_expr(
            value,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
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
        let declared_ty = if needs_cast { target_ty } else { value_ty };
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

        let target_ty = hir_expr_type_id(target).unwrap_or(ty);
        let old_temp = self.fresh_local();
        self.push_temp_local(old_temp, target_ty);
        let load_expr = mir_place_to_expr(place.clone());
        out.push(MirStmt::Let {
            local: old_temp,
            ty: target_ty,
            init: Some(load_expr),
            mutable: false,
        });

        let rhs_mir =
            self.convert_expr(rhs, out, shared_struct_ids, shared_next_struct, types, ctx);

        let rhs_ty = hir_expr_type_id(rhs).unwrap_or(ty);
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
            let post_new_value = MirExpr::Binary {
                op: convert_binop(op, ctx),
                left: Box::new(MirExpr::Local(old_temp)),
                right: Box::new(rhs_for_binop),
                ty: target_ty,
            };
            out.push(MirStmt::Assign {
                target: place,
                value: post_new_value,
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
        _ => TypeCategory::Other,
    }
}

fn report_compat_error(ctx: &mut PassContext, code: &str, message: &str, span: Span) -> bool {
    ctx.error(code, message.to_owned(), span);
    false
}

fn is_assignment_compatible(
    value: &HirExpr,
    value_ty: TypeId,
    target_ty: TypeId,
    types: &TypeTable,
    span: Span,
    ctx: &mut PassContext,
) -> bool {
    let target_cat = classify(types.resolve(target_ty));
    let value_cat = classify(types.resolve(value_ty));

    if matches!(value, HirExpr::String(_, _)) && target_cat == TypeCategory::Numeric {
        return report_compat_error(
            ctx,
            "E0410",
            "string value cannot be assigned to a numeric target; \
             this AOT target has no implicit string-to-number coercion",
            span,
        );
    }
    if target_cat == value_cat {
        return true;
    }
    if target_cat == TypeCategory::Numeric && value_cat == TypeCategory::Numeric {
        return true;
    }
    match (target_cat, value_cat) {
        (_, TypeCategory::Void) | (TypeCategory::Void, _) => report_compat_error(
            ctx,
            "E0409",
            "void/Unit value cannot be assigned to a typed target; \
             this AOT target has no implicit Unit-to-value coercion",
            span,
        ),
        (TypeCategory::Numeric, TypeCategory::String) => report_compat_error(
            ctx,
            "E0408",
            "string value cannot be assigned to a numeric target; \
             this AOT target has no implicit string-to-number coercion",
            span,
        ),
        (TypeCategory::Numeric, TypeCategory::Bool)
        | (TypeCategory::Bool, TypeCategory::Numeric) => report_compat_error(
            ctx,
            "E0410",
            "boolean value cannot be assigned to a numeric target (and vice versa); \
             this AOT target has no implicit boolean-to-number coercion",
            span,
        ),
        _ => true,
    }
}
