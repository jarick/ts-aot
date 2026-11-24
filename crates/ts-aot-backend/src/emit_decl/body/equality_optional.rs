use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::coerce::{emit_owned_value, numeric_coerce_exact};
use super::equality::{caught_operand_ty, emit_caught_literal_eq, is_rust_comparable_value};
use super::expr::emit_expr;
use super::helpers::{is_dynamic_expr, is_float_type, is_integer_type, is_numeric_type, value_ty};
use crate::error::BackendError;
use proc_macro2::{Literal, TokenStream};
use quote::quote;
use ts_aot_core::{NullishKinds, Type, TypeId};
use ts_aot_ir_mir::{BinaryOp, MirExpr};

pub(super) fn emit_mixed_optional_equality(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let neg = matches!(op, BinaryOp::Ne | BinaryOp::StrictNe);
    let optional_inner = |expr: &MirExpr| -> Option<TypeId> {
        match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
            Some(Type::Optional { inner, .. }) => Some(*inner),
            _ => None,
        }
    };
    let optional_is_left = optional_inner(left).is_some();
    let (optional_side, plain_side) = match (optional_is_left, optional_inner(right).is_some()) {
        (true, false) => (left, right),
        (false, true) => (right, left),
        _ => return Ok(None),
    };
    let Some(inner_ty) = optional_inner(optional_side) else {
        return Ok(None);
    };
    let plain_ty = value_ty(plain_side, ctx, emit_env, body_ctx);
    let inner_resolved = emit_env.types.resolve(inner_ty);
    let plain_resolved = plain_ty.and_then(|t| emit_env.types.resolve(t));
    let plain_matches_inner = match plain_side {
        MirExpr::Bool(_) => matches!(inner_resolved, Some(Type::Bool)),
        MirExpr::Int { .. } | MirExpr::Float { .. } => matches!(
            inner_resolved,
            Some(ty) if is_float_type(ty) || is_integer_type(ty)
        ),
        MirExpr::String { .. } => matches!(inner_resolved, Some(Type::String)),
        _ => match (plain_resolved, inner_resolved) {
            (Some(plain), Some(inner)) => {
                (inner == plain && is_rust_comparable_value(plain))
                    || (is_numeric_type(plain) && is_numeric_type(inner))
            }
            _ => false,
        },
    };
    if !plain_matches_inner {
        return Ok(None);
    }
    let comparison_ty = match (inner_resolved, plain_resolved) {
        (Some(inner), Some(plain)) if is_integer_type(inner) && is_float_type(plain) => {
            plain_ty.unwrap_or(inner_ty)
        }
        _ => inner_ty,
    };
    let optional = emit_expr(optional_side, ctx, emit_env, body_ctx)?;
    let plain = emit_owned_value(plain_side, ctx, emit_env, body_ctx)?;
    let plain = numeric_coerce_exact(plain_side, plain, comparison_ty, ctx, emit_env, body_ctx);
    let optional = match (inner_resolved, emit_env.types.resolve(comparison_ty)) {
        (Some(inner), Some(comparison)) if is_integer_type(inner) && is_float_type(comparison) => {
            let comparison = emit_type_id_with_ctx(comparison_ty, emit_env, ctx);
            quote!((#optional).map(|__ts_num| __ts_num as #comparison))
        }
        _ => optional,
    };
    let cmp = if optional_is_left {
        quote!(#optional.as_ref() == ::core::option::Option::Some(&(#plain)))
    } else {
        quote!(::core::option::Option::Some(&(#plain)) == #optional.as_ref())
    };
    Ok(Some(if neg { quote!(!(#cmp)) } else { cmp }))
}

pub(super) fn optional_operand_kinds(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<NullishKinds> {
    match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
        Some(Type::Optional { nullish, .. }) => Some(*nullish),
        _ => None,
    }
}

fn describe_nullish_kinds(kinds: NullishKinds) -> &'static str {
    match (kinds.null, kinds.undefined) {
        (true, true) => "`null | undefined`",
        (true, false) => "`null`",
        (false, true) => "`undefined`",
        (false, false) => "a non-nullish value",
    }
}

pub(super) fn check_optional_strict_equality_kinds(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<(), BackendError> {
    if !matches!(op, BinaryOp::StrictEq | BinaryOp::StrictNe) {
        return Ok(());
    }
    let (Some(left_kinds), Some(right_kinds)) = (
        optional_operand_kinds(left, ctx, emit_env, body_ctx),
        optional_operand_kinds(right, ctx, emit_env, body_ctx),
    ) else {
        return Ok(());
    };
    let empty = |kinds: NullishKinds| !kinds.null && !kinds.undefined;
    if empty(left_kinds) || empty(right_kinds) {
        return Ok(());
    }
    let op_name = if op == BinaryOp::StrictEq {
        "==="
    } else {
        "!=="
    };
    let both = |kinds: NullishKinds| kinds.null && kinds.undefined;
    if both(left_kinds) || both(right_kinds) {
        return Err(BackendError::UndecidableNullishComparison {
            code: "P0020",
            message: format!(
                "`{op_name}` compares two optional values, and one operand allows both `null` and \
                 `undefined`; the AOT target stores both as one `Option::None`, so the runtime value \
                 cannot decide the comparison (`null === undefined` is `false` in JavaScript while \
                 `None == None` is `true`). Narrow the operand to a single nullish kind (`T | null` \
                 or `T | undefined`), or use a presence test (`x == null` / `x != null`)"
            ),
        });
    }
    if left_kinds != right_kinds {
        return Err(BackendError::UndecidableNullishComparison {
            code: "P0019",
            message: format!(
                "`{op_name}` compares two optional values whose nullish kinds do not match ({} vs \
                 {}); the AOT target stores `null` and `undefined` as one `Option::None`, so the \
                 comparison cannot be decided (it would report `null === undefined` as `true`). \
                 Widen both operands to the same nullish kind, or compare each against its literal \
                 (`x === null` / `x === undefined`)",
                describe_nullish_kinds(left_kinds),
                describe_nullish_kinds(right_kinds)
            ),
        });
    }
    Ok(())
}

fn optional_operand_inner(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TypeId> {
    match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
        Some(Type::Optional { inner, .. }) => Some(*inner),
        _ => None,
    }
}

pub(super) fn check_optional_strict_equality_inner_types(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<(), BackendError> {
    if !matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) {
        return Ok(());
    }
    let (Some(left_inner), Some(right_inner)) = (
        optional_operand_inner(left, ctx, emit_env, body_ctx),
        optional_operand_inner(right, ctx, emit_env, body_ctx),
    ) else {
        return Ok(());
    };
    let (Some(left_ty), Some(right_ty)) = (
        emit_env.types.resolve(left_inner),
        emit_env.types.resolve(right_inner),
    ) else {
        return Ok(());
    };
    let op_name = match op {
        BinaryOp::StrictEq => "===",
        BinaryOp::StrictNe => "!==",
        BinaryOp::Eq => "==",
        _ => "!=",
    };
    if left_inner != right_inner {
        return Err(BackendError::UndecidableNullishComparison {
            code: "P0022",
            message: format!(
                "`{op_name}` compares two optional values whose inner types differ ({left_ty:?} vs \
                 {right_ty:?}); the AOT target compares them as one `Option<T>`, and the two Rust \
                 types do not match, so the comparison cannot be emitted. Compare the values in one \
                 numeric type (cast one operand first) or narrow both operands to the same `T | null`, \
                 or compare each against its literal (`x === null`)"
            ),
        });
    }
    if !is_rust_comparable_value(left_ty) {
        return Err(BackendError::UndecidableNullishComparison {
            code: "P0022",
            message: format!(
                "`{op_name}` compares two optional values whose inner type {left_ty:?} has no Rust \
                 `PartialEq` (a structure or an array); the AOT target would emit `Option<T> == \
                 Option<T>`, which does not compile. Compare a scalar field of the operands, or use a \
                 presence test (`x == null` / `x != null`)"
            ),
        });
    }
    Ok(())
}

pub(super) fn emit_dynamic_equality(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let left_dynamic = is_dynamic_expr(left, body_ctx);
    let right_dynamic = is_dynamic_expr(right, body_ctx);
    if !left_dynamic && !right_dynamic {
        return Ok(None);
    }
    let neg = matches!(op, BinaryOp::Ne | BinaryOp::StrictNe);
    if !left_dynamic || !right_dynamic {
        let (caught_expr, other_expr) = if left_dynamic {
            (left, right)
        } else {
            (right, left)
        };
        if let Some(call) =
            emit_caught_literal_eq(caught_expr, other_expr, ctx, emit_env, body_ctx)?
        {
            return Ok(Some(if neg { quote!(!(#call)) } else { call }));
        }
        let caught = emit_expr(caught_expr, ctx, emit_env, body_ctx)?;
        let call = if let Some(other_ty) = caught_operand_ty(other_expr, ctx, emit_env, body_ctx) {
            let other = emit_owned_value(other_expr, ctx, emit_env, body_ctx)?;
            quote!(ts_aot_runtime::__ts_aot_caught_eq::<#other_ty>(&#caught, #other))
        } else {
            let other = emit_expr(other_expr, ctx, emit_env, body_ctx)?;
            quote!(ts_aot_runtime::__ts_aot_caught_eq_dyn(&#caught, &#other))
        };
        return Ok(Some(if neg { quote!(!#call) } else { call }));
    }
    let left = emit_expr(left, ctx, emit_env, body_ctx)?;
    let right = emit_expr(right, ctx, emit_env, body_ctx)?;
    let call = quote!(#left == #right);
    Ok(Some(if neg { quote!(!(#call)) } else { call }))
}

fn typeof_of_optional_inner(ty: TypeId, emit_env: &EmitEnv) -> Option<&'static str> {
    match emit_env.types.resolve(ty) {
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
        ) => Some("number"),
        Some(Type::Bool) => Some("boolean"),
        Some(Type::String) => Some("string"),
        Some(Type::BigInt) => Some("bigint"),
        Some(Type::Symbol) => Some("symbol"),
        Some(
            Type::Struct { .. }
            | Type::Array { .. }
            | Type::Named { .. }
            | Type::Promise { .. }
            | Type::Date
            | Type::ArrayBuffer
            | Type::WeakMap { .. }
            | Type::Int8Array
            | Type::Uint8Array
            | Type::Uint8ClampedArray
            | Type::Int16Array
            | Type::Uint16Array
            | Type::Int32Array
            | Type::Uint32Array
            | Type::Float32Array
            | Type::Float64Array
            | Type::Result { .. }
            | Type::Tuple { .. }
            | Type::Generator { .. }
            | Type::GeneratorResult { .. }
            | Type::AsyncGenYield { .. },
        ) => Some("object"),
        _ => None,
    }
}

pub(super) fn emit_typeof(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    ctx.mark_runtime_used();
    match expr {
        MirExpr::Unit => Ok(quote!(ts_aot_runtime::JsString::from(
            __ts_aot_typeof_unit()
        ))),
        MirExpr::Null { .. } => Ok(quote!(ts_aot_runtime::JsString::from(
            __ts_aot_typeof_null()
        ))),
        _ => {
            if let Some(Type::Fn { .. }) =
                value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty))
            {
                if matches!(expr, MirExpr::Global(_)) {
                    return Ok(quote!(ts_aot_runtime::JsString::from("function")));
                }
                let evaluated = emit_expr(expr, ctx, emit_env, body_ctx)?;
                return Ok(quote!({
                    let _ = #evaluated;
                    ts_aot_runtime::JsString::from("function")
                }));
            }
            if let MirExpr::Global(name) = expr
                && (ctx.fn_value_for_name(name).is_some()
                    || (ctx.global_ty(name).is_none() && ctx.is_function_symbol(name)))
            {
                return Ok(quote!(ts_aot_runtime::JsString::from("function")));
            }
            if let Some(Type::Optional { inner, nullish }) = value_ty(expr, ctx, emit_env, body_ctx)
                .and_then(|ty| emit_env.types.resolve(ty))
                .cloned()
                && nullish.undefined
                && !nullish.null
                && let Some(some_typeof) = typeof_of_optional_inner(inner, emit_env)
            {
                let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
                let some_literal = Literal::string(some_typeof);
                return Ok(quote!(ts_aot_runtime::JsString::from(
                    if (#inner).is_none() {
                        "undefined"
                    } else {
                        #some_literal
                    }
                )));
            }
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(ts_aot_runtime::JsString::from(__ts_aot_typeof(&#inner))))
        }
    }
}
