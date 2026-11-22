use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::coerce::{numeric_coerce, optional_coerce};
use super::equality::is_nullish_literal;
use super::expr::{emit_expr, emit_value_expr};
use super::helpers::{as_promise_type, is_dynamic_expr, type_requires_arena, value_ty};
use super::stmt::emit_block_stmts;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{LocalId, Type, TypeTable};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirStmt};

pub(super) fn block_contains_try(block: &MirBlock) -> bool {
    block.stmts.iter().any(stmt_contains_try)
}

pub(super) fn mir_block_has_value_return(block: &MirBlock) -> bool {
    block.stmts.iter().any(mir_stmt_has_value_return)
}

fn mir_stmt_has_value_return(stmt: &MirStmt) -> bool {
    match stmt {
        MirStmt::Return(Some(expr)) => !is_nullish_literal(expr),
        MirStmt::If {
            then_block,
            else_block,
            ..
        } => {
            mir_block_has_value_return(then_block)
                || else_block.as_ref().is_some_and(mir_block_has_value_return)
        }
        MirStmt::While { body, .. }
        | MirStmt::DoWhile { body, .. }
        | MirStmt::ForOf { body, .. }
        | MirStmt::ForAwaitOf { body, .. }
        | MirStmt::ForIn { body, .. } => mir_block_has_value_return(body),
        MirStmt::Switch { cases, default, .. } => {
            cases
                .iter()
                .any(|case| mir_block_has_value_return(&case.body))
                || default.as_ref().is_some_and(mir_block_has_value_return)
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            mir_block_has_value_return(body)
                || catch.as_ref().is_some_and(mir_block_has_value_return)
                || finally.as_ref().is_some_and(mir_block_has_value_return)
        }
        _ => false,
    }
}

pub(super) fn mir_block_terminates(block: &MirBlock) -> bool {
    block.stmts.last().is_some_and(|stmt| {
        matches!(
            stmt,
            MirStmt::Return(_)
                | MirStmt::ReturnResultErr { .. }
                | MirStmt::Throw { .. }
                | MirStmt::Break
                | MirStmt::Continue
        )
    })
}

fn stmt_contains_try(stmt: &MirStmt) -> bool {
    match stmt {
        MirStmt::Try { .. } => true,
        MirStmt::If {
            then_block,
            else_block,
            ..
        } => block_contains_try(then_block) || else_block.as_ref().is_some_and(block_contains_try),
        MirStmt::While { body, .. }
        | MirStmt::DoWhile { body, .. }
        | MirStmt::ForOf { body, .. }
        | MirStmt::ForAwaitOf { body, .. }
        | MirStmt::ForIn { body, .. } => block_contains_try(body),
        MirStmt::Switch { cases, default, .. } => {
            cases.iter().any(|case| block_contains_try(&case.body))
                || default.as_ref().is_some_and(block_contains_try)
        }
        _ => false,
    }
}

pub(super) fn collect_assigned_locals(
    stmts: &[ts_aot_ir_mir::MirStmt],
    types: &TypeTable,
    candidates: &std::collections::HashSet<LocalId>,
) -> std::collections::HashSet<LocalId> {
    super::super::ctx::collect_assigned_locals(stmts, types, candidates)
}

