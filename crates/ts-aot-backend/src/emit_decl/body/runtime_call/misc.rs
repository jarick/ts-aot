use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::super::runtime_op::runtime_op_ident;
use super::super::super::types::emit_type_id_with_ctx;
use super::super::coerce::{emit_owned_value, numeric_coerce_exact};
use super::super::equality::{emit_same_value, same_place};
use super::super::equality_optional::emit_typeof;
use super::super::expr::emit_expr;
use super::super::helpers::{is_display_supported_reason, type_requires_arena, value_ty};
use super::super::place::expr_base_ty;
use super::strings::{emit_js_string_arg, emit_js_string_owned};
use crate::error::BackendError;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use ts_aot_core::{StructId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{MirDecl, MirExpr, RuntimeOp};

fn struct_key(ty: &Type, types: &TypeTable) -> Option<StructId> {
    match ty {
        Type::Struct { id } => Some(*id),
        Type::Optional { inner, .. } => match types.resolve(*inner) {
            Some(Type::Struct { id }) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn local_struct_key(ty: Option<TypeId>, types: &TypeTable) -> Option<StructId> {
    struct_key(types.resolve(ty?)?, types)
}

fn struct_identity_field(
    a: &MirExpr,
    b: &MirExpr,
    a_ty: &Type,
    b_ty: &Type,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<Ident> {
    let (MirExpr::Local(a_id), MirExpr::Local(b_id)) = (a, b) else {
        return None;
    };
    let a_key = struct_key(a_ty, emit_env.types)?;
    if struct_key(b_ty, emit_env.types)? != a_key {
        return None;
    }
    let a_local = local_struct_key(
        body_ctx
            .local_ty(*a_id)
            .or_else(|| body_ctx.capture_ty(*a_id)),
        emit_env.types,
    )?;
    let b_local = local_struct_key(
        body_ctx
            .local_ty(*b_id)
            .or_else(|| body_ctx.capture_ty(*b_id)),
        emit_env.types,
    )?;
    if a_local != a_key || b_local != a_key {
        return None;
    }
    let a_struct = a_key;
    let field = emit_env
        .program
        .declarations
        .iter()
        .find_map(|decl| match decl {
            MirDecl::Struct(s) if s.id == a_struct => s.fields.first(),
            _ => None,
        })?;
    Some(ctx.struct_field_ident(a_struct, field.id))
}

pub(super) fn emit_misc_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match op {
        RuntimeOp::ArrayIsArray => {
            let value = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_is_array(&#value)))
        }
        RuntimeOp::ArrayIsArrayFalse => {
            let mut evaluated: Vec<TokenStream> = Vec::with_capacity(args.len());
            for arg in args {
                let value = emit_expr(arg, ctx, emit_env, body_ctx)?;
                evaluated.push(quote!(let _ = &#value;));
            }
            if evaluated.is_empty() {
                return Ok(quote!(__ts_aot_array_is_array_false()));
            }
            Ok(quote!({
                #(#evaluated)*
                __ts_aot_array_is_array_false()
            }))
        }
        RuntimeOp::OpIn => {
            let value = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let object = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_op_in(&#value, &#object)))
        }
        RuntimeOp::ArrayFrom => {
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_from(__ctx, &#arr)))
        }
        RuntimeOp::ObjectIs => {
            let a_ty = value_ty(&args[0], ctx, emit_env, body_ctx);
            let b_ty = value_ty(&args[1], ctx, emit_env, body_ctx);
            let a = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let b = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let both_unit = matches!((&args[0], &args[1]), (MirExpr::Unit, MirExpr::Unit));
            let both_null = matches!(
                (&args[0], &args[1]),
                (MirExpr::Null { .. }, MirExpr::Null { .. })
            );
            if both_unit || both_null {
                return Ok(quote!({
                    let _ = (&#a, &#b);
                    true
                }));
            }
            let resolve = |expr: &MirExpr, ty: Option<TypeId>| match expr {
                MirExpr::Unit => Some(Type::Void),
                MirExpr::Null { .. } => Some(Type::Null),
                MirExpr::Bool(_) => Some(Type::Bool),
                _ => ty.and_then(|id| types.resolve(id)).cloned(),
            };
            let a_ty = resolve(&args[0], a_ty).ok_or(BackendError::NotImplemented(
                "Object.is: first operand type unresolved",
            ))?;
            let b_ty = resolve(&args[1], b_ty).ok_or(BackendError::NotImplemented(
                "Object.is: second operand type unresolved",
            ))?;
            let identity_field =
                struct_identity_field(&args[0], &args[1], &a_ty, &b_ty, ctx, emit_env, body_ctx);
            let comparison = emit_same_value(
                types,
                &a_ty,
                &b_ty,
                (quote!(__ts_aot_a), quote!(__ts_aot_b)),
                same_place(&args[0], &args[1]),
                identity_field.as_ref(),
                0,
            )?;
            Ok(quote!({
                let __ts_aot_a = &#a;
                let __ts_aot_b = &#b;
                #comparison
            }))
        }

        RuntimeOp::DateParse => {
            let s = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_date_parse(#s)))
        }
        RuntimeOp::GeneratorNext => {
            let owner = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!((#owner).next()))
        }
        RuntimeOp::OpInstanceof => {
            let value = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let target_type_id: u32 = match args.get(2) {
                Some(MirExpr::Int { value, .. }) => (*value).try_into().unwrap_or(0),
                _ => 0,
            };
            Ok(quote!(__ts_aot_op_instanceof(&#value, #target_type_id)))
        }
        RuntimeOp::TypeOf => emit_typeof(&args[0], ctx, emit_env, body_ctx),
        RuntimeOp::PromiseAll | RuntimeOp::PromiseAllSettled => Err(BackendError::NotImplemented(
            "promise combinator: `Promise.all`/`Promise.allSettled` take an arena-backed array of \
                 promises and resolve to an array of their results; the emitted array borrows the \
                 arena while the runtime helpers take an owned `std::vec::Vec`, and the resolved \
                 element type cannot satisfy the `'static` bound the promise helpers require, so \
                 emitting this call would produce Rust that does not compile",
        )),
        RuntimeOp::PromiseRace | RuntimeOp::PromiseAny => Err(BackendError::NotImplemented(
            "promise combinator: `Promise.race`/`Promise.any` take an arena-backed array of \
                 promises; the emitted array borrows the arena while the runtime helpers take an \
                 owned `std::vec::Vec`, so emitting this call would produce Rust that does not \
                 compile",
        )),
        RuntimeOp::PromiseResolveStatic | RuntimeOp::PromiseRejectStatic => {
            let name = runtime_op_ident(op)?;
            let target = target_ty.unwrap_or(ty);
            if type_requires_arena(target, ctx, emit_env) {
                let message = if matches!(op, RuntimeOp::PromiseRejectStatic) {
                    "Promise.reject: the promise's value type borrows the arena, but the promise \
                         helpers require `T: Clone + 'static`; a promise whose value is an \
                         arena-backed string, array, or struct cannot satisfy that bound"
                } else {
                    "Promise: the resolved value borrows the arena, but the promise helpers \
                         require `T: Clone + 'static`; a promise whose value is an arena-backed \
                         string, array, or struct cannot satisfy that bound"
                };
                return Err(BackendError::NotImplemented(message));
            }
            let ty_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
            if matches!(op, RuntimeOp::PromiseRejectStatic) {
                let base_ty = expr_base_ty(&args[0], body_ctx).or_else(|| match &args[0] {
                    MirExpr::Global(name) => ctx.global_ty(name),
                    _ => None,
                });
                let reason = match &args[0] {
                    MirExpr::String { id, .. } => {
                        let lit = Literal::string(id.as_str());
                        quote!(ts_aot_runtime::JsString::from(#lit).to_string_lossy())
                    }
                    _ if matches!(base_ty.and_then(|ty| types.resolve(ty)), Some(Type::String)) => {
                        let js = emit_js_string_owned(&args[0], ctx, emit_env, body_ctx)?;
                        quote!(#js.to_string_lossy())
                    }
                    _ => {
                        let reason_ty = base_ty.and_then(|ty| types.resolve(ty));
                        if !is_display_supported_reason(reason_ty) {
                            return Err(BackendError::NotImplemented(
                                "promise reject: unsupported reason type",
                            ));
                        }
                        let inner = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
                        quote!(#inner.to_string())
                    }
                };
                Ok(quote!(#name::<#ty_tokens>(#reason)))
            } else {
                let value = emit_owned_value(&args[0], ctx, emit_env, body_ctx)?;
                let value = numeric_coerce_exact(&args[0], value, target, ctx, emit_env, body_ctx);
                Ok(quote!(#name::<#ty_tokens>(#value)))
            }
        }
        RuntimeOp::PromiseThenInstance
        | RuntimeOp::PromiseCatchInstance
        | RuntimeOp::PromiseFinallyInstance => {
            let name = runtime_op_ident(op)?;
            if let Some(input) = target_ty
                && type_requires_arena(input, ctx, emit_env)
            {
                return Err(BackendError::NotImplemented(
                    "promise instance method: the promise resolves to a value that borrows the \
                         arena, but the runtime helpers require `T: Clone + 'static`; a promise whose \
                         value is an arena-backed string, array, or struct cannot use \
                         `then`/`catch`/`finally`",
                ));
            }
            let promise = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let handler = emit_owned_value(&args[1], ctx, emit_env, body_ctx)?;
            let input_ty_tokens = match target_ty {
                Some(t) => emit_type_id_with_ctx(t, emit_env, ctx),
                None => emit_type_id_with_ctx(ty, emit_env, ctx),
            };
            Ok(quote!(#name::<#input_ty_tokens, _>(&#promise, #handler)))
        }
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
