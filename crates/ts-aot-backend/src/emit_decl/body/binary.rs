use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::coerce::{
    bitwise_operand_ty, common_numeric_target, existing_f64_id, is_js_bitwise_expr,
    numeric_coerce_exact, numeric_expr_ty,
};
use super::equality::emit_nullish_literal_equality;
use super::equality_optional::{
    check_optional_strict_equality_inner_types, check_optional_strict_equality_kinds,
    emit_dynamic_equality, emit_mixed_optional_equality,
};
use super::expr::emit_expr;
use super::helpers::{
    is_function_value, is_integer_type, is_numeric_primitive_target, is_numeric_type, value_ty,
};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{BinaryOp, MirExpr, MirPlace, MirPlaceBase, UnaryOp};

pub(super) fn optional_chain_map_arm(
    base: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let MirExpr::OptionalChain { base: inner, ty } = base else {
        return Ok(None);
    };
    let Some(resolved) = types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!(#inner_tokens.as_ref())))
}

pub(super) fn emit_optional_chain_assign(
    target: &MirPlace,
    value: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let MirPlace::Field { base, field, .. } = target else {
        return Ok(None);
    };
    let MirPlaceBase::Chain {
        base: chain_base, ..
    } = base.as_ref()
    else {
        return Ok(None);
    };
    let MirExpr::OptionalChain { base: inner, ty } = chain_base.as_ref() else {
        return Ok(None);
    };
    let Some(resolved) = types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    let inner_ty = match types.resolve(*ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
        _ => {
            return Err(BackendError::NotImplemented(
                "optional field assign: target not Optional",
            ));
        }
    };
    let field_ident = match types.resolve(inner_ty) {
        Some(_) => {
            let struct_id = types
                .struct_id(inner_ty)
                .ok_or(BackendError::NotImplemented(
                    "optional field assign: base is not a struct",
                ))?;
            ctx.struct_field_ident(struct_id, *field)
        }
        None => {
            return Err(BackendError::NotImplemented(
                "optional field assign: inner type unresolved",
            ));
        }
    };
    let value = emit_expr(value, ctx, emit_env, body_ctx)?;
    Ok(Some(quote! {
        if #inner_tokens.is_some() {
            (*#inner_tokens.as_mut().unwrap().#field_ident.borrow_mut()) = #value;
        }
    }))
}

fn is_bitwise_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
    )
}

fn is_unknown_numeric_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Error | Type::Union { .. } | Type::Intersection { .. } | Type::GenericParam { .. }
    )
}

fn js_int32_from_float(value: &TokenStream) -> TokenStream {
    quote!({
        let __ts_aot_bitwise_value: f64 = #value;
        if __ts_aot_bitwise_value.is_finite() {
            let __ts_aot_bitwise_wrapped =
                __ts_aot_bitwise_value.trunc().rem_euclid(4294967296.0);
            let __ts_aot_bitwise_signed = if __ts_aot_bitwise_wrapped >= 2147483648.0 {
                __ts_aot_bitwise_wrapped - 4294967296.0
            } else {
                __ts_aot_bitwise_wrapped
            };
            __ts_aot_bitwise_signed as i32
        } else {
            0i32
        }
    })
}