pub(super) fn emit_if_stmt(
    cond: &MirExpr,
    then_block: &MirBlock,
    else_block: Option<&MirBlock>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let cond = emit_expr(cond, ctx, emit_env, body_ctx)?;
    let then_stmts = emit_block_stmts(then_block, ctx, emit_env, body_ctx)?;
    if let Some(else_block) = else_block {
        let else_stmts = emit_block_stmts(else_block, ctx, emit_env, body_ctx)?;
        Ok(quote!(if #cond { #(#then_stmts)* } else { #(#else_stmts)* }))
    } else {
        Ok(quote!(if #cond { #(#then_stmts)* }))
    }
}

fn finalize_return_value(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let ret_ty = body_ctx.return_type_id();
    let (through_result, promise, scalar) = match emit_env.types.resolve(ret_ty) {
        Some(Type::Result { ok, .. }) => match as_promise_type(*ok, emit_env) {
            Some((promise_ty, promise_ok)) => (true, Some((promise_ty, promise_ok)), promise_ok),
            None => (true, None, *ok),
        },
        _ => match as_promise_type(ret_ty, emit_env) {
            Some((promise_ty, promise_ok)) => (false, Some((promise_ty, promise_ok)), promise_ok),
            None => (false, None, ret_ty),
        },
    };

    let (inner_expr, mut inner_tokens) = if through_result {
        if let MirExpr::ResultOk { value, .. } = expr {
            (
                value.as_ref(),
                emit_value_expr(value, ctx, emit_env, body_ctx)?,
            )
        } else if matches!(
            value_ty(expr, ctx, emit_env, body_ctx).and_then(|t| emit_env.types.resolve(t)),
            Some(Type::Result { .. })
        ) {
            return emit_value_expr(expr, ctx, emit_env, body_ctx);
        } else {
            (expr, emit_value_expr(expr, ctx, emit_env, body_ctx)?)
        }
    } else {
        (expr, emit_value_expr(expr, ctx, emit_env, body_ctx)?)
    };

    if is_dynamic_expr(inner_expr, body_ctx)
        && matches!(emit_env.types.resolve(scalar), Some(Type::String))
    {
        inner_tokens = quote!(#inner_tokens.to_js_string(__ctx));
    }

    let returns_existing_promise = promise.is_some_and(|(promise_ty, _)| {
        value_ty(inner_expr, ctx, emit_env, body_ctx) == Some(promise_ty)
    });
    if !returns_existing_promise {
        inner_tokens = numeric_coerce(inner_expr, inner_tokens, scalar, ctx, emit_env, body_ctx);
        inner_tokens = optional_coerce(
            inner_expr,
            inner_tokens,
            scalar,
            ctx,
            emit_env,
            body_ctx,
            false,
        )?;
    }

    if let Some((_, promise_ok)) = promise
        && !returns_existing_promise
    {
        if type_requires_arena(promise_ok, ctx, emit_env) {
            return Err(BackendError::NotImplemented(
                "promise-returning function: the resolved value borrows the arena, but the \
                 promise helpers require `T: Clone + 'static`; store an owned value (for example \
                 `Promise<i64>`) or convert the value before resolving the promise",
            ));
        }
        let ok_ty = emit_type_id_with_ctx(promise_ok, emit_env, ctx);
        inner_tokens =
            quote!(ts_aot_runtime::__ts_aot_promise_resolve_value::<#ok_ty>(#inner_tokens));
    }

    Ok(if through_result {
        quote!(Ok(#inner_tokens))
    } else {
        inner_tokens
    })
}

pub(super) fn emit_return_stmt(
    slot: Option<&MirExpr>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if body_ctx.try_label().is_some() {
        let slot_ident = body_ctx
            .return_slot()
            .expect("emit_return_stmt inside try requires return_slot to be set by emit_try");
        let value_ts = if body_ctx.is_generator() {
            if let Some(expr) = slot {
                let expr = emit_value_expr(expr, ctx, emit_env, body_ctx)?;
                quote!(Some(#expr))
            } else {
                quote!(None)
            }
        } else if let Some(expr) = slot {
            finalize_return_value(expr, ctx, emit_env, body_ctx)?
        } else if body_ctx.async_plain_ok().is_some() {
            async_absent_return(ctx, emit_env, body_ctx)?
        } else {
            quote!(())
        };
        Ok(quote!(#slot_ident = Some(#value_ts); return __ReturnSignal::Break;))
    } else if body_ctx.is_generator() {
        let async_gen = body_ctx.is_async_generator();
        if let Some(expr) = slot {
            let expr = emit_value_expr(expr, ctx, emit_env, body_ctx)?;
            if async_gen {
                Ok(quote!(
                    return Some(ts_aot_runtime::AsyncGenYield::finished(Some(#expr)));
                ))
            } else {
                Ok(quote!(return Some(#expr);))
            }
        } else if async_gen {
            Ok(quote!(
                return Some(ts_aot_runtime::AsyncGenYield::finished(None));
            ))
        } else {
            Ok(quote!(return None;))
        }
    } else if let Some(expr) = slot {
        let value = finalize_return_value(expr, ctx, emit_env, body_ctx)?;
        Ok(quote!(return #value;))
    } else if body_ctx.async_plain_ok().is_some() {
        let absent = async_absent_return(ctx, emit_env, body_ctx)?;
        Ok(quote!(return #absent;))
    } else {
        Ok(quote!(return;))
    }
}

fn async_absent_return(
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let ok = body_ctx.async_plain_ok().ok_or_else(|| {
        BackendError::Internal(
            "async absent return requires async_plain_ok (BodyCtx invariant violation)".to_string(),
        )
    })?;
    let value = match emit_env.types.resolve(ok) {
        Some(
            Type::Void
            | Type::Null
            | Type::Error
            | Type::Union { .. }
            | Type::Intersection { .. }
            | Type::Tuple { .. },
        ) => quote!(()),
        Some(Type::Optional { .. }) => quote!(::core::option::Option::None),
        _ => {
            return Err(BackendError::NotImplemented(
                "a bare `return;` in an async function resolves `undefined`, but the declared \
                 promise payload cannot hold `undefined`; declare the payload as `T | undefined` \
                 or `void`",
            ));
        }
    };
    if type_requires_arena(ok, ctx, emit_env) {
        return Err(BackendError::NotImplemented(
            "promise-returning function: the resolved value borrows the arena, but the promise \
             helpers require `T: Clone + 'static`; store an owned value (for example \
             `Promise<i64>`) or convert the value before resolving the promise",
        ));
    }
    let ok_ty = emit_type_id_with_ctx(ok, emit_env, ctx);
    let resolved = quote!(ts_aot_runtime::__ts_aot_promise_resolve_value::<#ok_ty>(#value));
    Ok(
        if matches!(
            emit_env.types.resolve(body_ctx.return_type_id()),
            Some(Type::Result { .. })
        ) {
            quote!(Ok(#resolved))
        } else {
            resolved
        },
    )
}

pub(super) fn emit_throw_payload(
    error: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let error_tokens = emit_expr(error, ctx, emit_env, body_ctx)?;
    let Some(ty) = value_ty(error, ctx, emit_env, body_ctx) else {
        return Ok(error_tokens);
    };
    if !type_requires_arena(ty, ctx, emit_env) {
        return Ok(error_tokens);
    }
    match emit_env.types.resolve(ty) {
        Some(Type::String) => Ok(quote!(ts_aot_runtime::CaughtValue::from(#error_tokens))),
        _ => Err(BackendError::NotImplemented(
            "throw: this value borrows the arena and cannot cross the `'static` panic boundary \
             of `try`/`catch`; only a string or a primitive value can be thrown from inside a \
             `try`",
        )),
    }
}
