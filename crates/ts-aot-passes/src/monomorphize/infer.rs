use std::collections::{HashMap, HashSet};

use ts_aot_core::{GenericParamId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirExpr, HirFunction};

use super::substitute::TypeParamMap;

pub fn infer_type_args(
    generic_fn: &HirFunction,
    args: &[HirExpr],
    types: &mut TypeTable,
) -> Vec<TypeId> {
    let mut found: HashMap<GenericParamId, TypeId> = HashMap::new();
    let mut conflicted: HashSet<GenericParamId> = HashSet::new();
    let mut has_resolved_non_generic_param = false;
    for (param, arg) in generic_fn.params.iter().zip(args.iter()) {
        let arg_ty = hir_expr_ty(arg, types);
        if let Some(param_resolved) = types.resolve(param.ty) {
            if !matches!(param_resolved, Type::GenericParam { .. }) {
                has_resolved_non_generic_param = true;
            }
            bind_param_ty_resolved(
                param_resolved.clone(),
                arg_ty,
                &mut found,
                &mut conflicted,
                types,
            );
        }
    }
    generic_fn
        .type_params
        .iter()
        .enumerate()
        .map(|(i, id)| {
            found
                .get(id)
                .copied()
                .or_else(|| {
                    if !has_resolved_non_generic_param
                        && generic_fn.type_params.len() == 1
                        && !conflicted.contains(id)
                    {
                        args.get(i).and_then(|a| hir_expr_ty(a, types))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| types.intern(&Type::GenericParam { id: *id }))
        })
        .collect()
}

pub fn bind_param_ty_resolved(
    param_resolved: Type,
    arg_ty: Option<TypeId>,
    found: &mut HashMap<GenericParamId, TypeId>,
    conflicted: &mut HashSet<GenericParamId>,
    types: &TypeTable,
) {
    let Some(arg_ty) = arg_ty else {
        return;
    };
    match param_resolved {
        Type::GenericParam { id } => {
            if conflicted.contains(&id) {
                return;
            }
            if let Some(existing) = found.get(&id) {
                if *existing != arg_ty {
                    found.remove(&id);
                    conflicted.insert(id);
                }
            } else {
                found.insert(id, arg_ty);
            }
        }
        Type::Optional { inner } => {
            if let Some(Type::Optional { inner: arg_inner }) = types.resolve(arg_ty).cloned()
                && let Some(inner_resolved) = types.resolve(inner).cloned()
            {
                bind_param_ty_resolved(inner_resolved, Some(arg_inner), found, conflicted, types);
            }
        }
        Type::Array { element } => {
            if let Some(Type::Array {
                element: arg_element,
            }) = types.resolve(arg_ty).cloned()
                && let Some(element_resolved) = types.resolve(element).cloned()
            {
                bind_param_ty_resolved(
                    element_resolved,
                    Some(arg_element),
                    found,
                    conflicted,
                    types,
                );
            }
        }
        Type::Fn {
            params: fn_params,
            ret,
            err,
        } => {
            if let Some(Type::Fn {
                params: arg_params,
                ret: arg_ret,
                err: arg_err,
            }) = types.resolve(arg_ty).cloned()
            {
                for (p, a) in fn_params.iter().zip(arg_params.iter()) {
                    if let Some(pr) = types.resolve(*p).cloned() {
                        bind_param_ty_resolved(pr, Some(*a), found, conflicted, types);
                    }
                }
                if let Some(rr) = types.resolve(ret).cloned() {
                    bind_param_ty_resolved(rr, Some(arg_ret), found, conflicted, types);
                }
                if let (Some(e1), Some(e2)) = (err, arg_err)
                    && let Some(er) = types.resolve(e1).cloned()
                {
                    bind_param_ty_resolved(er, Some(e2), found, conflicted, types);
                }
            }
        }
        Type::Promise { ok, err } => {
            if let Some(Type::Promise {
                ok: arg_ok,
                err: arg_err,
            }) = types.resolve(arg_ty).cloned()
            {
                if let Some(or) = types.resolve(ok).cloned() {
                    bind_param_ty_resolved(or, Some(arg_ok), found, conflicted, types);
                }
                if let (Some(e1), Some(e2)) = (err, arg_err)
                    && let Some(er) = types.resolve(e1).cloned()
                {
                    bind_param_ty_resolved(er, Some(e2), found, conflicted, types);
                }
            }
        }
        Type::Result { ok, err } => {
            if let Some(Type::Result {
                ok: arg_ok,
                err: arg_err,
            }) = types.resolve(arg_ty).cloned()
            {
                if let Some(or) = types.resolve(ok).cloned() {
                    bind_param_ty_resolved(or, Some(arg_ok), found, conflicted, types);
                }
                if let Some(er) = types.resolve(err).cloned() {
                    bind_param_ty_resolved(er, Some(arg_err), found, conflicted, types);
                }
            }
        }
        _ => {}
    }
}

pub fn build_mapping(generic_fn: &HirFunction, type_args: &[TypeId]) -> TypeParamMap {
    let mut mapping = TypeParamMap::new();
    for (param_id, ty) in generic_fn.type_params.iter().zip(type_args.iter()) {
        mapping.insert(*param_id, *ty);
    }
    mapping
}

pub fn type_args_resolved(type_args: &[TypeId], types: &TypeTable) -> bool {
    type_args.iter().all(|t| type_resolved(*t, types))
}

pub fn type_resolved(ty: TypeId, types: &TypeTable) -> bool {
    let Some(resolved) = types.resolve(ty) else {
        return false;
    };
    match resolved {
        Type::GenericParam { .. } => false,
        Type::Optional { inner } => type_resolved(*inner, types),
        Type::Array { element } => type_resolved(*element, types),
        Type::Fn { params, ret, err } => {
            params.iter().all(|p| type_resolved(*p, types))
                && type_resolved(*ret, types)
                && err.is_none_or(|e| type_resolved(e, types))
        }
        Type::Promise { ok, err } => {
            type_resolved(*ok, types) && err.is_none_or(|e| type_resolved(e, types))
        }
        Type::Result { ok, err } => type_resolved(*ok, types) && type_resolved(*err, types),
        _ => true,
    }
}

pub fn format_type_args(type_args: &[TypeId]) -> String {
    if type_args.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = type_args.iter().map(|t| format!("t{}", t.raw())).collect();
    format!("_{}", parts.join("_"))
}

pub fn hir_expr_ty(expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
    match expr {
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
        | HirExpr::Sequence { ty, .. }
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
        HirExpr::Int(_) => Some(types.intern(&Type::I64)),
        HirExpr::Float(_) => Some(types.intern(&Type::F64)),
        HirExpr::String(_) => Some(types.intern(&Type::String)),
        HirExpr::Bool(_) => Some(types.intern(&Type::Bool)),
        HirExpr::Null => Some(types.intern(&Type::Null)),
        HirExpr::Unit | HirExpr::Undefined => None,
    }
}
