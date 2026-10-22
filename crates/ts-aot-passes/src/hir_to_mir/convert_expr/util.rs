use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

fn is_constant_literal(e: &MirExpr) -> bool {
    matches!(
        e,
        MirExpr::Unit
            | MirExpr::Bool(_)
            | MirExpr::Int { .. }
            | MirExpr::Float { .. }
            | MirExpr::String { .. }
            | MirExpr::Null { .. }
    )
}

fn is_effect_free_leaf(e: &MirExpr) -> bool {
    is_constant_literal(e) || matches!(e, MirExpr::Local(_) | MirExpr::Global(_))
}

pub(super) fn has_potential_side_effects(e: &MirExpr) -> bool {
    !is_effect_free_leaf(e)
}

pub(super) fn is_pure_place(e: &MirExpr) -> bool {
    if is_effect_free_leaf(e) {
        return true;
    }
    match e {
        MirExpr::Field { base, .. } => is_pure_place(base),
        MirExpr::Index { base, index, .. } => is_pure_place(base) && is_pure_place(index),
        _ => false,
    }
}

pub(super) fn mir_expr_ty(e: &MirExpr) -> TypeId {
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
        | MirExpr::Yield { ty, .. }
        | MirExpr::OptionalChain { ty, .. }
        | MirExpr::IndirectCall { ty, .. }
        | MirExpr::TypeOf { ty, .. }
        | MirExpr::Cast { ty, .. }
        | MirExpr::TemplateStringsArray { ty, .. }
        | MirExpr::RegExp { ty, .. }
        | MirExpr::BigInt { ty, .. }
        | MirExpr::Import { ty, .. } => *ty,
        MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => {
            TypeId::from_raw(0)
        }
        MirExpr::Closure { fn_ty, .. } => *fn_ty,
    }
}

pub(super) fn is_numeric_type_for_array_len(arg: &HirExpr, types: &TypeTable) -> bool {
    if matches!(arg, HirExpr::Int(_, _) | HirExpr::Float(_, _)) {
        return true;
    }
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(
        types.resolve(ty),
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
                | Type::F64
        )
    )
}

pub(super) fn hir_expr_type_id(owner: &HirExpr) -> Option<TypeId> {
    match owner {
        HirExpr::Local { ty, .. }
        | HirExpr::Global { ty, .. }
        | HirExpr::Field { ty, .. }
        | HirExpr::Index { ty, .. }
        | HirExpr::Call { ty, .. }
        | HirExpr::Binary { ty, .. }
        | HirExpr::Unary { ty, .. }
        | HirExpr::StructLiteral { ty, .. }
        | HirExpr::ObjectLiteral { ty, .. }
        | HirExpr::Ternary { ty, .. }
        | HirExpr::ArrayLiteral { ty, .. }
        | HirExpr::Closure { ty, .. }
        | HirExpr::Await { ty, .. }
        | HirExpr::Yield { ty, .. }
        | HirExpr::Template { ty, .. }
        | HirExpr::New { ty, .. }
        | HirExpr::OptionalChain { ty, .. }
        | HirExpr::Assignment { ty, .. }
        | HirExpr::CompoundUpdate { ty, .. }
        | HirExpr::Sequence { ty, .. }
        | HirExpr::RegExp { ty, .. }
        | HirExpr::BigInt { ty, .. }
        | HirExpr::Import { ty, .. } => Some(*ty),
        HirExpr::TypeAssertion { target, .. } => Some(*target),
        HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Bool(_, _)
        | HirExpr::Null(_)
        | HirExpr::Unit(_)
        | HirExpr::Undefined(_) => None,
    }
}

impl ExprConverter {
    pub(super) fn convert_length_argument(
        &mut self,
        args: &[HirExpr],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
        noun: &str,
        note: &str,
    ) -> MirExpr {
        if args.is_empty() {
            ctx.warning(
                "E0406",
                format!("{noun} requires exactly 1 argument; got 0 — the length defaults to 0"),
                Span::new(0, 0),
            );
        } else if args.len() > 1 {
            ctx.warning(
                "E0406",
                format!(
                    "{noun} requires exactly 1 argument; got {} — extra arguments are ignored",
                    args.len()
                ),
                Span::new(0, 0),
            );
        }
        let length_mir = if let Some(first) = args.first() {
            let converted = self.convert_expr(
                first,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            if is_numeric_type_for_array_len(first, types) {
                if is_constant_literal(&converted) {
                    converted
                } else {
                    let ty = match mir_expr_ty(&converted) {
                        t if t.raw() != 0 => t,
                        _ => hir_expr_type_id(first).unwrap_or_else(|| types.intern(&Type::I64)),
                    };
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, ty);
                    out.push(MirStmt::Let {
                        local: dest,
                        ty,
                        init: Some(converted),
                        mutable: false,
                    });
                    MirExpr::Local(dest)
                }
            } else {
                ctx.warning(
                    "E0406",
                    format!(
                        "{noun} length argument must be a number; got a non-numeric \
                         expression — the length defaults to 0{note}"
                    ),
                    Span::new(0, 0),
                );
                if has_potential_side_effects(&converted) {
                    out.push(MirStmt::Expr(converted));
                }
                MirExpr::Int {
                    value: 0,
                    ty: types.intern(&Type::I64),
                }
            }
        } else {
            MirExpr::Int {
                value: 0,
                ty: types.intern(&Type::I64),
            }
        };
        for extra_hir in args.iter().skip(1) {
            let extra = self.convert_expr(
                extra_hir,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            if has_potential_side_effects(&extra) {
                out.push(MirStmt::Expr(extra));
            }
        }
        length_mir
    }
}
