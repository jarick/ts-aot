use ts_aot_core::TypeId;
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
        | MirExpr::TypeOf { ty, .. } => *ty,
        MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => {
            TypeId::from_raw(0)
        }
    }
}
