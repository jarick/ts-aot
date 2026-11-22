use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::coerce::numeric_binary_result_ty;
use super::place::expr_base_ty;
use proc_macro2::{Ident, Punct, Spacing, TokenStream};
use quote::quote;
use ts_aot_core::{FieldId, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{BinaryOp, MirDecl, MirExpr, MirFieldDecl, UnaryOp};

pub(super) fn loop_label(ident: &Ident) -> TokenStream {
    let apostrophe = Punct::new('\'', Spacing::Joint);
    quote!(#apostrophe #ident)
}

fn array_element_of(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty) {
        Some(Type::Array { element }) => Some(*element),
        _ => None,
    }
}

fn expr_array_element_ty(expr: &MirExpr, emit_env: &EmitEnv, body_ctx: &BodyCtx) -> Option<TypeId> {
    let ty = expr_base_ty(expr, body_ctx)?;
    if let Some(element) = array_element_of(ty, emit_env.types) {
        return Some(element);
    }
    match emit_env.types.resolve(ty) {
        Some(Type::Optional { inner, .. }) => array_element_of(*inner, emit_env.types),
        _ => None,
    }
}

fn struct_fields<'a>(emit_env: &EmitEnv<'a>, struct_id: StructId) -> Option<&'a [MirFieldDecl]> {
    emit_env
        .program
        .declarations
        .iter()
        .find_map(|decl| match decl {
            MirDecl::Struct(s) if s.id == struct_id => Some(s.fields.as_slice()),
            _ => None,
        })
}

fn struct_fields_contain_type(
    emit_env: &EmitEnv,
    struct_id: StructId,
    target: TypeId,
    depth: u32,
) -> bool {
    match struct_fields(emit_env, struct_id) {
        Some(fields) => fields
            .iter()
            .any(|field| type_contains_type(field.ty, target, emit_env, depth)),
        None => true,
    }
}

fn type_contains_type(container: TypeId, target: TypeId, emit_env: &EmitEnv, depth: u32) -> bool {
    if container == target {
        return true;
    }
    if depth == 0 {
        return false;
    }
    let depth = depth - 1;
    match emit_env.types.resolve(container) {
        Some(
            Type::Optional { inner, .. }
            | Type::Generator { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner },
        ) => type_contains_type(*inner, target, emit_env, depth),
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|variant| type_contains_type(*variant, target, emit_env, depth)),
        Some(Type::Intersection { parts }) => parts
            .iter()
            .any(|part| type_contains_type(*part, target, emit_env, depth)),
        Some(Type::Array { element }) => type_contains_type(*element, target, emit_env, depth),
        Some(Type::Tuple { elements }) => elements
            .iter()
            .any(|element| type_contains_type(*element, target, emit_env, depth)),
        Some(Type::Result { ok, err }) => {
            type_contains_type(*ok, target, emit_env, depth)
                || type_contains_type(*err, target, emit_env, depth)
        }
        Some(Type::Promise { ok, err }) => {
            type_contains_type(*ok, target, emit_env, depth)
                || err.is_some_and(|err| type_contains_type(err, target, emit_env, depth))
        }
        Some(Type::Struct { id }) => struct_fields_contain_type(emit_env, *id, target, depth),
        Some(Type::Named { .. } | Type::GenericParam { .. } | Type::Error) | None => true,
        _ => false,
    }
}