fn js_int32_operand(
    expr: &MirExpr,
    tokens: TokenStream,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if matches!(expr, MirExpr::Unit | MirExpr::Null { .. }) {
        return Ok(quote!({
            let _ = #tokens;
            0i32
        }));
    }
    if is_js_bitwise_expr(expr) {
        return Ok(tokens);
    }
    let declared = value_ty(expr, ctx, emit_env, body_ctx)
        .and_then(|ty| emit_env.types.resolve(ty).map(|resolved| (ty, resolved)));
    Ok(match declared {
        Some((_, Type::Void | Type::Null)) => quote!({
            let _ = #tokens;
            0i32
        }),
        Some((_, Type::Bool)) => quote!((#tokens) as i32),
        Some((_, ty)) if !is_numeric_type(ty) && !is_unknown_numeric_type(ty) => {
            return Err(BackendError::NotImplemented(
                "a bitwise operator needs numeric operands: JavaScript applies ToInt32 to both \
                 operands, and this AOT target has no runtime ToNumber conversion for this \
                 operand type, so the operation cannot be lowered without guessing the value; \
                 convert the operand to a number first",
            ));
        }
        _ => {
            let resolved = numeric_expr_ty(expr, ctx, emit_env, body_ctx)
                .and_then(|ty| emit_env.types.resolve(ty).map(|resolved| (ty, resolved)));
            match resolved {
                Some((_, Type::I32)) => tokens,
                Some((ty, ty_kind)) if is_integer_type(ty_kind) => {
                    let int_tokens = emit_type_id_with_ctx(ty, emit_env, ctx);
                    let converted = quote!(((#tokens) as #int_tokens) as f64);
                    js_int32_from_float(&converted)
                }
                Some(_) => {
                    let converted = quote!((#tokens) as f64);
                    js_int32_from_float(&converted)
                }
                None => quote!((#tokens) as i32),
            }
        }
    })
}

pub(super) fn emit_binary_expr(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) = emit_dynamic_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) =
        emit_nullish_literal_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) =
        emit_mixed_optional_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    check_optional_strict_equality_kinds(op, left, right, ctx, emit_env, body_ctx)?;
    check_optional_strict_equality_inner_types(op, left, right, ctx, emit_env, body_ctx)?;
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && [left, right]
        .iter()
        .any(|side| is_function_value(side, ctx, emit_env, body_ctx))
    {
        return Err(BackendError::NotImplemented(
            "function value equality: the AOT target represents a function value as a generated \
             enum without a stable identity, so `==`/`===` on function values cannot be lowered \
             without guessing; compare the results of calling them instead",
        ));
    }
    let target = if matches!(
        op,
        BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::StrictEq
            | BinaryOp::StrictNe
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
    ) {
        let left_base = numeric_expr_ty(left, ctx, emit_env, body_ctx);
        let right_base = numeric_expr_ty(right, ctx, emit_env, body_ctx);
        let left_ty = bitwise_operand_ty(left, right_base, emit_env).or(left_base);
        let right_ty = bitwise_operand_ty(right, left_base, emit_env).or(right_base);
        if matches!(op, BinaryOp::Div) && (is_js_bitwise_expr(left) || is_js_bitwise_expr(right)) {
            existing_f64_id(emit_env.types)
                .or_else(|| common_numeric_target(left_ty, right_ty, emit_env))
        } else {
            common_numeric_target(left_ty, right_ty, emit_env)
        }
    } else {
        None
    };
    let mut left_tokens = emit_expr(left, ctx, emit_env, body_ctx)?;
    let mut right_tokens = emit_expr(right, ctx, emit_env, body_ctx)?;
    if let Some(t) = target {
        left_tokens = numeric_coerce_exact(left, left_tokens, t, ctx, emit_env, body_ctx);
        right_tokens = numeric_coerce_exact(right, right_tokens, t, ctx, emit_env, body_ctx);
    }
    if is_bitwise_op(op) {
        left_tokens = js_int32_operand(left, left_tokens, ctx, emit_env, body_ctx)?;
        right_tokens = js_int32_operand(right, right_tokens, ctx, emit_env, body_ctx)?;
    }
    Ok(match op {
        BinaryOp::Add => quote!(#left_tokens + #right_tokens),
        BinaryOp::Sub => quote!(#left_tokens - #right_tokens),
        BinaryOp::Mul => quote!(#left_tokens * #right_tokens),
        BinaryOp::Div => quote!(#left_tokens / #right_tokens),
        BinaryOp::Mod => quote!(#left_tokens % #right_tokens),
        BinaryOp::Eq | BinaryOp::StrictEq => quote!(#left_tokens == #right_tokens),
        BinaryOp::Ne | BinaryOp::StrictNe => quote!(#left_tokens != #right_tokens),
        BinaryOp::Lt => quote!(#left_tokens < #right_tokens),
        BinaryOp::Le => quote!(#left_tokens <= #right_tokens),
        BinaryOp::Gt => quote!(#left_tokens > #right_tokens),
        BinaryOp::Ge => quote!(#left_tokens >= #right_tokens),
        BinaryOp::And => quote!(#left_tokens && #right_tokens),
        BinaryOp::Or => quote!(#left_tokens || #right_tokens),
        BinaryOp::BitAnd => quote!(#left_tokens & #right_tokens),
        BinaryOp::BitOr => quote!(#left_tokens | #right_tokens),
        BinaryOp::BitXor => quote!(#left_tokens ^ #right_tokens),
        BinaryOp::Shl => quote!((#left_tokens).wrapping_shl((#right_tokens) as u32)),
        BinaryOp::Shr => quote!((#left_tokens).wrapping_shr((#right_tokens) as u32)),
    })
}

pub(super) fn emit_unary_expr(
    op: UnaryOp,
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let tokens = emit_expr(expr, ctx, emit_env, body_ctx)?;
    Ok(match op {
        UnaryOp::Neg => quote!(-(#tokens)),
        UnaryOp::Not => quote!(!(#tokens)),
        UnaryOp::BitNot => {
            let tokens = js_int32_operand(expr, tokens, ctx, emit_env, body_ctx)?;
            quote!(!(#tokens))
        }
    })
}

pub(super) fn emit_cast_expr(
    expr: &MirExpr,
    ty: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
    if !is_numeric_primitive_target(types.resolve(ty)) {
        return Err(BackendError::Internal(format!(
            "MirExpr::Cast target TypeId({}) is not a numeric primitive; \
             Rust's `as` cast cannot lower it. \
             Only I8/I16/I32/I64/U8/U16/U32/U64/F32/F64 are supported as cast targets.",
            ty.raw()
        )));
    }
    let cast_ty = emit_type_id_with_ctx(ty, emit_env, ctx);
    Ok(quote!(#inner as #cast_ty))
}
