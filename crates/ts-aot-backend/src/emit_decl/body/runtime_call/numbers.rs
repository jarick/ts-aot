use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::super::literals::emit_float_from_int;
use super::super::super::runtime_op::runtime_op_ident;
use super::super::expr::{emit_expr, emit_exprs};
use super::super::helpers::{is_integer_type, is_numeric_type, value_ty};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{MirExpr, RuntimeOp};

pub(super) fn emit_numbers_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    _ty: TypeId,
    _target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match op {
        RuntimeOp::MathMax | RuntimeOp::MathMin => {
            let name = runtime_op_ident(op)?;
            let args = emit_exprs(args, ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&[#(#args),*])))
        }
        RuntimeOp::BigIntNeg | RuntimeOp::BigIntNot | RuntimeOp::BigIntToNumber => {
            let name = runtime_op_ident(op)?;
            let arg = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&#arg)))
        }
        RuntimeOp::BigIntToString => {
            let name = runtime_op_ident(op)?;
            let owner = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            if args.len() == 1 {
                Ok(quote!(#name(&#owner, None)))
            } else {
                let radix = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
                Ok(quote!(#name(&#owner, Some(#radix))))
            }
        }
        RuntimeOp::NumberToString
        | RuntimeOp::NumberToFixed
        | RuntimeOp::NumberToExponential
        | RuntimeOp::NumberToPrecision => {
            let name = runtime_op_ident(op)?;
            let value = match &args[0] {
                MirExpr::Int { value, .. } => emit_float_from_int(*value),
                other => {
                    let other = emit_expr(other, ctx, emit_env, body_ctx)?;
                    quote!((#other) as f64)
                }
            };
            let argument = match args.get(1) {
                None | Some(MirExpr::Unit) => quote!(None),
                Some(MirExpr::Int { value, .. }) => {
                    let digits = i32::try_from(*value).unwrap_or(i32::MAX);
                    quote!(Some(#digits))
                }
                Some(other) => {
                    let digits_ty = value_ty(other, ctx, emit_env, body_ctx);
                    let optional_inner = match digits_ty.and_then(|ty| types.resolve(ty)) {
                        Some(Type::Optional { inner, .. }) => Some(*inner),
                        _ => None,
                    };
                    let numeric = optional_inner
                        .or(digits_ty)
                        .and_then(|ty| types.resolve(ty))
                        .is_some_and(is_numeric_type);
                    if !numeric {
                        return Err(BackendError::NotImplemented(
                            "Number.prototype formatting digits must be a number; a non-numeric \
                                 argument cannot be emitted as an `i32` digit count",
                        ));
                    }
                    let other = emit_expr(other, ctx, emit_env, body_ctx)?;
                    let integer_digits = optional_inner
                        .or(digits_ty)
                        .and_then(|ty| types.resolve(ty))
                        .is_some_and(is_integer_type);
                    if optional_inner.is_some() {
                        let converted = if integer_digits {
                            quote!(i32::try_from(__ts_digits).unwrap_or(i32::MAX))
                        } else {
                            quote!((__ts_digits) as i32)
                        };
                        quote!(match #other {
                            ::core::option::Option::Some(__ts_digits) =>
                                ::core::option::Option::Some(#converted),
                            ::core::option::Option::None => ::core::option::Option::None,
                        })
                    } else {
                        let converted = if integer_digits {
                            quote!(i32::try_from(#other).unwrap_or(i32::MAX))
                        } else {
                            quote!((#other) as i32)
                        };
                        quote!(::core::option::Option::Some(#converted))
                    }
                }
            };
            Ok(quote!(#name(__ctx, #value, #argument)))
        }
        RuntimeOp::BigIntNew => {
            let name = runtime_op_ident(op)?;
            let arg = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(#arg.to_string_lossy())))
        }
        RuntimeOp::BigIntAdd
        | RuntimeOp::BigIntSub
        | RuntimeOp::BigIntMul
        | RuntimeOp::BigIntDiv
        | RuntimeOp::BigIntRem
        | RuntimeOp::BigIntAnd
        | RuntimeOp::BigIntOr
        | RuntimeOp::BigIntXor
        | RuntimeOp::BigIntShl
        | RuntimeOp::BigIntShr
        | RuntimeOp::BigIntLt
        | RuntimeOp::BigIntLe
        | RuntimeOp::BigIntGt
        | RuntimeOp::BigIntGe
        | RuntimeOp::BigIntEq
        | RuntimeOp::BigIntNeq => {
            let name = runtime_op_ident(op)?;
            let lhs = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let rhs = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&#lhs, &#rhs)))
        }
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
