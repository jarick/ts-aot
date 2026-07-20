use ts_aot_core::{Type, TypeId, TypeTable};
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
        | MirExpr::DynamicFrom { ty, .. }
        | MirExpr::Conditional { ty, .. } => *ty,
        MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => {
            TypeId::from_raw(0)
        }
    }
}

pub(super) fn is_dynamic_owner(owner: &HirExpr, types: &TypeTable) -> bool {
    if matches!(owner, HirExpr::ObjectLiteral { .. }) {
        return true;
    }
    let Some(ty_id) = hir_expr_type_id(owner) else {
        return false;
    };
    let Some(ty) = types.resolve(ty_id) else {
        return false;
    };
    is_dynamic_type(ty, types)
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
        | HirExpr::CompoundUpdate { ty, .. } => Some(*ty),
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

pub(super) fn is_dynamic_type(ty: &Type, types: &TypeTable) -> bool {
    match ty {
        Type::Dynamic => true,
        Type::Optional { inner } => types
            .resolve(*inner)
            .is_some_and(|inner_ty| is_dynamic_type(inner_ty, types)),
        Type::Named { symbol } => {
            let s = symbol.as_str();
            s == "any" || s == "Object" || s == "unknown"
        }
        _ => false,
    }
}

pub(super) fn is_string_typed(expr: &HirExpr, types: &TypeTable) -> bool {
    let Some(ty_id) = hir_expr_type_id(expr) else {
        return matches!(expr, HirExpr::String(_));
    };
    matches!(types.resolve(ty_id), Some(Type::String))
}

pub(super) const DYNAMIC_OP_ADD: u8 = 0;
pub(super) const DYNAMIC_OP_SUB: u8 = 1;
pub(super) const DYNAMIC_OP_MUL: u8 = 2;
pub(super) const DYNAMIC_OP_DIV: u8 = 3;
pub(super) const DYNAMIC_OP_MOD: u8 = 4;

pub(super) fn map_dynamic_op(op: ts_aot_ir_hir::HirBinaryOp) -> Option<u8> {
    use ts_aot_ir_hir::HirBinaryOp;
    match op {
        HirBinaryOp::Add => Some(DYNAMIC_OP_ADD),
        HirBinaryOp::Sub => Some(DYNAMIC_OP_SUB),
        HirBinaryOp::Mul => Some(DYNAMIC_OP_MUL),
        HirBinaryOp::Div => Some(DYNAMIC_OP_DIV),
        HirBinaryOp::Mod => Some(DYNAMIC_OP_MOD),
        _ => None,
    }
}
