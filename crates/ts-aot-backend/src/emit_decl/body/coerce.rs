use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::literals::{emit_float_from_int, emit_whole_number_literal};
use super::super::types::emit_type_id_with_ctx;
use super::equality::{SourceNullish, is_nullish_literal, source_nullish_kind};
use super::equality_optional::optional_operand_kinds;
use super::expr::emit_value_expr;
use super::helpers::{is_dynamic_expr, is_float_type, is_integer_type, is_numeric_type, value_ty};
use super::place::is_non_copy_place;
use crate::error::BackendError;
use proc_macro2::{Literal, TokenStream};
use quote::quote;
use ts_aot_core::{Type, TypeId, TypeTable};
use ts_aot_ir_mir::{BinaryOp, MirExpr, UnaryOp};

pub(super) fn emit_owned_value(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let tokens = emit_value_expr(expr, ctx, emit_env, body_ctx)?;
    if is_non_copy_place(expr, emit_env, body_ctx) {
        Ok(quote!(#tokens.clone()))
    } else {
        Ok(tokens)
    }
}

pub(super) fn emit_owned_exprs(
    exprs: &[MirExpr],
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    exprs
        .iter()
        .map(|e| emit_owned_value(e, ctx, emit_env, body_ctx))
        .collect()
}

pub(super) fn emit_owned_exprs_coerced(
    exprs: &[MirExpr],
    param_tys: Option<&[TypeId]>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    exprs
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let tokens = emit_owned_value(e, ctx, emit_env, body_ctx)?;
            let Some(target) = param_tys.and_then(|tys| tys.get(i).copied()) else {
                return Ok(tokens);
            };
            let tokens = optional_coerce(e, tokens, target, ctx, emit_env, body_ctx, false)?;
            Ok(numeric_coerce_exact(
                e, tokens, target, ctx, emit_env, body_ctx,
            ))
        })
        .collect()
}

fn float_width(ty: &Type) -> Option<u8> {
    match ty {
        Type::F32 => Some(32),
        Type::F64 => Some(64),
        _ => None,
    }
}

fn integer_width(ty: &Type) -> Option<(bool, u8)> {
    match ty {
        Type::I8 => Some((true, 8)),
        Type::I16 => Some((true, 16)),
        Type::I32 => Some((true, 32)),
        Type::I64 => Some((true, 64)),
        Type::U8 => Some((false, 8)),
        Type::U16 => Some((false, 16)),
        Type::U32 => Some((false, 32)),
        Type::U64 => Some((false, 64)),
        _ => None,
    }
}

pub(super) fn existing_f64_id(types: &TypeTable) -> Option<TypeId> {
    types
        .types()
        .iter()
        .position(|ty| matches!(ty, Type::F64))
        .and_then(|index| u32::try_from(index).ok())
        .map(TypeId::from_raw)
}

fn common_numeric_id(
    types: &TypeTable,
    left_id: TypeId,
    left: &Type,
    right_id: TypeId,
    right: &Type,
) -> Option<TypeId> {
    if !is_numeric_type(left) || !is_numeric_type(right) {
        return None;
    }
    if let Some(left_width) = float_width(left) {
        return Some(match float_width(right) {
            Some(right_width) if right_width > left_width => right_id,
            _ => left_id,
        });
    }
    if float_width(right).is_some() {
        return Some(right_id);
    }
    let (left_signed, left_width) = integer_width(left)?;
    let (right_signed, right_width) = integer_width(right)?;
    if left_signed == right_signed {
        return Some(if left_width >= right_width {
            left_id
        } else {
            right_id
        });
    }
    let (signed_id, signed_width, unsigned_width) = if left_signed {
        (left_id, left_width, right_width)
    } else {
        (right_id, right_width, left_width)
    };
    if signed_width > unsigned_width {
        return Some(signed_id);
    }
    existing_f64_id(types).or(Some(if left_width >= right_width {
        left_id
    } else {
        right_id
    }))
}

pub(super) fn common_numeric_target(
    left: Option<TypeId>,
    right: Option<TypeId>,
    emit_env: &EmitEnv,
) -> Option<TypeId> {
    let (left_id, left_ty) = (left?, emit_env.types.resolve(left?)?);
    let (right_id, right_ty) = (right?, emit_env.types.resolve(right?)?);
    common_numeric_id(emit_env.types, left_id, left_ty, right_id, right_ty)
}

