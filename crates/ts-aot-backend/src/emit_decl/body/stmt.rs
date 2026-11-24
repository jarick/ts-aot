use super::super::ctx::{
    BodyCtx, EmitCtx, EmitEnv, RuntimeCallSpec, SharedStorage, shared_reset_tokens,
    shared_write_tokens,
};
use super::super::types::emit_type_id_with_ctx;
use super::coerce::{dynamic_string_coerce, emit_owned_value, numeric_coerce, optional_coerce};
use super::expr::emit_expr;
use super::globals::check_fn_value_init;
use super::helpers::{is_dynamic_expr, loop_label};
use super::runtime_call::collections::emit_weakmap_liveness_reset;
use super::runtime_call::emit_runtime_call;
use super::stmt_assign::{emit_assign_stmt, shared_write_capture};
use super::stmt_flow::{emit_if_stmt, emit_return_stmt, emit_throw_payload};
use super::switch::emit_switch;
use super::try_stmt::{await_helper_path, emit_try};
use super::with_loop_body;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt};

pub(super) fn emit_block_stmts(
    block: &MirBlock,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    block
        .stmts
        .iter()
        .map(|stmt| emit_stmt(stmt, ctx, emit_env, body_ctx))
        .collect()
}

pub(super) fn let_init_tokens(
    init: &MirExpr,
    target_ty: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    check_fn_value_init(init, target_ty, ctx)?;
    let init_tokens = emit_owned_value(init, ctx, emit_env, body_ctx)?;
    let init_tokens = dynamic_string_coerce(init, init_tokens, target_ty, emit_env, body_ctx);
    let init_tokens =
        optional_coerce(init, init_tokens, target_ty, ctx, emit_env, body_ctx, false)?;
    Ok(numeric_coerce(
        init,
        init_tokens,
        target_ty,
        ctx,
        emit_env,
        body_ctx,
    ))
}

