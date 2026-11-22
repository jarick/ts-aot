use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, SharedStorage};
use super::coerce::{emit_owned_exprs, emit_owned_exprs_coerced};
use super::expr::{emit_expr, emit_exprs};
use super::globals::{emit_global_access, is_owned_global, struct_global_place_access};
use super::place::{
    PlaceBorrow, emit_place_expr, emit_place_ref_expr, place_paths_overlap, place_root_and_path,
};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::Type;
use ts_aot_ir_mir::MirExpr;

pub(super) fn mir_arg_requires_staging(arg: &MirExpr, body_ctx: &BodyCtx) -> bool {
    match arg {
        MirExpr::Local(id) => {
            Some(*id) == body_ctx.self_param()
                || body_ctx
                    .shared_access_for_local(*id)
                    .is_some_and(|access| access.storage == SharedStorage::RefCell)
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. } => false,
        _ => true,
    }
}

pub(super) fn emit_array_slice_arg(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Global(name) if is_owned_global(name, ctx, emit_env) => {
            Ok(emit_global_access(name, ctx, emit_env, body_ctx, false))
        }
        other => {
            let value = emit_expr(other, ctx, emit_env, body_ctx)?;
            Ok(quote!(&#value))
        }
    }
}

fn receiver_place_needs_value(expr: &MirExpr, body_ctx: &BodyCtx) -> bool {
    match expr {
        MirExpr::Local(id) => body_ctx
            .shared_access_for_local(*id)
            .is_some_and(|access| access.storage == SharedStorage::RefCell),
        MirExpr::Field { .. } => true,
        MirExpr::Index { base, .. } => receiver_place_needs_value(base, body_ctx),
        _ => false,
    }
}

pub(super) fn emit_mut_receiver(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
) -> Result<(TokenStream, bool), BackendError> {
    if let MirExpr::Global(name) = expr
        && ctx.is_mutable_global(name)
        && body_ctx.shared_access_for_name(name).is_none()
    {
        return Ok((
            emit_place_ref_expr(
                expr,
                ctx,
                emit_env,
                body_ctx,
                prologue,
                PlaceBorrow::Exclusive,
            )?,
            true,
        ));
    }
    Ok((
        emit_place_expr(
            expr,
            ctx,
            emit_env,
            body_ctx,
            prologue,
            PlaceBorrow::Exclusive,
        )?,
        false,
    ))
}

fn indirect_callee_fn_kind(
    callee: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<usize> {
    let EmitEnv { types, .. } = emit_env;
    let fn_ty = match callee {
        MirExpr::Local(id) => body_ctx
            .local_ty(*id)
            .or_else(|| body_ctx.capture_ty(*id))?,
        MirExpr::Global(name) => {
            if let Some((kind, _)) = ctx.fn_value_for_name(name) {
                return Some(kind);
            }
            body_ctx.shared_ty_for_name(name)?
        }
        other => other.ty()?,
    };
    if !matches!(types.resolve(fn_ty), Some(Type::Fn { .. })) {
        return None;
    }
    ctx.fn_value_kind(fn_ty)
}

fn optional_call_map_arm(
    callee: &MirExpr,
    args: &[MirExpr],
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let MirExpr::OptionalChain { base: inner, ty } = callee else {
        return Ok(None);
    };
    let Some(Type::Optional {
        inner: inner_ty, ..
    }) = types.resolve(*ty)
    else {
        return Ok(None);
    };
    let receiver = body_ctx.alloc_hoist_ident("__ts_recv");
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    if let Some(kind) = ctx.fn_value_kind(*inner_ty) {
        let sig = ctx.fn_value_sig(kind);
        let param_tys = sig.params.clone();
        let param_optional = sig.optional.clone();
        let mut coerced_args: Vec<MirExpr> = args.to_vec();
        while coerced_args.len() < param_tys.len() {
            let index = coerced_args.len();
            if !param_optional.get(index).copied().unwrap_or(false) {
                break;
            }
            coerced_args.push(MirExpr::Unit);
        }
        let args_tokens =
            emit_owned_exprs_coerced(&coerced_args, Some(&param_tys), ctx, emit_env, body_ctx)?;
        let helper = EmitCtx::fn_call_ident(kind);
        return Ok(Some(quote!(
            #inner_tokens
                .as_ref()
                .map(|#receiver| #helper(__ctx, #receiver, #(#args_tokens),*))
        )));
    }
    let args_tokens = emit_exprs(args, ctx, emit_env, body_ctx)?;
    Ok(Some(
        quote!(#inner_tokens.as_ref().map(|#receiver| #receiver(#(#args_tokens),*))),
    ))
}

pub(super) fn emit_call_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Call { callee, args, .. } = expr else {
        unreachable!()
    };
    let callee_id = *callee;
    let is_async = ctx.function_is_async(callee_id);
    let is_method = ctx.function_owner(callee_id).is_some();
    let is_constructor = ctx.function_is_constructor(callee_id);
    let is_mutating_method = ctx.function_is_mutating_method(callee_id);
    let callee_ident = ctx.function_ident(callee_id);
    let param_tys = ctx.function_params(callee_id);
    let owner_ident = ctx
        .function_owner(callee_id)
        .map(|owner| ctx.struct_ident(owner));
    if is_method
        && let Some(MirExpr::OptionalChain { base, ty }) = args.first()
        && let Some(owner_ident) = owner_ident.clone()
        && matches!(emit_env.types.resolve(*ty), Some(Type::Optional { .. }))
    {
        let receiver = emit_expr(base, ctx, emit_env, body_ctx)?;
        let receiver_opt = body_ctx.alloc_hoist_ident("__ts_recv_opt");
        let recv = body_ctx.alloc_hoist_ident("__ts_recv");
        let rest = &args[1..];
        let rest_param_tys = param_tys.map(|tys| &tys[1..]);
        let rest = emit_owned_exprs_coerced(rest, rest_param_tys, ctx, emit_env, body_ctx)?;
        let call = if is_mutating_method {
            quote!(#owner_ident::#callee_ident(#recv, __ctx, #(#rest),*))
        } else {
            quote!(#owner_ident::#callee_ident(#recv.clone(), __ctx, #(#rest),*))
        };
        let call = if is_async {
            quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call))
        } else {
            call
        };
        return Ok(quote!({
            let #receiver_opt = (#receiver).clone();
            #receiver_opt.as_ref().map(|#recv| #call)
        }));
    }
    let callee = if let Some(owner_ident) = owner_ident {
        quote!(#owner_ident::#callee_ident)
    } else {
        quote!(#callee_ident)
    };
    if is_method && (is_constructor || is_mutating_method) {
        let (receiver, rest) = args
            .split_first()
            .expect("a method call always carries its receiver");
        let receiver_path = place_root_and_path(receiver);
        let receiver_is_local = matches!(receiver, MirExpr::Local(_));
        let receiver_stays_inline = matches!(receiver, MirExpr::Local(_) | MirExpr::Global(_));
        let receiver_needs_value = receiver_place_needs_value(receiver, body_ctx);
        let mut receiver_prologue: Vec<TokenStream> = Vec::new();
        let receiver = match receiver {
            MirExpr::Local(id) if body_ctx.shared_access_for_local(*id).is_some() => {
                body_ctx.local_place_base(*id)
            }
            MirExpr::Local(id) if Some(*id) == body_ctx.self_param() => {
                if body_ctx.self_is_borrowed() {
                    quote!((*self))
                } else {
                    quote!((self))
                }
            }
            MirExpr::Local(id) => body_ctx.local_ref(*id),
            MirExpr::Global(name) => {
                if let Some(access) = struct_global_place_access(name, ctx, emit_env) {
                    access
                } else {
                    emit_place_expr(
                        receiver,
                        ctx,
                        emit_env,
                        body_ctx,
                        &mut receiver_prologue,
                        PlaceBorrow::Shared,
                    )?
                }
            }
            MirExpr::UnwrapOptional { .. } => {
                return Err(BackendError::NotImplemented(
                    "a mutating method call on a non-null-asserted optional receiver is not \
                         supported: the AOT target cannot borrow the unwrapped value in place. \
                         Bind the value to a non-optional local first",
                ));
            }
            other => emit_place_expr(
                other,
                ctx,
                emit_env,
                body_ctx,
                &mut receiver_prologue,
                PlaceBorrow::Shared,
            )?,
        };
        let mut receiver_arg = quote!(&(#receiver));
        if !receiver_stays_inline {
            let name = body_ctx.alloc_hoist_ident("__ts_recv");
            if receiver_needs_value {
                receiver_prologue.push(quote!(let #name = (#receiver).clone();));
                receiver_arg = quote!(&(#name));
            } else {
                receiver_prologue.push(quote!(let #name = &(#receiver);));
                receiver_arg = quote!(#name);
            }
        }
        let rest_tokens = emit_owned_exprs_coerced(
            rest,
            param_tys.map(|tys| &tys[1..]),
            ctx,
            emit_env,
            body_ctx,
        )?;
        let mut binds = Vec::with_capacity(rest_tokens.len());
        let mut staged_rest = Vec::with_capacity(rest_tokens.len());
        let stage_all = rest.iter().any(|arg| {
            mir_arg_requires_staging(arg, body_ctx)
                || place_paths_overlap(receiver_path.as_ref(), place_root_and_path(arg).as_ref())
        });
        for (index, (arg, tokens)) in rest.iter().zip(rest_tokens.iter()).enumerate() {
            if stage_all || mir_arg_requires_staging(arg, body_ctx) {
                let name = body_ctx.alloc_hoist_ident(&format!("__ts_arg_{index}"));
                binds.push(quote!(let #name = #tokens;));
                staged_rest.push(quote!(#name));
            } else {
                staged_rest.push(tokens.clone());
            }
        }
        if receiver_is_local && (receiver_needs_value || !binds.is_empty()) {
            let name = body_ctx.alloc_hoist_ident("__ts_recv");
            receiver_prologue.push(quote!(let #name = (#receiver).clone();));
            receiver_arg = quote!(&(#name));
        }
        let call = quote!(#callee(#receiver_arg, __ctx, #(#staged_rest),*));
        let call = if is_async {
            quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call))
        } else {
            call
        };
        return Ok(quote!({
            #(#receiver_prologue)*
            #(#binds)*
            #call
        }));
    }
    let args = emit_owned_exprs_coerced(args, param_tys, ctx, emit_env, body_ctx)?;
    let call_args = if is_method {
        let mut v: Vec<TokenStream> = Vec::with_capacity(args.len() + 1);
        let mut iter = args.into_iter();
        if let Some(first) = iter.next() {
            v.push(first);
        }
        v.push(quote!(__ctx));
        v.extend(iter);
        v
    } else {
        let mut v: Vec<TokenStream> = Vec::with_capacity(args.len() + 1);
        v.push(quote!(__ctx));
        v.extend(args);
        v
    };
    let call = quote!(#callee(#(#call_args),*));
    if is_async {
        Ok(quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call)))
    } else {
        Ok(call)
    }
}

pub(super) fn emit_indirect_call_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::IndirectCall { callee, args, .. } = expr else {
        unreachable!()
    };
    if let Some(map) = optional_call_map_arm(callee, args, ctx, emit_env, body_ctx)? {
        return Ok(map);
    }
    if let Some(k) = indirect_callee_fn_kind(callee, ctx, emit_env, body_ctx) {
        let callee = emit_expr(callee, ctx, emit_env, body_ctx)?;
        let sig = ctx.fn_value_sig(k);
        let param_tys = sig.params.clone();
        let param_optional = sig.optional.clone();
        let mut coerced_args: Vec<MirExpr> = args.clone();
        while coerced_args.len() < param_tys.len() {
            let index = coerced_args.len();
            if !param_optional.get(index).copied().unwrap_or(false) {
                break;
            }
            coerced_args.push(MirExpr::Unit);
        }
        let args =
            emit_owned_exprs_coerced(&coerced_args, Some(&param_tys), ctx, emit_env, body_ctx)?;
        let helper = EmitCtx::fn_call_ident(k);
        return Ok(quote!(#helper(__ctx, &#callee, #(#args),*)));
    }
    let callee = emit_expr(callee, ctx, emit_env, body_ctx)?;
    let args = emit_owned_exprs(args, ctx, emit_env, body_ctx)?;
    Ok(quote!(#callee(#(#args),*)))
}