fn type_may_hold_fn(ty: TypeId, emit_env: &EmitEnv, seen: &mut Vec<StructId>, depth: u32) -> bool {
    if depth == 0 {
        return true;
    }
    let depth = depth - 1;
    match emit_env.types.resolve(ty) {
        Some(
            Type::Optional { inner, .. }
            | Type::Generator { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner },
        ) => type_may_hold_fn(*inner, emit_env, seen, depth),
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|variant| type_may_hold_fn(*variant, emit_env, seen, depth)),
        Some(Type::Intersection { parts }) => parts
            .iter()
            .any(|part| type_may_hold_fn(*part, emit_env, seen, depth)),
        Some(Type::Array { element }) => type_may_hold_fn(*element, emit_env, seen, depth),
        Some(Type::Tuple { elements }) => elements
            .iter()
            .any(|element| type_may_hold_fn(*element, emit_env, seen, depth)),
        Some(Type::Result { ok, err }) => {
            type_may_hold_fn(*ok, emit_env, seen, depth)
                || type_may_hold_fn(*err, emit_env, seen, depth)
        }
        Some(Type::Promise { ok, err }) => {
            type_may_hold_fn(*ok, emit_env, seen, depth)
                || err.is_some_and(|err| type_may_hold_fn(err, emit_env, seen, depth))
        }
        Some(Type::WeakMap { value, .. }) => type_may_hold_fn(*value, emit_env, seen, depth),
        Some(Type::Struct { id }) => {
            if seen.contains(id) {
                return false;
            }
            seen.push(*id);
            let holds = match struct_fields(emit_env, *id) {
                Some(fields) => fields
                    .iter()
                    .any(|field| type_may_hold_fn(field.ty, emit_env, seen, depth)),
                None => true,
            };
            seen.pop();
            holds
        }
        Some(Type::Fn { .. } | Type::Named { .. } | Type::GenericParam { .. } | Type::Error)
        | None => true,
        _ => false,
    }
}

pub(super) fn ty_may_hold_fn(ty: TypeId, emit_env: &EmitEnv) -> bool {
    type_may_hold_fn(ty, emit_env, &mut Vec::new(), 8)
}

pub(super) fn array_from_mapped_can_forward_element(
    result_ty: TypeId,
    source: &MirExpr,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    let Some(result_element) = array_element_of(result_ty, emit_env.types) else {
        return false;
    };
    let Some(source_element) = expr_array_element_ty(source, emit_env, body_ctx) else {
        return false;
    };
    type_contains_type(result_element, source_element, emit_env, 8)
}

pub(super) fn as_promise_type(ty: TypeId, emit_env: &EmitEnv) -> Option<(TypeId, TypeId)> {
    match emit_env.types.resolve(ty) {
        Some(Type::Promise { ok, .. }) => Some((ty, *ok)),
        _ => None,
    }
}

pub(super) fn value_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TypeId> {
    match expr {
        MirExpr::Call { callee, .. } => {
            let mir_ty = expr_base_ty(expr, body_ctx);
            match mir_ty.and_then(|t| emit_env.types.resolve(t)) {
                Some(Type::Optional { .. }) => mir_ty,
                _ => ctx.function_ret(*callee).or(mir_ty),
            }
        }
        MirExpr::IndirectCall { callee, .. } => {
            let callee_ty = expr_base_ty(callee, body_ctx)?;
            match emit_env.types.resolve(callee_ty) {
                Some(Type::Fn { ret, .. }) => Some(*ret),
                Some(Type::Optional { inner, .. })
                    if matches!(emit_env.types.resolve(*inner), Some(Type::Fn { .. })) =>
                {
                    Some(callee_ty)
                }
                _ => expr_base_ty(expr, body_ctx),
            }
        }
        MirExpr::Global(name) => ctx.global_ty(name).or_else(|| expr_base_ty(expr, body_ctx)),
        MirExpr::Unary {
            op: UnaryOp::Neg,
            expr: operand,
            ..
        } => value_ty(operand, ctx, emit_env, body_ctx),
        MirExpr::Unary {
            op: UnaryOp::BitNot,
            expr: operand,
            ..
        } => match value_ty(operand, ctx, emit_env, body_ctx)
            .and_then(|ty| emit_env.types.resolve(ty).map(|t| (ty, t)))
        {
            Some((ty, t)) if is_integer_type(t) => Some(ty),
            _ => expr_base_ty(expr, body_ctx),
        },
        MirExpr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod,
            left,
            right,
            ..
        } => numeric_binary_result_ty(
            value_ty(left, ctx, emit_env, body_ctx),
            value_ty(right, ctx, emit_env, body_ctx),
            emit_env,
        )
        .or_else(|| expr_base_ty(expr, body_ctx)),
        MirExpr::Field { base, field, .. } => value_ty(base, ctx, emit_env, body_ctx)
            .and_then(|base_ty| struct_field_type(emit_env, base_ty, *field))
            .or_else(|| expr_base_ty(expr, body_ctx)),
        _ => expr_base_ty(expr, body_ctx),
    }
}