pub(super) fn numeric_binary_result_ty(
    left: Option<TypeId>,
    right: Option<TypeId>,
    emit_env: &EmitEnv,
) -> Option<TypeId> {
    common_numeric_target(left, right, emit_env)
}

pub(super) fn bitwise_operand_ty(
    expr: &MirExpr,
    other: Option<TypeId>,
    emit_env: &EmitEnv,
) -> Option<TypeId> {
    if !is_js_bitwise_expr(expr) {
        return None;
    }
    let other_is_numeric = other
        .and_then(|ty| emit_env.types.resolve(ty))
        .is_some_and(is_numeric_type);
    if other_is_numeric {
        other
    } else {
        existing_f64_id(emit_env.types)
    }
}

pub(super) fn numeric_expr_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TypeId> {
    if let Some(ty) = value_ty(expr, ctx, emit_env, body_ctx)
        && emit_env.types.resolve(ty).is_some_and(is_numeric_type)
    {
        return Some(ty);
    }
    if is_js_bitwise_expr(expr) {
        return existing_f64_id(emit_env.types);
    }
    if let MirExpr::Unary {
        op: UnaryOp::Neg,
        expr: operand,
        ..
    } = expr
    {
        return numeric_expr_ty(operand, ctx, emit_env, body_ctx);
    }
    let MirExpr::Binary {
        op, left, right, ..
    } = expr
    else {
        return None;
    };
    if !matches!(
        op,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
    ) {
        return None;
    }
    let left_base = numeric_expr_ty(left, ctx, emit_env, body_ctx);
    let right_base = numeric_expr_ty(right, ctx, emit_env, body_ctx);
    let left_ty = bitwise_operand_ty(left, right_base, emit_env).or(left_base);
    let right_ty = bitwise_operand_ty(right, left_base, emit_env).or(right_base);
    if matches!(op, BinaryOp::Div) && (is_js_bitwise_expr(left) || is_js_bitwise_expr(right)) {
        return existing_f64_id(emit_env.types)
            .or_else(|| common_numeric_target(left_ty, right_ty, emit_env));
    }
    common_numeric_target(left_ty, right_ty, emit_env)
}

