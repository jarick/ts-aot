use self::helpers::{as_promise_type, loop_label};
use self::runtime_call::collections::emit_weakmap_liveness_binding;
use self::self_env::emit_env_prelude;
use self::stmt::emit_block_stmts;
use self::stmt_flow::{block_contains_try, mir_block_has_value_return, mir_block_terminates};
use super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ts_aot_core::Type;
use ts_aot_ir_mir::MirFunctionDecl;

mod binary;
mod call;
mod coerce;
mod equality;
mod equality_optional;
mod expr;
mod expr_ops;
mod globals;
mod helpers;
mod place;
mod runtime_call;
mod self_env;
mod stmt;
mod stmt_assign;
mod stmt_flow;
mod struct_literal;
mod switch;
mod taint;
mod taint_scan;
mod try_stmt;

pub(super) use self::coerce::emit_float;
pub(super) use self::equality::is_nullish_literal;
pub(super) use self::globals::owned_array_ty;
pub(super) use self::globals::struct_global_default_tokens;
pub(super) use self::struct_literal::coerce_static_init;
pub(super) use self::taint::self_capture_may_escape;

fn with_loop_body<F>(
    body_ctx: &BodyCtx,
    prefix: &str,
    emit_body: F,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError>
where
    F: FnOnce() -> Result<Vec<TokenStream>, BackendError>,
{
    let ident = format_ident!("__{}_{}", prefix, body_ctx.alloc_block_id());
    let stmts = body_ctx.with_loop_labels(Some(ident.clone()), emit_body)?;
    Ok((loop_label(&ident), stmts))
}

pub(crate) fn emit_body(
    f: &MirFunctionDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if !body_ctx.is_generator()
        && matches!(emit_env.types.resolve(f.ret), Some(Type::Error))
        && mir_block_has_value_return(&f.body.block)
    {
        return Err(BackendError::NotImplemented(
            "a function returns a value but its return type could not be determined; \
             add an explicit return type annotation",
        ));
    }
    if let Some(self_param) = body_ctx.self_param()
        && body_ctx.captures_receiver()
        && self_capture_may_escape(&f.body.block, self_param, emit_env, body_ctx)
    {
        return Err(BackendError::EscapingThisCapture {
            code: "E0513",
            message: "an arrow function that captures `this` escapes its method here, so the \
                 capture would outlive the method frame while still sharing the receiver's field \
                 cells; call the arrow inside the method, or pass the values it needs in as \
                 parameters/arguments",
        });
    }
    let mut stmts = emit_block_stmts(&f.body.block, ctx, emit_env, body_ctx)?;
    if block_contains_try(&f.body.block) {
        stmts.insert(0, return_signal_enum());
    }
    if let Some(prelude) = emit_env_prelude(body_ctx, emit_env, ctx) {
        stmts.insert(0, prelude);
    }
    if body_ctx.has_weakmap_liveness() {
        let cache = body_ctx.weakmap_liveness_per_key.borrow();
        let mut prelude: Vec<TokenStream> =
            cache.values().map(emit_weakmap_liveness_binding).collect();
        prelude.append(&mut stmts);
        stmts = prelude;
    }
    if !body_ctx.is_generator() && !mir_block_terminates(&f.body.block) {
        let (through_result, promise_ok) = match emit_env.types.resolve(f.ret) {
            Some(Type::Result { ok, .. }) => (
                true,
                as_promise_type(*ok, emit_env).map(|(_, promise_ok)| promise_ok),
            ),
            _ => (
                false,
                as_promise_type(f.ret, emit_env).map(|(_, promise_ok)| promise_ok),
            ),
        };
        let async_unit_completion = f.effects.is_async
            && promise_ok.is_some_and(|ok| matches!(emit_env.types.resolve(ok), Some(Type::Void)));
        let result_void = matches!(
            emit_env.types.resolve(f.ret),
            Some(Type::Result { ok, .. })
                if matches!(emit_env.types.resolve(*ok), Some(Type::Void))
        );
        if async_unit_completion {
            let unit = quote!(ts_aot_runtime::__ts_aot_promise_resolve_value::<()>(()));
            if through_result {
                stmts.push(quote!(return Ok(#unit);));
            } else {
                stmts.push(quote!(return #unit;));
            }
        } else if result_void {
            stmts.push(quote!(return Ok(());));
        } else if !matches!(emit_env.types.resolve(f.ret), Some(Type::Void)) {
            stmts.push(quote!(unreachable!()));
        }
    }
    if body_ctx.is_generator() {
        let co = body_ctx.gen_co().ok_or_else(|| {
            BackendError::Internal(
                "is_generator() returned true but gen_co identifier is unset (BodyCtx invariant violation)"
                    .to_string(),
            )
        })?;
        if body_ctx.is_async_generator() {
            Ok(quote!({
                ts_aot_runtime::__ts_aot_async_generator_new(|#co| async move { #(#stmts)* })
            }))
        } else {
            Ok(quote!({
                ts_aot_runtime::__ts_aot_generator_new(|#co| async move { #(#stmts)* })
            }))
        }
    } else {
        Ok(quote!({ #(#stmts)* }))
    }
}

fn return_signal_enum() -> TokenStream {
    quote!(
        enum __ReturnSignal {
            Continue,
            Break,
            LoopBreak,
            LoopContinue,
        }
    )
}
