use ts_aot_core::TypeId;
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::MirExpr;

pub(super) fn has_potential_side_effects(e: &MirExpr) -> bool {
    !matches!(
        e,
        MirExpr::Unit
            | MirExpr::Bool(_)
            | MirExpr::Int { .. }
            | MirExpr::Float { .. }
            | MirExpr::String { .. }
            | MirExpr::Null { .. }
            | MirExpr::Local(_)
            | MirExpr::Global(_)
    )
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
        | MirExpr::TemplateStringsArray { ty, .. }
        | MirExpr::RegExp { ty, .. }
        | MirExpr::BigInt { ty, .. }
        | MirExpr::Import { ty, .. } => *ty,
        MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => {
            TypeId::from_raw(0)
        }
    }
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
        HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Bool(_)
        | HirExpr::Null
        | HirExpr::Unit
        | HirExpr::Undefined => None,
    }
}
