use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::super::types::emit_type_id_with_ctx;
use super::super::call::emit_array_slice_arg;
use super::super::coerce::{emit_owned_value, numeric_coerce, optional_coerce};
use super::super::equality::index_of_element_is_comparable;
use super::super::expr::emit_expr;
use super::super::helpers::{is_float_type, is_integer_type, value_ty};
use super::arrays::array_element_ty;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{MirExpr, RuntimeOp};

pub(super) fn emit_array_index_of_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    _target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match op {
        RuntimeOp::ArrayIndexOf => {
            let element_ty_id = match array_element_ty(&args[0], ctx, emit_env, body_ctx) {
                Some(element) => element,
                None => ty,
            };
            if !index_of_element_is_comparable(element_ty_id, emit_env) {
                return Err(BackendError::NotImplemented(
                    "Array.prototype.indexOf() requires a primitive element type: the AOT target \
                         compares elements with Rust `PartialEq`, which a class instance or an array \
                         element does not implement, while JavaScript compares object elements by \
                         identity. Search for a primitive key, or compare a scalar field of the \
                         elements",
                ));
            }
            let element_is_optional =
                matches!(types.resolve(element_ty_id), Some(Type::Optional { .. }));
            let element_ty = emit_type_id_with_ctx(element_ty_id, emit_env, ctx);
            let arr = emit_array_slice_arg(&args[0], ctx, emit_env, body_ctx)?;
            let raw_needle = emit_owned_value(&args[1], ctx, emit_env, body_ctx)?;
            let needle_ty = value_ty(&args[1], ctx, emit_env, body_ctx);
            let needle_is_optional = needle_ty
                .is_some_and(|ty| matches!(types.resolve(ty), Some(Type::Optional { .. })));
            let needle_scalar_ty = match needle_ty.and_then(|ty| types.resolve(ty)) {
                Some(Type::Optional { inner, .. }) => Some(*inner),
                _ => needle_ty,
            };
            let needle_is_float = needle_scalar_ty
                .and_then(|ty| emit_env.types.resolve(ty))
                .is_some_and(is_float_type);
            let integer_element = match emit_env.types.resolve(element_ty_id) {
                Some(Type::Optional { inner, .. })
                    if emit_env.types.resolve(*inner).is_some_and(is_integer_type) =>
                {
                    Some(*inner)
                }
                Some(resolved) if is_integer_type(resolved) => Some(element_ty_id),
                _ => None,
            };
            let needle = numeric_coerce(
                &args[1],
                raw_needle.clone(),
                element_ty_id,
                ctx,
                emit_env,
                body_ctx,
            );
            let needle = optional_coerce(
                &args[1],
                needle,
                element_ty_id,
                ctx,
                emit_env,
                body_ctx,
                false,
            )?;
            let from = match args.get(2) {
                None | Some(MirExpr::Unit | MirExpr::Null { .. }) => quote!(0_i64),
                Some(from) => {
                    let tokens = emit_expr(from, ctx, emit_env, body_ctx)?;
                    let optional = value_ty(from, ctx, emit_env, body_ctx)
                        .is_some_and(|ty| matches!(types.resolve(ty), Some(Type::Optional { .. })));
                    if optional {
                        quote!((#tokens).map_or(0_i64, |from| from as i64))
                    } else {
                        quote!((#tokens) as i64)
                    }
                }
            };
            if !element_is_optional
                && value_ty(&args[1], ctx, emit_env, body_ctx)
                    .and_then(|ty| types.resolve(ty))
                    .is_some_and(|ty| matches!(ty, Type::Null | Type::Void))
            {
                return Ok(quote!({
                    let _ = #arr;
                    let _ = #from;
                    -1.0_f64
                }));
            }
            if let Some(integer_id) = integer_element
                && needle_is_float
            {
                if !needle_is_optional
                    && let MirExpr::Float { value, .. } = &args[1]
                    && value.fract() != 0.0
                {
                    return Ok(quote!({
                        let _ = #arr;
                        let _ = #from;
                        -1.0_f64
                    }));
                }
                let integer_ty = emit_type_id_with_ctx(integer_id, emit_env, ctx);
                let needle_binding = body_ctx.alloc_hoist_ident("__ts_index_needle");
                let arr_binding = body_ctx.alloc_hoist_ident("__ts_index_arr");
                let from_binding = body_ctx.alloc_hoist_ident("__ts_index_from");
                if needle_is_optional {
                    let value_binding = body_ctx.alloc_hoist_ident("__ts_index_digits");
                    let converted = if element_is_optional {
                        quote!(::core::option::Option::Some((#value_binding) as #integer_ty))
                    } else {
                        quote!((#value_binding) as #integer_ty)
                    };
                    let none_arm = if element_is_optional {
                        quote!(__ts_aot_array_index_of::<#element_ty>(
                            #arr_binding,
                            &::core::option::Option::None,
                            #from_binding,
                        ))
                    } else {
                        quote!(-1.0_f64)
                    };
                    return Ok(quote!({
                        let #arr_binding = #arr;
                        let #needle_binding = #needle;
                        let #from_binding = #from;
                        match #needle_binding {
                            ::core::option::Option::Some(#value_binding) => {
                                if (#value_binding).fract() == 0.0
                                    && ((#value_binding) as f64) >= <#integer_ty>::MIN as f64
                                    && ((#value_binding) as f64) < <#integer_ty>::MAX as f64 + 1.0
                                {
                                    __ts_aot_array_index_of::<#element_ty>(
                                        #arr_binding,
                                        &(#converted),
                                        #from_binding,
                                    )
                                } else {
                                    -1.0_f64
                                }
                            }
                            ::core::option::Option::None => #none_arm,
                        }
                    }));
                }
                let needle_value = if element_is_optional {
                    quote!(::core::option::Option::Some((#needle_binding) as #integer_ty))
                } else {
                    quote!((#needle_binding) as #integer_ty)
                };
                return Ok(quote!({
                    let #arr_binding = #arr;
                    let #needle_binding = (#raw_needle) as f64;
                    let #from_binding = #from;
                    if (#needle_binding).fract() == 0.0
                        && ((#needle_binding) as f64) >= <#integer_ty>::MIN as f64
                        && ((#needle_binding) as f64) < <#integer_ty>::MAX as f64 + 1.0
                    {
                        __ts_aot_array_index_of::<#element_ty>(
                            #arr_binding,
                            &(#needle_value),
                            #from_binding,
                        )
                    } else {
                        -1.0_f64
                    }
                }));
            }
            if !element_is_optional && needle_is_optional {
                let arr_binding = body_ctx.alloc_hoist_ident("__ts_index_arr");
                let needle_binding = body_ctx.alloc_hoist_ident("__ts_index_needle");
                let from_binding = body_ctx.alloc_hoist_ident("__ts_index_from");
                return Ok(quote!({
                    let #arr_binding = #arr;
                    let #needle_binding = #needle;
                    let #from_binding = #from;
                    match #needle_binding {
                        ::core::option::Option::Some(__ts_needle) =>
                            __ts_aot_array_index_of::<#element_ty>(
                                #arr_binding,
                                &__ts_needle,
                                #from_binding,
                            ),
                        ::core::option::Option::None => -1.0_f64,
                    }
                }));
            }
            Ok(quote!(__ts_aot_array_index_of::<#element_ty>(
                #arr, &#needle, #from
            )))
        }
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