pub(super) fn type_requires_arena(ty: TypeId, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    let EmitEnv { types, .. } = emit_env;
    match types.resolve(ty) {
        Some(Type::String | Type::Array { .. }) => true,
        Some(Type::Struct { id }) => ctx.struct_needs_lifetime(*id),
        Some(Type::Named { .. }) => types
            .struct_id(ty)
            .is_some_and(|id| ctx.struct_needs_lifetime(id)),
        Some(Type::Promise { ok, .. }) => type_requires_arena(*ok, ctx, emit_env),
        Some(Type::Result { ok, err }) => {
            type_requires_arena(*ok, ctx, emit_env) || type_requires_arena(*err, ctx, emit_env)
        }
        Some(
            Type::Optional { inner, .. }
            | Type::Generator { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner },
        ) => type_requires_arena(*inner, ctx, emit_env),
        Some(Type::WeakMap { value, .. }) => type_requires_arena(*value, ctx, emit_env),
        Some(Type::Fn { .. }) => ctx.fn_value_kind(ty).is_some(),
        _ => false,
    }
}

fn struct_field_type(emit_env: &EmitEnv, ty: TypeId, field: FieldId) -> Option<TypeId> {
    let id = emit_env.types.struct_id(ty)?;
    struct_field_decl_ty(emit_env, id, field)
}

pub(super) fn struct_field_decl_ty(
    emit_env: &EmitEnv,
    struct_id: StructId,
    field: FieldId,
) -> Option<TypeId> {
    emit_env.program.declarations.iter().find_map(|d| match d {
        MirDecl::Struct(s) if s.id == struct_id => {
            s.fields.iter().find(|f| f.id == field).map(|f| f.ty)
        }
        _ => None,
    })
}

pub(super) fn optional_chain_inner_ty(emit_env: &EmitEnv, base: &MirExpr) -> Option<TypeId> {
    let MirExpr::OptionalChain { ty, .. } = base else {
        return None;
    };
    match emit_env.types.resolve(*ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => Some(*inner),
        _ => None,
    }
}

pub(super) fn is_copy_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.resolve(ty) {
        Some(
            Type::Bool
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Date
            | Type::Symbol,
        ) => true,
        Some(Type::Optional { inner, .. }) => is_copy_type(types, *inner),
        _ => false,
    }
}

pub(super) fn is_display_supported_reason(reason_ty: Option<&Type>) -> bool {
    matches!(
        reason_ty,
        Some(Type::I32 | Type::I64 | Type::F64 | Type::String | Type::Bool)
    )
}

pub(super) fn is_function_value(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    if let MirExpr::Global(name) = expr {
        if ctx.fn_value_for_name(name).is_some() {
            return true;
        }
        if ctx.global_ty(name).is_none() && ctx.is_function_symbol(name) {
            return true;
        }
    }
    matches!(
        value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)),
        Some(Type::Fn { .. })
    )
}

pub(super) fn is_dynamic_expr(expr: &MirExpr, body_ctx: &BodyCtx) -> bool {
    match expr {
        MirExpr::Local(id) => body_ctx.is_dynamic(*id),
        MirExpr::Global(name) => body_ctx.is_dynamic_name(name),
        _ => false,
    }
}

pub(super) fn is_float_type(ty: &Type) -> bool {
    matches!(ty, Type::F32 | Type::F64)
}

pub(super) fn is_integer_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::U8 | Type::U16 | Type::U32 | Type::U64
    )
}

pub(super) fn is_numeric_type(ty: &Type) -> bool {
    is_float_type(ty) || is_integer_type(ty)
}

pub(super) fn is_numeric_primitive_target(ty: Option<&Type>) -> bool {
    matches!(
        ty,
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