fn emit_runtime_stmt(
    spec: &RuntimeCallSpec<'_>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let call = emit_runtime_call(
        spec.op,
        spec.args,
        spec.ty,
        spec.target_ty,
        ctx,
        emit_env,
        body_ctx,
    )?;
    if let Some(dest) = spec.dest {
        let dest = body_ctx.local_ident(dest);
        let ty = emit_type_id_with_ctx(spec.ty, emit_env, ctx);
        let mutability = quote!();
        Ok(quote!(let #mutability #dest: #ty = #call;))
    } else {
        Ok(quote!(#call;))
    }
}

fn emit_do_while(
    body: &MirBlock,
    cond: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let id = body_ctx.alloc_block_id();
    let continue_ident = format_ident!("__do_while_continue_{id}");
    let break_ident = format_ident!("__do_while_break_{id}");
    let prev_continue = body_ctx.continue_label();
    let prev_break = body_ctx.break_label();
    body_ctx.set_continue_label(Some(continue_ident.clone()));
    body_ctx.set_break_label(Some(break_ident.clone()));
    body_ctx.enter_loop();
    let body_stmts = emit_block_stmts(body, ctx, emit_env, body_ctx);
    body_ctx.exit_loop();
    body_ctx.set_continue_label(prev_continue);
    body_ctx.set_break_label(prev_break);
    let body_stmts = body_stmts?;
    let cond = emit_expr(cond, ctx, emit_env, body_ctx)?;
    let continue_lt = loop_label(&continue_ident);
    let break_lt = loop_label(&break_ident);
    Ok(quote!(
        #break_lt: loop {
            #continue_lt: {
                #(#body_stmts)*
            }
            if !(#cond) { break #break_lt; }
        }
    ))
}

pub(super) fn emit_stmt(
    stmt: &MirStmt,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match stmt {
        MirStmt::Let {
            local,
            ty,
            init,
            mutable,
        } => {
            if let Some(acc) = body_ctx.shared_access_for_local(*local) {
                let Some(init) = init else {
                    return Ok(shared_reset_tokens(&acc));
                };
                return shared_write_capture(init, Some(*ty), &acc, ctx, emit_env, body_ctx);
            }
            let name = body_ctx.local_ident(*local);
            let target_ty = *ty;
            let dynamic_copy = init.as_ref().is_some_and(|init| {
                is_dynamic_expr(init, body_ctx)
                    && matches!(emit_env.types.resolve(target_ty), Some(Type::Error))
            });
            let ty = if dynamic_copy {
                quote!(ts_aot_runtime::CaughtValue)
            } else {
                emit_type_id_with_ctx(target_ty, emit_env, ctx)
            };
            let mutability = if *mutable || body_ctx.local_mut(*local) {
                quote!(mut)
            } else {
                quote!()
            };
            if let Some(init) = init {
                let init_tokens = let_init_tokens(init, target_ty, ctx, emit_env, body_ctx)?;
                Ok(quote!(let #mutability #name: #ty = #init_tokens;))
            } else if matches!(
                emit_env.types.resolve(target_ty),
                Some(Type::Optional { .. })
            ) {
                Ok(quote!(let #mutability #name: #ty = ::core::option::Option::None;))
            } else {
                Ok(quote!(let #mutability #name: #ty;))
            }
        }
        MirStmt::LetInfer { local, init } => {
            let name = body_ctx.local_ident(*local);
            let init_tokens = emit_owned_value(init, ctx, emit_env, body_ctx)?;
            Ok(quote!(let #name = #init_tokens;))
        }
        MirStmt::Assign { .. } => emit_assign_stmt(stmt, ctx, emit_env, body_ctx),
        MirStmt::Expr(expr) => {
            if matches!(expr, MirExpr::Local(_)) && !body_ctx.in_tla_main() {
                return Ok(quote!());
            }
            let expr = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(#expr;))
        }
        MirStmt::Return(slot) => emit_return_stmt(slot.as_ref(), ctx, emit_env, body_ctx),
        MirStmt::ReturnResultErr { error, err_ty }
        | MirStmt::Throw {
            error,
            error_ty: err_ty,
        } => {
            if body_ctx.is_async_generator() && !body_ctx.in_try() {
                let payload = emit_throw_payload(error, ctx, emit_env, body_ctx)?;
                Ok(quote!(ts_aot_runtime::__ts_aot_async_generator_throw(#payload);))
            } else if body_ctx.in_try() || body_ctx.is_generator() {
                let payload = emit_throw_payload(error, ctx, emit_env, body_ctx)?;
                Ok(quote!(ts_aot_runtime::__ts_aot_throw(#payload);))
            } else {
                let tokens = emit_expr(error, ctx, emit_env, body_ctx)?;
                let ty = *err_ty;
                let tokens = dynamic_string_coerce(error, tokens, ty, emit_env, body_ctx);
                let tokens = optional_coerce(error, tokens, ty, ctx, emit_env, body_ctx, false)?;
                let tokens = numeric_coerce(error, tokens, ty, ctx, emit_env, body_ctx);
                Ok(quote!(return Err(#tokens);))
            }
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => emit_if_stmt(
            cond,
            then_block,
            else_block.as_ref(),
            ctx,
            emit_env,
            body_ctx,
        ),
        MirStmt::While { cond, body } => {
            let cond = emit_expr(cond, ctx, emit_env, body_ctx)?;
            let (loop_label_lt, body_stmts) = with_loop_body(body_ctx, "while", || {
                emit_block_stmts(body, ctx, emit_env, body_ctx)
            })?;
            Ok(quote!(#loop_label_lt: while #cond { #(#body_stmts)* }))
        }
        this @ (MirStmt::ForOf {
            item,
            iterable,
            iter_ty,
            body,
        }
        | MirStmt::ForAwaitOf {
            item,
            iterable,
            iter_ty,
            body,
        }) => {
            let is_for_await = matches!(this, MirStmt::ForAwaitOf { .. });
            let iter_local = match iterable {
                MirExpr::Local(id) => Some(*id),
                _ => None,
            };
            let item_id = *item;
            let item_ident = body_ctx.local_ident(item_id);
            let item = if body_ctx.local_mut(item_id) {
                quote!(mut #item_ident)
            } else {
                quote!(#item_ident)
            };
            let iterable = emit_expr(iterable, ctx, emit_env, body_ctx)?;
            let (loop_label_lt, mut body_stmts) = with_loop_body(body_ctx, "for_of", || {
                emit_block_stmts(body, ctx, emit_env, body_ctx)
            })?;
            if let Some(acc) = body_ctx.shared_access_for_local(item_id) {
                body_stmts.insert(0, shared_write_tokens(&acc, &quote!(#item_ident)));
            }
            let liveness_reset =
                emit_weakmap_liveness_reset(&MirPlace::Local { id: item_id }, body_ctx);
            if !liveness_reset.is_empty() {
                body_stmts.insert(0, liveness_reset);
            }
            if is_for_await
                && matches!(
                    types.resolve(*iter_ty),
                    Some(ts_aot_core::Type::Generator { is_async: true, .. })
                )
            {
                let await_path = await_helper_path(body_ctx);
                let (owner_init, advance) = if let Some(id) = iter_local {
                    if let Some(acc) = body_ctx.shared_access_for_local(id)
                        && acc.storage == SharedStorage::RefCell
                    {
                        let env = &acc.env_ident;
                        let field = &acc.field;
                        (
                            quote!(),
                            quote! {{
                                let mut __for_await_guard = #env.#field.borrow_mut();
                                let __for_await_iter = __for_await_guard.as_mut().unwrap();
                                #await_path(&__for_await_iter.next())
                            }},
                        )
                    } else {
                        (
                            quote!(),
                            quote! {{
                                let __for_await_iter = &mut #iterable;
                                #await_path(&__for_await_iter.next())
                            }},
                        )
                    }
                } else {
                    (
                        quote!(let mut __for_await_owner = #iterable;),
                        quote! {{
                            let __for_await_iter = &mut __for_await_owner;
                            #await_path(&__for_await_iter.next())
                        }},
                    )
                };
                return Ok(quote!({
                    #owner_init
                    #loop_label_lt: loop {
                        let __for_await_yield = #advance;
                        if __for_await_yield.done {
                            break;
                        }
                        let #item = match __for_await_yield.value {
                            Some(__for_await_value) => __for_await_value,
                            None => break,
                        };
                        #(#body_stmts)*
                    }
                }));
            }
            let needs_mut_ref = matches!(
                types.resolve(*iter_ty),
                Some(ts_aot_core::Type::Generator { .. })
            );
            if needs_mut_ref {
                Ok(quote!(#loop_label_lt: for #item in &mut (#iterable) { #(#body_stmts)* }))
            } else {
                Ok(quote!(#loop_label_lt: for #item in #iterable { #(#body_stmts)* }))
            }
        }
        MirStmt::ForIn { key, object, body } => {
            let key_ident = body_ctx.local_ident(*key);
            let object = emit_expr(object, ctx, emit_env, body_ctx)?;
            let (loop_label_lt, mut body_stmts) = with_loop_body(body_ctx, "for_in", || {
                emit_block_stmts(body, ctx, emit_env, body_ctx)
            })?;
            if let Some(acc) = body_ctx.shared_access_for_local(*key) {
                body_stmts.insert(0, shared_write_tokens(&acc, &quote!(#key_ident)));
            }
            Ok(quote!(#loop_label_lt: for #key_ident in #object { #(#body_stmts)* }))
        }
        MirStmt::Break => {
            if body_ctx.break_escapes_try() {
                Ok(quote!(return __ReturnSignal::LoopBreak;))
            } else if let Some(label) = body_ctx.break_label() {
                let label_lt = loop_label(&label);
                Ok(quote!(break #label_lt;))
            } else {
                Ok(quote!(break;))
            }
        }
        MirStmt::Continue => {
            if body_ctx.continue_escapes_try() {
                Ok(quote!(return __ReturnSignal::LoopContinue;))
            } else if let Some(label) = body_ctx.continue_label() {
                let label_lt = loop_label(&label);
                Ok(quote!(break #label_lt;))
            } else if let Some(target) = body_ctx.continue_target() {
                let target_lt = loop_label(&target);
                Ok(quote!(continue #target_lt;))
            } else {
                Ok(quote!(continue;))
            }
        }
        MirStmt::Runtime {
            op,
            args,
            dest,
            ty,
            target_ty,
        } => {
            let spec = RuntimeCallSpec {
                op: *op,
                args,
                dest: *dest,
                ty: *ty,
                target_ty: *target_ty,
            };
            emit_runtime_stmt(&spec, ctx, emit_env, body_ctx)
        }
        MirStmt::DoWhile { body, cond } => emit_do_while(body, cond, ctx, emit_env, body_ctx),
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => emit_switch(disc, cases, default.as_ref(), ctx, emit_env, body_ctx),
        MirStmt::Try {
            body,
            catch_param,
            catch,
            finally,
        } => emit_try(
            body,
            *catch_param,
            catch.as_ref(),
            finally.as_ref(),
            ctx,
            emit_env,
            body_ctx,
        ),
    }
}