pub(super) fn dynamic_string_coerce(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    if is_dynamic_expr(expr, body_ctx)
        && matches!(emit_env.types.resolve(target), Some(Type::String))
    {
        quote!(#tokens.to_js_string(__ctx))
    } else {
        tokens
    }
}

fn optional_string_coerce(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    owned: bool,
) -> TokenStream {
    let is_optional_string = matches!(
        emit_env.types.resolve(target),
        Some(Type::Optional { inner, .. }) if matches!(emit_env.types.resolve(*inner), Some(Type::String))
    );
    if !is_optional_string {
        return tokens;
    }
    match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
        Some(Type::Optional { .. }) if owned => quote!(#tokens.map(|__s| __s.to_string_lossy())),
        Some(Type::Optional { .. }) => tokens,
        Some(Type::String) if owned => {
            quote!(::core::option::Option::Some(#tokens.to_string_lossy()))
        }
        Some(Type::String) => quote!(::core::option::Option::Some(#tokens)),
        _ if owned => quote!(#tokens.map(|__s| __s.to_string_lossy())),
        _ => tokens,
    }
}

fn evaluate_then_replacement(
    expr: &MirExpr,
    tokens: &TokenStream,
    replacement: TokenStream,
) -> TokenStream {
    if is_nullish_literal(expr) {
        return replacement;
    }
    quote!({
        let _ = #tokens;
        #replacement
    })
}

pub(super) fn optional_coerce(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    owned: bool,
) -> Result<TokenStream, BackendError> {
    let Some(Type::Optional { inner, .. }) = emit_env.types.resolve(target) else {
        return Ok(tokens);
    };
    let inner = *inner;
    if matches!(emit_env.types.resolve(inner), Some(Type::Optional { .. })) {
        if matches!(expr, MirExpr::Unit) {
            return Ok(quote!(::core::option::Option::None));
        }
        if matches!(expr, MirExpr::Null { .. }) {
            return Ok(quote!(::core::option::Option::Some(
                ::core::option::Option::None
            )));
        }
        match source_nullish_kind(expr, ctx, emit_env, body_ctx) {
            Some(SourceNullish::Null) => {
                let replacement =
                    quote!(::core::option::Option::Some(::core::option::Option::None));
                return Ok(evaluate_then_replacement(expr, &tokens, replacement));
            }
            Some(SourceNullish::Undefined) => {
                let replacement = quote!(::core::option::Option::None);
                return Ok(evaluate_then_replacement(expr, &tokens, replacement));
            }
            None => {}
        }
        if matches!(
            value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)),
            Some(Type::Optional { inner: value, .. })
                if matches!(emit_env.types.resolve(*value), Some(Type::Optional { .. }))
        ) {
            return Ok(tokens);
        }
        if let Some(kinds) = optional_operand_kinds(expr, ctx, emit_env, body_ctx) {
            if kinds.null && kinds.undefined {
                return Err(BackendError::NotImplemented(
                    "a null-or-undefined optional argument cannot select a nullable parameter default unambiguously",
                ));
            }
            if kinds.undefined {
                let value = optional_coerce(expr, tokens, inner, ctx, emit_env, body_ctx, owned)?;
                return Ok(quote!((#value).map(::core::option::Option::Some)));
            }
        }
        let value = optional_coerce(expr, tokens, inner, ctx, emit_env, body_ctx, owned)?;
        return Ok(quote!(::core::option::Option::Some(#value)));
    }
    if is_nullish_literal(expr) || source_nullish_kind(expr, ctx, emit_env, body_ctx).is_some() {
        let replacement = quote!(::core::option::Option::None);
        return Ok(evaluate_then_replacement(expr, &tokens, replacement));
    }
    if matches!(emit_env.types.resolve(inner), Some(Type::String)) {
        return Ok(optional_string_coerce(
            expr, tokens, target, ctx, emit_env, body_ctx, owned,
        ));
    }
    let source_is_optional = matches!(
        value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)),
        Some(Type::Optional { .. })
    );
    if source_is_optional {
        let source_inner = value_ty(expr, ctx, emit_env, body_ctx)
            .and_then(|ty| emit_env.types.resolve(ty))
            .and_then(|ty| match ty {
                Type::Optional {
                    inner: source_inner,
                    ..
                } => Some(*source_inner),
                _ => None,
            });
        let Some(source_inner) = source_inner else {
            return Ok(tokens);
        };
        if !numeric_requires_cast(source_inner, inner, emit_env) {
            return Ok(tokens);
        }
        let dst_tokens = emit_type_id_with_ctx(inner, emit_env, ctx);
        return Ok(quote!((#tokens).map(|__value| ((__value) as #dst_tokens))));
    }
    let payload = numeric_coerce(expr, tokens, inner, ctx, emit_env, body_ctx);
    let float_width_mismatch = matches!(
        (
            value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)),
            emit_env.types.resolve(inner),
        ),
        (Some(source), Some(target))
            if is_float_type(source) && is_float_type(target) && source != target
    );
    if float_width_mismatch {
        let dst_tokens = emit_type_id_with_ctx(inner, emit_env, ctx);
        return Ok(quote!(::core::option::Option::Some((#payload) as #dst_tokens)));
    }
    Ok(quote!(::core::option::Option::Some(#payload)))
}

fn numeric_requires_cast(source: TypeId, target: TypeId, emit_env: &EmitEnv) -> bool {
    if source == target {
        return false;
    }
    let (Some(source), Some(target)) = (
        emit_env.types.resolve(source),
        emit_env.types.resolve(target),
    ) else {
        return false;
    };
    is_numeric_type(source) && is_numeric_type(target)
}

pub(super) fn numeric_coerce(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    numeric_coerce_impl(expr, tokens, target, ctx, emit_env, body_ctx, false)
}

pub(super) fn numeric_coerce_exact(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    numeric_coerce_impl(expr, tokens, target, ctx, emit_env, body_ctx, true)
}

pub(super) fn numeric_coerce_tokens(
    tokens: TokenStream,
    source: &Type,
    target: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> TokenStream {
    let Some(target_id) = target else {
        return tokens;
    };
    let Some(dst) = emit_env.types.resolve(target_id) else {
        return tokens;
    };
    if let Type::Optional { inner, .. } = dst {
        let inner = *inner;
        let Some(inner_ty) = emit_env.types.resolve(inner) else {
            return tokens;
        };
        if !is_numeric_type(source) || !is_numeric_type(inner_ty) {
            return tokens;
        }
        let payload = if source == inner_ty {
            tokens
        } else {
            let inner_tokens = emit_type_id_with_ctx(inner, emit_env, ctx);
            quote!((#tokens) as #inner_tokens)
        };
        return quote!(::core::option::Option::Some(#payload));
    }
    if is_numeric_type(source) && is_numeric_type(dst) && source != dst {
        let dst_tokens = emit_type_id_with_ctx(target_id, emit_env, ctx);
        quote!(((#tokens) as #dst_tokens))
    } else {
        tokens
    }
}

fn numeric_coerce_impl(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    cast_int_widths: bool,
) -> TokenStream {
    let dst = emit_env.types.resolve(target);
    match expr {
        MirExpr::Int { value, .. } => match dst {
            Some(t) if is_float_type(t) => emit_float_from_int(*value),
            Some(t) if is_integer_type(t) => emit_whole_number_literal(*value),
            _ => tokens,
        },
        MirExpr::Unary {
            op: UnaryOp::Neg,
            expr: operand,
            ..
        } => match (operand.as_ref(), dst) {
            (MirExpr::Int { value, .. }, Some(t)) if is_float_type(t) => {
                let inner = emit_float_from_int(*value);
                quote!(-(#inner))
            }
            (MirExpr::Float { .. }, Some(t)) if is_integer_type(t) => {
                let dst_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
                quote!(((#tokens) as #dst_tokens))
            }
            _ => numeric_coerce_from_source_ty(
                operand,
                tokens,
                target,
                dst,
                ctx,
                emit_env,
                body_ctx,
                cast_int_widths,
            ),
        },
        MirExpr::Float { .. } if matches!(dst, Some(t) if is_integer_type(t)) => {
            let dst_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
            quote!(((#tokens) as #dst_tokens))
        }
        _ => numeric_coerce_from_source_ty(
            expr,
            tokens,
            target,
            dst,
            ctx,
            emit_env,
            body_ctx,
            cast_int_widths,
        ),
    }
}

pub(super) fn is_js_bitwise_expr(expr: &MirExpr) -> bool {
    matches!(
        expr,
        MirExpr::Unary {
            op: UnaryOp::BitNot,
            ..
        } | MirExpr::Binary {
            op: BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr,
            ..
        }
    )
}

fn numeric_coerce_from_source_ty(
    value: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    dst: Option<&Type>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    cast_int_widths: bool,
) -> TokenStream {
    if is_js_bitwise_expr(value) {
        let Some(dst) = dst else {
            return tokens;
        };
        if is_numeric_type(dst) && !matches!(dst, Type::I32) {
            let dst_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
            return quote!(((#tokens) as #dst_tokens));
        }
        return tokens;
    }
    let Some(src) = numeric_expr_ty(value, ctx, emit_env, body_ctx) else {
        return tokens;
    };
    let (Some(src), Some(dst)) = (emit_env.types.resolve(src), dst) else {
        return tokens;
    };
    let both_numeric = (is_float_type(src) || is_integer_type(src))
        && (is_float_type(dst) || is_integer_type(dst));
    let width_mismatch = cast_int_widths && src != dst;
    if both_numeric && (is_float_type(src) != is_float_type(dst) || width_mismatch) {
        let dst_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
        quote!(((#tokens) as #dst_tokens))
    } else {
        tokens
    }
}

pub(crate) fn emit_float(value: f64) -> TokenStream {
    if value.is_nan() {
        quote!(f64::NAN)
    } else if value.is_infinite() && value.is_sign_positive() {
        quote!(f64::INFINITY)
    } else if value.is_infinite() {
        quote!(f64::NEG_INFINITY)
    } else {
        let literal = Literal::f64_unsuffixed(value);
        quote!(#literal)
    }
}
