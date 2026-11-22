use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::expr::emit_expr;
use super::helpers::value_ty;
use super::place::{PlaceBorrow, emit_place_expr, expr_base_ty};
use super::try_stmt::await_helper_path;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::Type;
use ts_aot_ir_mir::MirExpr;

pub(super) fn emit_await_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Await { expr, ty, .. } = expr else {
        unreachable!()
    };
    let EmitEnv { types, .. } = emit_env;
    let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
    let needs_helper = matches!(expr.as_ref(), MirExpr::Import { .. })
        || value_ty(expr, ctx, emit_env, body_ctx)
            .and_then(|ty_id| types.resolve(ty_id))
            .is_some_and(|ty| matches!(ty, Type::Promise { .. }));
    if needs_helper {
        let ok_ty_id = value_ty(expr, ctx, emit_env, body_ctx)
            .and_then(|t| match types.resolve(t) {
                Some(Type::Promise { ok, .. }) => Some(*ok),
                _ => None,
            })
            .unwrap_or(*ty);
        let ok_ty = emit_type_id_with_ctx(ok_ty_id, emit_env, ctx);
        let helper = await_helper_path(body_ctx);
        Ok(quote!(#helper::<#ok_ty>(&#inner)))
    } else {
        Ok(inner)
    }
}

pub(super) fn emit_is_some_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::IsSome { expr, .. } = expr else {
        unreachable!()
    };
    let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
    if let Some(ty) = expr_base_ty(expr, body_ctx)
        && matches!(emit_env.types.resolve(ty), Some(Type::Fn { .. }))
        && let Some(kind) = ctx.fn_value_kind(ty)
    {
        let name = EmitCtx::fn_value_ident(kind);
        return Ok(quote!(!matches!(#inner, #name::__Unset)));
    }
    Ok(quote!(#inner.is_some()))
}

pub(super) fn emit_unwrap_optional_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::UnwrapOptional { expr, .. } = expr else {
        unreachable!()
    };
    if matches!(
        expr.as_ref(),
        MirExpr::Local(_) | MirExpr::Global(_) | MirExpr::Field { .. } | MirExpr::Index { .. }
    ) {
        let mut place_prologue: Vec<TokenStream> = Vec::new();
        let place = emit_place_expr(
            expr,
            ctx,
            emit_env,
            body_ctx,
            &mut place_prologue,
            PlaceBorrow::Exclusive,
        )?;
        let access = quote!((#place).clone().unwrap());
        return Ok(if place_prologue.is_empty() {
            access
        } else {
            quote!({
                #(#place_prologue)*
                #access
            })
        });
    }
    let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
    Ok(quote!(#inner.unwrap()))
}

pub(super) fn emit_yield_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Yield { expr, .. } = expr else {
        unreachable!()
    };
    let co = body_ctx.gen_co().ok_or_else(|| {
        BackendError::Internal(
            "MirExpr::Yield reached for non-generator body \
                 (HIR->MIR contract violation: lower_generators must \
                 transform every yield into a generator body)"
                .to_string(),
        )
    })?;
    let value = if let Some(inner) = expr {
        emit_expr(inner, ctx, emit_env, body_ctx)?
    } else {
        quote!(())
    };
    if body_ctx.is_async_generator() {
        Ok(quote!(
            #co.yield_(ts_aot_runtime::AsyncGenYield::yielded(#value)).await
        ))
    } else {
        Ok(quote!(#co.yield_(#value).await))
    }
}
