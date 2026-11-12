use proc_macro2::{Ident, Literal, Punct, Spacing, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashSet;

use ts_aot_core::{Atom, FieldId, LocalId, NullishKinds, StructId, Type, TypeId, TypeTable};

use ts_aot_ir_mir::{
    BinaryOp, ConstValue, MirBlock, MirDecl, MirExpr, MirFunctionDecl, MirLocalDecl, MirParam,
    MirPlace, MirPlaceBase, MirStmt, RuntimeOp, UnaryOp,
};

use super::ctx::{
    BodyCtx, EmitCtx, EmitEnv, RuntimeCallSpec, SharedAccess, SharedStorage,
    place_root_global_name, shared_borrow_tokens, shared_reset_tokens, shared_write_tokens,
};
use super::ident::ident_from;
use super::literals::{emit_float_from_int, emit_whole_number_literal};
use super::runtime_op::runtime_op_ident;
use super::types::emit_type_id_with_ctx;
use super::{DurableGlobal, global_durable_kind};
use crate::error::BackendError;

fn loop_label(ident: &Ident) -> TokenStream {
    let apostrophe = Punct::new('\'', Spacing::Joint);
    quote!(#apostrophe #ident)
}

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

pub(super) fn emit_body(
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

fn block_contains_try(block: &MirBlock) -> bool {
    block.stmts.iter().any(stmt_contains_try)
}

fn mir_block_has_value_return(block: &MirBlock) -> bool {
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

fn mir_block_terminates(block: &MirBlock) -> bool {
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

fn collect_assigned_locals(
    stmts: &[ts_aot_ir_mir::MirStmt],
    types: &TypeTable,
    candidates: &std::collections::HashSet<LocalId>,
) -> std::collections::HashSet<LocalId> {
    super::ctx::collect_assigned_locals(stmts, types, candidates)
}

fn emit_block_stmts(
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

fn emit_if_stmt(
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

fn as_promise_type(ty: TypeId, emit_env: &EmitEnv) -> Option<(TypeId, TypeId)> {
    match emit_env.types.resolve(ty) {
        Some(Type::Promise { ok, .. }) => Some((ty, *ok)),
        _ => None,
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

    if let Some((promise_ty, promise_ok)) = promise
        && value_ty(inner_expr, ctx, emit_env, body_ctx) != Some(promise_ty)
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

fn emit_return_stmt(
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
            async_unit_return(emit_env, body_ctx)
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
        let unit = async_unit_return(emit_env, body_ctx);
        Ok(quote!(return #unit;))
    } else {
        Ok(quote!(return;))
    }
}

fn async_unit_return(emit_env: &EmitEnv, body_ctx: &BodyCtx) -> TokenStream {
    let unit = quote!(ts_aot_runtime::__ts_aot_promise_resolve_value::<()>(()));
    if matches!(
        emit_env.types.resolve(body_ctx.return_type_id()),
        Some(Type::Result { .. })
    ) {
        quote!(Ok(#unit))
    } else {
        unit
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

/// Writes a value into a shared (captured) variable.
///
/// The environment field holds `Option<T>` (the storage the runtime uses to
/// detect a read before initialization), so a nullish value resets it to `None`
/// and anything else is coerced to `T` and wrapped in `Some`. Without this the
/// write emitted `Some(())`/`Some(<i64>)` for an `Option<f64>` field, i.e. code
/// that cannot compile.
fn shared_write_capture(
    value: &MirExpr,
    declared: Option<TypeId>,
    acc: &SharedAccess,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
    // The environment field holds `Option<declared type>` (the outer `Option` is
    // the runtime's "written yet?" marker, which `shared_write_tokens` adds), so
    // the value must be coerced to the *declared* type - an `Option<f64>` field
    // of a captured `number | null` needs `Some(Option::None)` for `null`, not
    // `Some(())`, and `Some(5.0)` for `5`, not `Some(5)`.
    let tokens = match declared {
        Some(declared_ty)
            if !matches!(
                emit_env.types.resolve(declared_ty),
                None | Some(Type::Error)
            ) =>
        {
            let tokens = dynamic_string_coerce(value, tokens, declared_ty, emit_env, body_ctx);
            let tokens =
                optional_coerce(value, tokens, declared_ty, ctx, emit_env, body_ctx, false)?;
            numeric_coerce(value, tokens, declared_ty, ctx, emit_env, body_ctx)
        }
        _ => tokens,
    };
    Ok(shared_write_tokens(acc, &tokens))
}
fn emit_stmt(
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
                check_fn_value_init(init, target_ty, ctx)?;
                let init_tokens = emit_owned_value(init, ctx, emit_env, body_ctx)?;
                let init_tokens =
                    dynamic_string_coerce(init, init_tokens, target_ty, emit_env, body_ctx);
                let init_tokens =
                    optional_coerce(init, init_tokens, target_ty, ctx, emit_env, body_ctx, false)?;
                let init_tokens =
                    numeric_coerce(init, init_tokens, target_ty, ctx, emit_env, body_ctx);
                Ok(quote!(let #mutability #name: #ty = #init_tokens;))
            } else {
                Ok(quote!(let #mutability #name: #ty;))
            }
        }
        MirStmt::Assign { target, value } => {
            if let MirPlace::Local { id } = target
                && let Some(acc) = body_ctx.shared_access_for_local(*id)
            {
                return shared_write_capture(
                    value,
                    // A closure's own `BodyCtx` has no entry for the captured
                    // local (it belongs to the enclosing function), so the
                    // capture table is consulted as well.
                    body_ctx.local_ty(*id).or_else(|| body_ctx.capture_ty(*id)),
                    &acc,
                    ctx,
                    emit_env,
                    body_ctx,
                );
            }
            if let MirPlace::Global(name) = target
                && let Some(acc) = body_ctx.shared_access_for_name(name)
            {
                return shared_write_capture(
                    value,
                    // A shared name is a captured local: the module-global table
                    // does not have it, so the env field's declared type is the
                    // one to coerce to.
                    body_ctx
                        .shared_ty_for_name(name)
                        .or_else(|| ctx.global_ty(name)),
                    &acc,
                    ctx,
                    emit_env,
                    body_ctx,
                );
            }
            if let Some(chain_assign) =
                emit_optional_chain_assign(target, value, ctx, emit_env, body_ctx)?
            {
                return Ok(chain_assign);
            }
            let target_tokens = emit_place(target, ctx, emit_env, body_ctx)?;
            let value_tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
            let value_tokens = match target {
                MirPlace::Local { id } => match body_ctx.local_ty(*id) {
                    Some(target_ty) => {
                        let coerced = dynamic_string_coerce(
                            value,
                            value_tokens,
                            target_ty,
                            emit_env,
                            body_ctx,
                        );
                        let coerced = optional_coerce(
                            value, coerced, target_ty, ctx, emit_env, body_ctx, false,
                        )?;
                        numeric_coerce(value, coerced, target_ty, ctx, emit_env, body_ctx)
                    }
                    None => value_tokens,
                },
                MirPlace::Global(name) if is_durable_global(name, ctx, emit_env) => {
                    let target_ty = ctx.global_ty(name);
                    let coerced = match target_ty {
                        Some(target_ty) => dynamic_string_coerce(
                            value,
                            value_tokens,
                            target_ty,
                            emit_env,
                            body_ctx,
                        ),
                        None => value_tokens,
                    };
                    if let (Some(target_ty), Some(DurableGlobal::OptionalString)) = (
                        target_ty,
                        target_ty.and_then(|ty| global_durable_kind(ty, emit_env)),
                    ) {
                        optional_coerce(value, coerced, target_ty, ctx, emit_env, body_ctx, true)?
                    } else {
                        quote!(#coerced.to_string_lossy())
                    }
                }
                MirPlace::Global(name) => {
                    if let Some(target_ty) = ctx.global_ty(name) {
                        let coerced = optional_coerce(
                            value,
                            value_tokens,
                            target_ty,
                            ctx,
                            emit_env,
                            body_ctx,
                            false,
                        )?;
                        numeric_coerce(value, coerced, target_ty, ctx, emit_env, body_ctx)
                    } else {
                        value_tokens
                    }
                }
                MirPlace::Field { ty, .. } | MirPlace::Index { ty, .. } => {
                    let coerced =
                        dynamic_string_coerce(value, value_tokens, *ty, emit_env, body_ctx);
                    let coerced =
                        optional_coerce(value, coerced, *ty, ctx, emit_env, body_ctx, false)?;
                    numeric_coerce(value, coerced, *ty, ctx, emit_env, body_ctx)
                }
            };
            let reset = emit_weakmap_liveness_reset(target, body_ctx);
            let assignment = quote! {
                #target_tokens = #value_tokens;
                #reset
            };
            if place_root_global_name(target).is_some_and(|name| ctx.is_mutable_global(name)) {
                Ok(quote! {
                    unsafe { #assignment }
                })
            } else {
                Ok(assignment)
            }
        }
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
                // The thrown value must have the function's error type
                // (`throw 1` in a function that throws `f64`).
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

fn durable_global_kind(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> Option<DurableGlobal> {
    ctx.global_ty(name)
        .and_then(|ty| global_durable_kind(ty, emit_env))
}

fn is_durable_global(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    durable_global_kind(name, ctx, emit_env).is_some()
}

/// A function value is emitted as a variant of the enum for its *own* signature
/// bucket, so a local initialised from a function name is valid only when the
/// local's declared function type is that same bucket and the bucket has a
/// variant for the function.
///
/// Both failures are possible - parameter bivariance lets `f(x: number)` be
/// stored in a `(x: number | null) => number` local, whose bucket no declaration
/// fits, and a declaration is left out of a bucket when its ABI cannot be
/// adapted (see `fn_value_bucket`) - and both are invalid Rust: the emitted
/// initialiser is a bare function item where the enum is expected. Report it
/// instead of emitting that.
fn check_fn_value_init(
    init: &MirExpr,
    target_ty: TypeId,
    ctx: &EmitCtx,
) -> Result<(), BackendError> {
    let MirExpr::Global(name) = init else {
        return Ok(());
    };
    let Some(declared) = ctx.fn_value_kind(target_ty) else {
        return Ok(());
    };
    if ctx
        .fn_value_for_name(name)
        .is_some_and(|(registered, _)| registered == declared)
    {
        return Ok(());
    }
    Err(BackendError::Internal(format!(
        "`{}` cannot be stored in this function value: its own signature belongs to a different \
         function-value bucket, so the generated crate would name a variant the enum does not \
         have (the AOT target has no adapter for this function type yet)",
        name.as_str()
    )))
}

fn emit_global_access(
    name: &Atom,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    owned: bool,
) -> TokenStream {
    if let Some(read) = body_ctx.shared_read_for_name(name) {
        return read;
    }
    if let Some((k, variant)) = ctx.fn_value_for_name(name) {
        let enum_name = EmitCtx::fn_value_ident(k);
        return quote!(#enum_name::#variant);
    }
    let ident = ident_from(name);
    let mutable = ctx.is_mutable_global(name);
    let inner = if let Some(kind) = durable_global_kind(name, ctx, emit_env) {
        let base = if mutable {
            quote!((*::core::ptr::addr_of!(#ident)))
        } else {
            quote!(#ident)
        };
        match kind {
            DurableGlobal::OptionalString => quote!(
                #base.as_ref().map(|__s| ts_aot_runtime::JsString::Valid(
                    __ctx.arena.alloc_str(__s.as_str())
                ))
            ),
            DurableGlobal::String => {
                quote!(ts_aot_runtime::JsString::Valid(__ctx.arena.alloc_str(#base.as_str())))
            }
        }
    } else if owned
        && ctx
            .global_ty(name)
            .is_some_and(|t| !is_copy_type(emit_env.types, t))
    {
        quote!(#ident.clone())
    } else {
        quote!(#ident)
    };
    if mutable {
        quote!(unsafe { #inner })
    } else {
        quote!(#inner)
    }
}

fn emit_global_place_base(name: &Atom, body_ctx: &BodyCtx) -> TokenStream {
    if let Some(tokens) = body_ctx.shared_place_base_for_name(name) {
        return tokens;
    }
    let ident = ident_from(name);
    quote!(#ident)
}

fn emit_place_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Global(name) => Ok(emit_global_access(name, ctx, emit_env, body_ctx, false)),
        _ => emit_expr(expr, ctx, emit_env, body_ctx),
    }
}

fn emit_place_ref_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Global(name) => Ok(emit_global_place_base(name, body_ctx)),
        _ => emit_expr(expr, ctx, emit_env, body_ctx),
    }
}

fn emit_mut_receiver(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<(TokenStream, bool), BackendError> {
    if let MirExpr::Global(name) = expr
        && ctx.is_mutable_global(name)
        && body_ctx.shared_access_for_name(name).is_none()
    {
        return Ok((emit_place_ref_expr(expr, ctx, emit_env, body_ctx)?, true));
    }
    Ok((emit_place_expr(expr, ctx, emit_env, body_ctx)?, false))
}

fn emit_place(
    place: &MirPlace,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match place {
        MirPlace::Local { id } => Ok(body_ctx.local_ref(*id)),
        MirPlace::Global(name) => {
            let name = ident_from(name);
            Ok(quote!(#name))
        }
        MirPlace::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented(
                "field place: base type unresolved",
            ))?;
            let struct_id = types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented(
                    "field place: base is not a struct",
                ))?;
            let base = emit_place_base(base, ctx, emit_env, body_ctx)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!(#base.#field))
        }
        MirPlace::Index { base, index, .. } => {
            let base = emit_place_ref_expr(base, ctx, emit_env, body_ctx)?;
            let index = emit_expr(index, ctx, emit_env, body_ctx)?;
            Ok(quote!(#base[#index as usize]))
        }
    }
}

fn emit_place_base(
    base: &MirPlaceBase,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match base {
        MirPlaceBase::Local(id) => Ok(body_ctx.local_place_base(*id)),
        MirPlaceBase::Global(name) => Ok(emit_global_place_base(name, body_ctx)),
        MirPlaceBase::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented(
                "nested field place: base type unresolved",
            ))?;
            let struct_id = types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented(
                    "nested field place: base is not a struct",
                ))?;
            let base = emit_place_base(base, ctx, emit_env, body_ctx)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!(#base.#field))
        }
        MirPlaceBase::Index { base, index, .. } => {
            let base = emit_place_ref_expr(base, ctx, emit_env, body_ctx)?;
            let index = emit_expr(index, ctx, emit_env, body_ctx)?;
            Ok(quote!(#base[#index as usize]))
        }
        MirPlaceBase::Chain { base, .. } => emit_expr(base, ctx, emit_env, body_ctx),
    }
}

fn place_base_ty(base: &MirPlaceBase, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirPlaceBase::Local(id) => body_ctx.local_ty(*id),
        // A name-based capture is a `MirPlaceBase::Global` whose declared type
        // lives in the closure environment, not in the module globals.
        MirPlaceBase::Global(name) => body_ctx.shared_ty_for_name(name),
        other => other.ty(),
    }
}

fn expr_base_ty(base: &MirExpr, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirExpr::Local(id) => body_ctx.local_ty(*id).or_else(|| body_ctx.capture_ty(*id)),
        // A name-based capture (`x` captured through two levels of closure) is a
        // `MirExpr::Global` that carries no `TypeId`; the env field does.
        MirExpr::Global(name) => body_ctx.shared_ty_for_name(name),
        other => other.ty(),
    }
}

fn value_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TypeId> {
    match expr {
        MirExpr::Call { callee, .. } => {
            let mir_ty = expr_base_ty(expr, body_ctx);
            match mir_ty.and_then(|t| emit_env.types.resolve(t)) {
                // A method call through an optional receiver (`c?.m()`) is typed
                // `T | undefined` by the passes, which is more precise than the
                // declared return type of the method.
                Some(Type::Optional { .. }) => mir_ty,
                _ => ctx.function_ret(*callee).or(mir_ty),
            }
        }
        MirExpr::IndirectCall { callee, .. } => {
            let callee_ty = expr_base_ty(callee, body_ctx)?;
            match emit_env.types.resolve(callee_ty) {
                Some(Type::Fn { ret, .. }) => Some(*ret),
                _ => expr_base_ty(expr, body_ctx),
            }
        }
        MirExpr::Global(name) => ctx.global_ty(name).or_else(|| expr_base_ty(expr, body_ctx)),
        // A negation carries the frontend's placeholder type, but the Rust value
        // it emits has the operand's type (`-y` for `y: i64` is an `i64`). The
        // binary/argument/return coercions take the operand type to pick a common
        // numeric type, so `1.5 + -y` widens the negation instead of leaving an
        // `i64` in an `f64` position. `!` (`UnaryOp::Not`) yields a `bool` and is
        // deliberately not covered.
        MirExpr::Unary {
            op: UnaryOp::Neg,
            expr: operand,
            ..
        } => value_ty(operand, ctx, emit_env, body_ctx),
        // `~` is a Rust integer operation (`!`), so its value is an integer even
        // when the operand's own type is a float. A float operand has no integer
        // value type to read from the metadata, so that case keeps the
        // expression's placeholder type like the other arms that do not know the
        // emitted type.
        MirExpr::Unary {
            op: UnaryOp::BitNot,
            expr: operand,
            ..
        } => match value_ty(operand, ctx, emit_env, body_ctx)
            .and_then(|ty| emit_env.types.resolve(ty).map(|t| (ty, t)))
        {
            Some((ty, t)) if is_integer_type(t) => Some(ty),
            _ => expr_base_ty(expr, body_ctx),
        },
        // A binary arithmetic expression carries the frontend's placeholder type
        // too, while the Rust expression it emits has the numeric unification of
        // its operands: a float operand makes the result a float, two integers
        // keep an integer. Without this the assignment/return/argument
        // coercions see the placeholder and leave an `i64` where an `f64` is
        // required (`-(a + b)`, `1.5 + a + b`), which is E0308.
        MirExpr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod,
            left,
            right,
            ..
        } => numeric_binary_result_ty(
            value_ty(left, ctx, emit_env, body_ctx),
            value_ty(right, ctx, emit_env, body_ctx),
            emit_env,
        )
        .or_else(|| expr_base_ty(expr, body_ctx)),
        MirExpr::Field { base, field, .. } => value_ty(base, ctx, emit_env, body_ctx)
            .and_then(|base_ty| struct_field_type(emit_env, base_ty, *field))
            .or_else(|| expr_base_ty(expr, body_ctx)),
        _ => expr_base_ty(expr, body_ctx),
    }
}

pub(super) fn type_requires_arena(ty: TypeId, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    let EmitEnv { types, .. } = emit_env;
    match types.resolve(ty) {
        Some(Type::String | Type::Array { .. }) => true,
        Some(Type::Struct { id }) => ctx.struct_needs_lifetime(*id),
        Some(Type::Named { .. }) => types
            .struct_id(ty)
            .is_some_and(|id| ctx.struct_needs_lifetime(id)),
        Some(Type::Promise { ok, .. }) => type_requires_arena(*ok, ctx, emit_env),
        Some(Type::Result { ok, err }) => {
            type_requires_arena(*ok, ctx, emit_env) || type_requires_arena(*err, ctx, emit_env)
        }
        Some(
            Type::Optional { inner, .. }
            | Type::Generator { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner },
        ) => type_requires_arena(*inner, ctx, emit_env),
        Some(Type::WeakMap { value, .. }) => type_requires_arena(*value, ctx, emit_env),
        Some(Type::Fn { .. }) => ctx.fn_value_kind(ty).is_some(),
        _ => false,
    }
}

fn emit_throw_payload(
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

fn struct_field_type(emit_env: &EmitEnv, ty: TypeId, field: FieldId) -> Option<TypeId> {
    let id = emit_env.types.struct_id(ty)?;
    struct_field_decl_ty(emit_env, id, field)
}

/// The *declared* type of `field` in `struct_id`, read from the MIR struct
/// declaration. Unlike the type of an access expression (which the frontend
/// already flattens to a single `Optional` for `a?.b`), this tells whether the
/// field itself is nullable - the distinction the optional-chain emission needs.
fn struct_field_decl_ty(emit_env: &EmitEnv, struct_id: StructId, field: FieldId) -> Option<TypeId> {
    emit_env.program.declarations.iter().find_map(|d| match d {
        MirDecl::Struct(s) if s.id == struct_id => {
            s.fields.iter().find(|f| f.id == field).map(|f| f.ty)
        }
        _ => None,
    })
}

/// The inner type of an `OptionalChain` expression, i.e. the receiver's type
/// with the `| null`/`| undefined` part stripped (`Optional<C>` -> `C`).
fn optional_chain_inner_ty(emit_env: &EmitEnv, base: &MirExpr) -> Option<TypeId> {
    let MirExpr::OptionalChain { ty, .. } = base else {
        return None;
    };
    match emit_env.types.resolve(*ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => Some(*inner),
        _ => None,
    }
}

fn emit_same_value(
    types: &TypeTable,
    a_ty: &Type,
    b_ty: &Type,
    operands: (TokenStream, TokenStream),
    same_place: bool,
    depth: usize,
) -> Result<TokenStream, BackendError> {
    fn supported(types: &TypeTable, ty: &Type) -> bool {
        match ty {
            Type::Optional { inner, .. } => {
                types.resolve(*inner).is_some_and(|t| supported(types, t))
            }
            Type::Void
            | Type::Null
            | Type::Struct { .. }
            | Type::Named { .. }
            | Type::Array { .. }
            | Type::Bool
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::String
            | Type::Symbol => true,
            _ => false,
        }
    }
    let (a, b) = operands;
    let unsupported = || BackendError::NotImplemented("Object.is: unsupported operand type");
    if !supported(types, a_ty) || !supported(types, b_ty) {
        return Err(unsupported());
    }
    let left = format_ident!("__ts_aot_left_{}", depth);
    let right = format_ident!("__ts_aot_right_{}", depth);
    match (a_ty, b_ty) {
        (Type::Optional { inner: x, .. }, Type::Optional { inner: y, .. }) => {
            let inner = emit_same_value(
                types,
                types.resolve(*x).ok_or_else(unsupported)?,
                types.resolve(*y).ok_or_else(unsupported)?,
                (quote!(#left), quote!(#right)),
                same_place,
                depth + 1,
            )?;
            Ok(quote!(match (#a, #b) {
                (Some(#left), Some(#right)) => #inner,
                (None, None) => true,
                _ => false,
            }))
        }
        (Type::Optional { inner, .. }, _) => {
            let comparison = emit_same_value(
                types,
                types.resolve(*inner).ok_or_else(unsupported)?,
                b_ty,
                (quote!(#left), b.clone()),
                same_place,
                depth + 1,
            )?;
            let absent = matches!(b_ty, Type::Void);
            Ok(quote!(match #a { Some(#left) => #comparison, None => #absent }))
        }
        (_, Type::Optional { .. }) => {
            emit_same_value(types, b_ty, a_ty, (b, a), same_place, depth + 1)
        }
        (Type::Void, Type::Void) | (Type::Null, Type::Null) => Ok(quote!(true)),
        (Type::Struct { .. } | Type::Named { .. }, Type::Struct { .. } | Type::Named { .. })
        | (Type::Array { .. }, Type::Array { .. }) => Ok(quote!(#same_place)),
        _ if a_ty == b_ty => Ok(quote!(__ts_aot_object_is(#a, #b))),
        _ if is_number_type(a_ty) && is_number_type(b_ty) => {
            Ok(quote!(__ts_aot_object_is(&(*#a as f64), &(*#b as f64))))
        }
        _ => Ok(quote!(false)),
    }
}

fn is_number_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
    )
}

fn same_place(a: &MirExpr, b: &MirExpr) -> bool {
    match (a, b) {
        (MirExpr::Local(x), MirExpr::Local(y)) => x == y,
        (
            MirExpr::Field {
                base: ba,
                field: fa,
                ..
            },
            MirExpr::Field {
                base: bb,
                field: fb,
                ..
            },
        ) => fa == fb && same_place(ba, bb),
        (
            MirExpr::Index {
                base: ba,
                index: ia,
                ..
            },
            MirExpr::Index {
                base: bb,
                index: ib,
                ..
            },
        ) => same_place(ba, bb) && same_place(ia, ib),
        (MirExpr::Int { value: x, .. }, MirExpr::Int { value: y, .. }) => x == y,
        (MirExpr::Bool(x), MirExpr::Bool(y)) => x == y,
        (MirExpr::String { id: x, .. }, MirExpr::String { id: y, .. }) => x == y,
        _ => false,
    }
}

fn is_copy_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.resolve(ty) {
        Some(
            Type::Bool
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Date
            | Type::Symbol,
        ) => true,
        Some(Type::Optional { inner, .. }) => is_copy_type(types, *inner),
        _ => false,
    }
}

fn is_non_copy_place(expr: &MirExpr, emit_env: &EmitEnv, body_ctx: &BodyCtx) -> bool {
    match expr {
        MirExpr::Local(id) => {
            if body_ctx.shared_access_for_local(*id).is_some() || body_ctx.is_move_safe(*id) {
                return false;
            }
            body_ctx
                .local_ty(*id)
                .is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        MirExpr::Field { base, .. } => {
            if is_shared_local_base(base, body_ctx) {
                return false;
            }
            expr.ty().is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        MirExpr::Index { base, index, .. } => {
            let simple_index = matches!(
                index.as_ref(),
                MirExpr::Int { .. }
                    | MirExpr::Float { .. }
                    | MirExpr::Bool(_)
                    | MirExpr::Local(_)
                    | MirExpr::Global(_)
            );
            if simple_index && is_shared_local_base(base, body_ctx) {
                return false;
            }
            expr.ty().is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        _ => false,
    }
}

fn is_shared_local_base(base: &MirExpr, body_ctx: &BodyCtx) -> bool {
    matches!(base, MirExpr::Local(id)
        if body_ctx
            .shared_access_for_local(*id)
            .is_some_and(|acc| acc.storage == SharedStorage::RefCell))
}

fn emit_owned_value(
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

fn emit_owned_exprs(
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

fn emit_owned_exprs_coerced(
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
            Ok(numeric_coerce(e, tokens, target, ctx, emit_env, body_ctx))
        })
        .collect()
}

fn indirect_callee_fn_kind(
    callee: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<usize> {
    let EmitEnv { types, .. } = emit_env;
    let fn_ty = match callee {
        MirExpr::Local(id) => body_ctx.local_ty(*id)?,
        MirExpr::Global(name) => return ctx.fn_value_for_name(name).map(|(k, _)| k),
        other => other.ty()?,
    };
    if !matches!(types.resolve(fn_ty), Some(Type::Fn { .. })) {
        return None;
    }
    ctx.fn_value_kind(fn_ty)
}

fn is_display_supported_reason(reason_ty: Option<&Type>) -> bool {
    matches!(
        reason_ty,
        Some(Type::I32 | Type::I64 | Type::F64 | Type::String | Type::Bool)
    )
}

fn optional_chain_map_arm(
    base: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let MirExpr::OptionalChain { base: inner, ty } = base else {
        return Ok(None);
    };
    let Some(resolved) = types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!(#inner_tokens.as_ref())))
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
    let Some(resolved) = types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    let args_tokens = emit_exprs(args, ctx, emit_env, body_ctx)?;
    Ok(Some(
        quote!(#inner_tokens.as_ref().map(|f| f(#(#args_tokens),*))),
    ))
}

fn emit_optional_chain_assign(
    target: &MirPlace,
    value: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let MirPlace::Field { base, field, .. } = target else {
        return Ok(None);
    };
    let MirPlaceBase::Chain {
        base: chain_base, ..
    } = base.as_ref()
    else {
        return Ok(None);
    };
    let MirExpr::OptionalChain { base: inner, ty } = chain_base.as_ref() else {
        return Ok(None);
    };
    let Some(resolved) = types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, emit_env, body_ctx)?;
    let inner_ty = match types.resolve(*ty) {
        Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
        _ => {
            return Err(BackendError::NotImplemented(
                "optional field assign: target not Optional",
            ));
        }
    };
    let field_ident = match types.resolve(inner_ty) {
        Some(_) => {
            let struct_id = types
                .struct_id(inner_ty)
                .ok_or(BackendError::NotImplemented(
                    "optional field assign: base is not a struct",
                ))?;
            ctx.struct_field_ident(struct_id, *field)
        }
        None => {
            return Err(BackendError::NotImplemented(
                "optional field assign: inner type unresolved",
            ));
        }
    };
    let value = emit_expr(value, ctx, emit_env, body_ctx)?;
    Ok(Some(quote! {
        if #inner_tokens.is_some() {
            #inner_tokens.as_mut().unwrap().#field_ident = #value;
        }
    }))
}

fn emit_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match expr {
        MirExpr::Unit | MirExpr::Null { .. } => Ok(quote!(())),
        MirExpr::Bool(value) => Ok(quote!(#value)),
        MirExpr::Int { value, ty } => Ok(
            if matches!(emit_env.types.resolve(*ty), Some(Type::F64 | Type::F32)) {
                emit_float_from_int(*value)
            } else {
                emit_whole_number_literal(*value)
            },
        ),
        MirExpr::Float { value, .. } => Ok(emit_float(*value)),
        MirExpr::String { id, .. } => {
            let literal = Literal::string(id.as_str());
            Ok(quote!(ts_aot_runtime::JsString::from(#literal)))
        }
        MirExpr::Local(id) => Ok(body_ctx.local_read(*id)),
        // `expr?`: the passes only emit this when the enclosing function
        // declares the same `throws` type, so `?` type-checks directly.
        MirExpr::Propagate { expr, .. } => {
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(#inner?))
        }
        MirExpr::Global(name) => Ok(emit_global_access(name, ctx, emit_env, body_ctx, true)),
        MirExpr::Field { base, field, .. } => {
            if let Some(map) = optional_chain_map_arm(base, ctx, emit_env, body_ctx)? {
                let base_ty = expr_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented(
                    "optional-chain field: base type unresolved",
                ))?;
                let inner_ty = match types.resolve(base_ty) {
                    Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
                    _ => {
                        return Err(BackendError::NotImplemented(
                            "optional-chain field: base not Optional",
                        ));
                    }
                };
                let struct_id = types
                    .struct_id(inner_ty)
                    .ok_or(BackendError::NotImplemented(
                        "optional-chain field: base is not a struct",
                    ))?;
                let field_id = *field;
                let field_ty = struct_field_decl_ty(emit_env, struct_id, field_id);
                let field = ctx.struct_field_ident(struct_id, field_id);
                // `a?.b` yields exactly one level of absence in JavaScript: when
                // the field itself is optional (`b: T | null`), `null` and
                // "`a` was nullish" are the same absent value, so the access must
                // flatten (`and_then`) instead of nesting (`map` would give
                // `Option<Option<T>>`). The type of the *access expression* cannot
                // tell the two apart (the frontend already flattens it), so the
                // decision comes from the field's declared type. Both `map` and
                // `and_then` hand the receiver to the closure by shared reference,
                // so a non-`Copy` field has to be cloned out of it - otherwise the
                // closure moves the field from behind `&` (E0507).
                let access = if field_ty.is_some_and(|ty| !is_copy_type(types, ty)) {
                    quote!(o.#field.clone())
                } else {
                    quote!(o.#field)
                };
                if let Some(field_ty) = field_ty
                    && matches!(
                        types.resolve(field_ty),
                        Some(ts_aot_core::Type::Optional { .. })
                    )
                {
                    return Ok(quote!(#map.and_then(|o| #access)));
                }
                return Ok(quote!(#map.map(|o| #access)));
            }
            if let Some(base_ty) = expr_base_ty(base, body_ctx)
                && matches!(
                    types.resolve(base_ty),
                    Some(ts_aot_core::Type::AsyncGenYield { .. })
                )
            {
                let field_ident = if field.raw() == 0 {
                    format_ident!("value")
                } else {
                    format_ident!("done")
                };
                let base = emit_expr(base, ctx, emit_env, body_ctx)?;
                return Ok(quote!(#base.#field_ident));
            }
            let base_ty = value_ty(base, ctx, emit_env, body_ctx).ok_or(
                BackendError::NotImplemented(match base.as_ref() {
                    MirExpr::Local(_) => "field base unresolved: local",
                    MirExpr::Global(_) => "field base unresolved: global",
                    MirExpr::Call { .. } => "field base unresolved: call",
                    MirExpr::IndirectCall { .. } => "field base unresolved: indirect-call",
                    MirExpr::Field { .. } => "field base unresolved: nested-field",
                    MirExpr::Index { .. } => "field base unresolved: index",
                    MirExpr::Cast { .. } => "field base unresolved: cast",
                    _ => "field base unresolved: other",
                }),
            )?;
            let struct_id = types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented(
                    "field access: base is not a struct",
                ))?;
            let field = ctx.struct_field_ident(struct_id, *field);
            if let MirExpr::Local(id) = base.as_ref()
                && let Some(acc) = body_ctx.shared_access_for_local(*id)
                && let Some(borrowed) = shared_borrow_tokens(&acc)
            {
                return Ok(quote!(#borrowed.#field.clone()));
            }
            let base = emit_expr(base, ctx, emit_env, body_ctx)?;
            Ok(quote!(#base.#field))
        }
        MirExpr::Index { base, index, .. } => {
            let simple_index = matches!(
                index.as_ref(),
                MirExpr::Int { .. }
                    | MirExpr::Float { .. }
                    | MirExpr::Bool(_)
                    | MirExpr::Local(_)
                    | MirExpr::Global(_)
            );
            let index = emit_expr(index, ctx, emit_env, body_ctx)?;
            if let Some(map) = optional_chain_map_arm(base, ctx, emit_env, body_ctx)? {
                // A string receiver is not indexed as a container: `s?.[i]` on a
                // `string | null` has to use the same string-index helper as the
                // plain `s[i]` path below. Indexing the emitted `JsString`/`String`
                // directly would be E0608, and the short-circuit for `None` must
                // stay (`map` over the receiver's `as_ref()`), so the helper is
                // called inside the closure.
                if let Some(inner_ty) = optional_chain_inner_ty(emit_env, base)
                    && matches!(types.resolve(inner_ty), Some(Type::String))
                {
                    return Ok(quote!(#map.map(|o| ts_aot_runtime::__ts_aot_string_char_at(
                        __ctx,
                        o,
                        #index as i64,
                    ))));
                }
                // Same flattening rule as for fields: an element that is itself
                // optional (`(T | null)[]`) must not nest the two absences.
                // The element type comes from the receiver's array type, not from
                // the access expression, whose type the frontend already flattened.
                let element_ty = optional_chain_inner_ty(emit_env, base).and_then(|inner| {
                    match types.resolve(inner) {
                        Some(Type::Array { element }) => Some(*element),
                        _ => None,
                    }
                });
                // Same clone rule as for `?.field` above: the element comes out of
                // a shared reference, so a non-`Copy` element type must be cloned
                // in both the flattening (`and_then`) and the nesting (`map`) arm.
                let access = if element_ty.is_some_and(|ty| !is_copy_type(types, ty)) {
                    quote!(o[#index as usize].clone())
                } else {
                    quote!(o[#index as usize])
                };
                if let Some(element_ty) = element_ty
                    && matches!(
                        types.resolve(element_ty),
                        Some(ts_aot_core::Type::Optional { .. })
                    )
                {
                    return Ok(quote!(#map.and_then(|o| #access)));
                }
                return Ok(quote!(#map.map(|o| #access)));
            }
            if matches!(
                value_ty(base, ctx, emit_env, body_ctx).and_then(|t| emit_env.types.resolve(t)),
                Some(Type::String)
            ) {
                let base = emit_expr(base, ctx, emit_env, body_ctx)?;
                return Ok(quote!(ts_aot_runtime::__ts_aot_string_char_at(
                    __ctx,
                    &#base,
                    #index as i64,
                )));
            }
            if simple_index
                && let MirExpr::Local(id) = base.as_ref()
                && let Some(acc) = body_ctx.shared_access_for_local(*id)
                && let Some(borrowed) = shared_borrow_tokens(&acc)
            {
                return Ok(quote!(#borrowed[#index as usize].clone()));
            }
            let base = emit_expr(base, ctx, emit_env, body_ctx)?;
            Ok(quote!(#base[#index as usize]))
        }
        MirExpr::Call { callee, args, .. } => {
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
            // `c?.m(...)`: the receiver is an optional value, so the call is
            // mapped over it and the result is `Option<ret>` (the passes type
            // the call that way).
            if is_method
                && let Some(MirExpr::OptionalChain { base, ty }) = args.first()
                && let Some(owner_ident) = owner_ident.clone()
                && matches!(emit_env.types.resolve(*ty), Some(Type::Optional { .. }))
            {
                let receiver = if is_mutating_method {
                    match base.as_ref() {
                        MirExpr::Local(id) => body_ctx.local_ref(*id),
                        other => emit_expr(other, ctx, emit_env, body_ctx)?,
                    }
                } else {
                    emit_expr(base, ctx, emit_env, body_ctx)?
                };
                let rest = &args[1..];
                let rest_param_tys = param_tys.map(|tys| &tys[1..]);
                let rest = emit_owned_exprs_coerced(rest, rest_param_tys, ctx, emit_env, body_ctx)?;
                let call = if is_mutating_method {
                    quote!(#owner_ident::#callee_ident(__ts_recv, __ctx, #(#rest),*))
                } else {
                    quote!(#owner_ident::#callee_ident(__ts_recv.clone(), __ctx, #(#rest),*))
                };
                let call = if is_async {
                    quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call))
                } else {
                    call
                };
                return Ok(if is_mutating_method {
                    quote!(#receiver.as_mut().map(|__ts_recv| #call))
                } else {
                    quote!(#receiver.as_ref().map(|__ts_recv| #call))
                });
            }
            let callee = if let Some(owner_ident) = owner_ident {
                quote!(#owner_ident::#callee_ident)
            } else {
                quote!(#callee_ident)
            };
            if is_method && (is_constructor || is_mutating_method) {
                // A constructor mutates the instance the caller allocated, and a
                // method that writes a field of `this` mutates the instance it is
                // called on, so the receiver is borrowed (`&mut self`) instead of
                // being moved or cloned.
                let (receiver, rest) = args
                    .split_first()
                    .expect("a method call always carries its receiver");
                // The receiver must be emitted as a *place*: a value read of `this`
                // clones it (`&mut (self.clone())` mutates a temporary), while
                // `local_ref` keeps the borrow (`&mut (self)`, `&mut (c)`).
                let receiver = match receiver {
                    // Inside a mutating method `self` is already a `&mut` binding, so
                    // the place is a reborrow (`&mut (*self)`); `&mut (self)` would
                    // borrow the binding, which needs `mut self`.
                    MirExpr::Local(id) if Some(*id) == body_ctx.self_param() => {
                        Ok(if body_ctx.self_is_borrowed() {
                            quote!((*self))
                        } else {
                            quote!((self))
                        })
                    }
                    // A captured (shared) local lives in the closure environment, so
                    // it has no ident to borrow: it keeps the by-value form.
                    MirExpr::Local(id) if body_ctx.shared_access_for_local(*id).is_none() => {
                        Ok(body_ctx.local_ref(*id))
                    }
                    other => emit_expr(other, ctx, emit_env, body_ctx),
                }?;
                let rest = emit_owned_exprs_coerced(
                    rest,
                    param_tys.map(|tys| &tys[1..]),
                    ctx,
                    emit_env,
                    body_ctx,
                )?;
                return Ok(quote!(#callee(&mut (#receiver), __ctx, #(#rest),*)));
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
        MirExpr::IndirectCall { callee, args, .. } => {
            if let Some(map) = optional_call_map_arm(callee, args, ctx, emit_env, body_ctx)? {
                return Ok(map);
            }
            if let Some(k) = indirect_callee_fn_kind(callee, ctx, emit_env, body_ctx) {
                let callee = emit_expr(callee, ctx, emit_env, body_ctx)?;
                // The generated `__ts_fn_call_k` helper is typed by the `Fn` type
                // of the callee, so its arguments need the same coercion a direct
                // call gets: numeric widening and `Some(..)`/`None` for optional
                // parameters (an omitted `?`-parameter reaches us as `MirExpr::Unit`).
                let sig = ctx.fn_value_sig(k);
                let param_tys = sig.params.clone();
                let param_optional = sig.optional.clone();
                let mut coerced_args: Vec<MirExpr> = args.clone();
                while coerced_args.len() < param_tys.len() {
                    // Only a *declared* optional parameter may be left out: the
                    // signature's flag, never an `Option` in the ABI type alone -
                    // `x: T | undefined` without `?` is a required parameter.
                    let index = coerced_args.len();
                    if !param_optional.get(index).copied().unwrap_or(false) {
                        break;
                    }
                    coerced_args.push(MirExpr::Unit);
                }
                let args = emit_owned_exprs_coerced(
                    &coerced_args,
                    Some(&param_tys),
                    ctx,
                    emit_env,
                    body_ctx,
                )?;
                let helper = EmitCtx::fn_call_ident(k);
                return Ok(quote!(#helper(__ctx, &#callee, #(#args),*)));
            }
            let callee = emit_expr(callee, ctx, emit_env, body_ctx)?;
            let args = emit_owned_exprs(args, ctx, emit_env, body_ctx)?;
            Ok(quote!(#callee(#(#args),*)))
        }
        MirExpr::StructLiteral {
            struct_id, fields, ..
        } => {
            let name = ctx.struct_ident(*struct_id);
            let declared: Vec<(FieldId, TypeId)> = emit_env
                .program
                .declarations
                .iter()
                .find_map(|d| match d {
                    MirDecl::Struct(s) if s.id == *struct_id => {
                        Some(s.fields.iter().map(|f| (f.id, f.ty)).collect())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            let field_tys: std::collections::HashMap<FieldId, TypeId> =
                declared.iter().copied().collect();
            let mut emitted: Vec<TokenStream> = Vec::with_capacity(fields.len());
            for (field_id, value) in fields {
                let field = ctx.struct_field_ident(*struct_id, *field_id);
                let value_tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
                let value_tokens = match field_tys.get(field_id) {
                    Some(fty) => {
                        let coerced =
                            numeric_coerce(value, value_tokens, *fty, ctx, emit_env, body_ctx);
                        optional_coerce(value, coerced, *fty, ctx, emit_env, body_ctx, false)?
                    }
                    None => value_tokens,
                };
                emitted.push(quote!(#field: #value_tokens));
            }
            let listed: HashSet<FieldId> = fields.iter().map(|(id, _)| *id).collect();
            let mut budget = super::CONST_DEFAULT_BUDGET;
            for (fid, fty) in declared {
                if listed.contains(&fid) {
                    continue;
                }
                let fname = ctx.struct_field_ident(*struct_id, fid);
                let fdef = super::emit_const_default(
                    fty,
                    ctx,
                    emit_env,
                    0,
                    &mut budget,
                    super::types::LifetimeMode::Full,
                )?;
                emitted.push(quote!(#fname: #fdef));
            }
            if ctx.struct_needs_lifetime(*struct_id) {
                Ok(quote!(#name {
                    #(#emitted,)*
                    _marker: ::core::marker::PhantomData,
                }))
            } else {
                Ok(quote!(#name { #(#emitted),* }))
            }
        }
        MirExpr::ResultOk { value, .. } => {
            let value = emit_expr(value, ctx, emit_env, body_ctx)?;
            Ok(quote!(Ok(#value)))
        }
        MirExpr::ResultErr { error, .. } => {
            let error = emit_expr(error, ctx, emit_env, body_ctx)?;
            Ok(quote!(Err(#error)))
        }
        MirExpr::Binary {
            op, left, right, ..
        } => {
            let bare = emit_binary_expr(*op, left, right, ctx, emit_env, body_ctx)?;
            Ok(quote!((#bare)))
        }
        MirExpr::Unary { op, expr, .. } => {
            let bare = emit_unary_expr(*op, expr, ctx, emit_env, body_ctx)?;
            Ok(quote!((#bare)))
        }
        MirExpr::Await { expr, ty, .. } => {
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
        MirExpr::OptionalChain { base, .. } => emit_expr(base, ctx, emit_env, body_ctx),
        MirExpr::IsSome { expr, .. } => {
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(#inner.is_some()))
        }
        MirExpr::UnwrapOptional { expr, .. } => {
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(#inner.unwrap()))
        }
        MirExpr::TypeOf { expr, .. } => emit_typeof(expr, ctx, emit_env, body_ctx),
        MirExpr::Cast { expr, ty } => {
            let bare = emit_cast_expr(expr, *ty, ctx, emit_env, body_ctx)?;
            Ok(quote!((#bare)))
        }
        MirExpr::TemplateStringsArray { cooked, .. } => {
            let cooked_lits: Vec<TokenStream> = cooked
                .iter()
                .map(|p| {
                    let lit = Literal::string(p.as_str());
                    quote!(ts_aot_runtime::JsString::from(#lit))
                })
                .collect();
            Ok(quote!(
                ts_aot_runtime::bumpalo::vec![in __ctx.arena; #(#cooked_lits),*]
            ))
        }
        MirExpr::RegExp { pattern, flags, .. } => {
            let pattern_lit = Literal::string(pattern);
            let flags_lit = Literal::string(flags);
            Ok(quote!(ts_aot_runtime::__ts_aot_regex_new(#pattern_lit, #flags_lit)))
        }
        MirExpr::BigInt { value, .. } => {
            let value_lit = Literal::string(value);
            Ok(quote!(ts_aot_runtime::__ts_aot_bigint_new(#value_lit)))
        }
        MirExpr::Import { source, ty } => {
            let source = emit_expr(source, ctx, emit_env, body_ctx)?;
            let payload_ty = emit_type_id_with_ctx(*ty, emit_env, ctx);
            Ok(
                quote!(ts_aot_runtime::__ts_aot_dynamic_import::<#payload_ty>(
                    &#source.to_string_lossy()
                )),
            )
        }
        MirExpr::Yield { expr, .. } => {
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
        MirExpr::Closure {
            id: _,
            params,
            captures,
            locals,
            body,
            ret_ty,
            fn_ty,
        } => {
            if let Some(sig) = ctx.fn_value_kind(*fn_ty) {
                return emit_closure_value(
                    &ClosureEmit {
                        sig,
                        params,
                        captures,
                        locals,
                        body,
                        ret_ty: *ret_ty,
                    },
                    ctx,
                    emit_env,
                    body_ctx,
                );
            }
            let mut child_ctx = BodyCtx::for_closure(
                *ret_ty,
                body_ctx.shared_name_map(),
                body_ctx.shared_id_map(),
            );
            child_ctx.inherit_dynamic(body_ctx);
            child_ctx.seed_dynamic(body, params, locals, types);
            child_ctx.seed_move_safe(body, locals);
            let mut candidates: std::collections::HashSet<LocalId> =
                params.iter().map(|p| p.id).collect();
            for l in locals {
                candidates.insert(l.id);
            }
            let assigned: std::collections::HashSet<LocalId> =
                collect_assigned_locals(&body.stmts, types, &candidates);
            for l in locals {
                let mutable = l.mutable || assigned.contains(&l.id);
                let name = ident_from(&l.name);
                child_ctx.register_local(l.id, name, l.ty, mutable);
            }
            for p in params {
                let mutable = assigned.contains(&p.id);
                child_ctx.register_local(p.id, ident_from(&p.name), p.ty, mutable);
            }
            let param_tokens: Vec<TokenStream> = params
                .iter()
                .map(|p| {
                    let name = child_ctx.local_ident(p.id);
                    let ty = emit_type_id_with_ctx(p.ty, emit_env, ctx);
                    if assigned.contains(&p.id) {
                        quote!(mut #name: #ty)
                    } else {
                        quote!(#name: #ty)
                    }
                })
                .collect();
            let ret_ty_tokens = emit_type_id_with_ctx(*ret_ty, emit_env, ctx);
            let mut inner_stmts = emit_block_stmts(body, ctx, emit_env, &child_ctx)?;
            if block_contains_try(body) {
                inner_stmts.insert(0, return_signal_enum());
            }
            let stmts = if child_ctx.has_weakmap_liveness() {
                let cache = child_ctx.weakmap_liveness_per_key.borrow();
                let mut prelude: Vec<TokenStream> =
                    cache.values().map(emit_weakmap_liveness_binding).collect();
                prelude.extend(inner_stmts);
                prelude
            } else {
                inner_stmts
            };
            Ok(quote!(move |#(#param_tokens),*| -> #ret_ty_tokens { #(#stmts)* }))
        }
    }
}

fn collect_global_names(block: &MirBlock, out: &mut Vec<Atom>) {
    for stmt in &block.stmts {
        collect_global_names_stmt(stmt, out);
    }
}

fn collect_global_names_stmt(stmt: &MirStmt, out: &mut Vec<Atom>) {
    match stmt {
        MirStmt::Let { init: Some(e), .. } | MirStmt::Expr(e) | MirStmt::Return(Some(e)) => {
            collect_global_names_expr(e, out);
        }
        MirStmt::Let { init: None, .. }
        | MirStmt::Return(None)
        | MirStmt::Break
        | MirStmt::Continue => {}
        MirStmt::Assign { target, value } => {
            collect_global_names_place(target, out);
            collect_global_names_expr(value, out);
        }
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            collect_global_names_expr(error, out);
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_global_names_expr(cond, out);
            collect_global_names(then_block, out);
            if let Some(b) = else_block {
                collect_global_names(b, out);
            }
        }
        MirStmt::While { cond, body } | MirStmt::DoWhile { body, cond } => {
            collect_global_names_expr(cond, out);
            collect_global_names(body, out);
        }
        MirStmt::ForOf { iterable, body, .. } | MirStmt::ForAwaitOf { iterable, body, .. } => {
            collect_global_names_expr(iterable, out);
            collect_global_names(body, out);
        }
        MirStmt::ForIn { object, body, .. } => {
            collect_global_names_expr(object, out);
            collect_global_names(body, out);
        }
        MirStmt::Runtime { args, .. } => {
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            collect_global_names_expr(disc, out);
            for c in cases {
                collect_global_names(&c.body, out);
            }
            if let Some(b) = default {
                collect_global_names(b, out);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_global_names(body, out);
            if let Some(b) = catch {
                collect_global_names(b, out);
            }
            if let Some(b) = finally {
                collect_global_names(b, out);
            }
        }
    }
}

fn collect_global_names_place(place: &MirPlace, out: &mut Vec<Atom>) {
    match place {
        MirPlace::Local { .. } => {}
        MirPlace::Global(name) => out.push(name.clone()),
        MirPlace::Field { base, .. } => collect_global_names_place_base(base, out),
        MirPlace::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
    }
}

fn collect_global_names_place_base(base: &MirPlaceBase, out: &mut Vec<Atom>) {
    match base {
        MirPlaceBase::Local(_) => {}
        MirPlaceBase::Global(name) => out.push(name.clone()),
        MirPlaceBase::Field { base, .. } => collect_global_names_place_base(base, out),
        MirPlaceBase::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
        MirPlaceBase::Chain { base, .. } => collect_global_names_expr(base, out),
    }
}

fn collect_global_names_expr(expr: &MirExpr, out: &mut Vec<Atom>) {
    match expr {
        MirExpr::Global(name) => out.push(name.clone()),

        MirExpr::Closure { body, .. } => collect_global_names(body, out),
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. }
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Unary { expr: base, .. }
        | MirExpr::Await { expr: base, .. }
        | MirExpr::TypeOf { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::Import { source: base, .. } => collect_global_names_expr(base, out),
        MirExpr::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_global_names_expr(left, out);
            collect_global_names_expr(right, out);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            collect_global_names_expr(expr, out);
        }
        MirExpr::Yield { expr: Some(e), .. } | MirExpr::Propagate { expr: e, .. } => {
            collect_global_names_expr(e, out);
        }
        MirExpr::Call { args, .. } => {
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_global_names_expr(callee, out);
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_global_names_expr(v, out);
            }
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Yield { expr: None, .. } => {}
    }
}

fn emit_env_prelude(body_ctx: &BodyCtx, emit_env: &EmitEnv, ctx: &EmitCtx) -> Option<TokenStream> {
    let env = body_ctx.own_env()?;
    let fields: Vec<TokenStream> = env
        .fields
        .iter()
        .map(|f| {
            let field = &f.field;
            let t = if f.dynamic {
                quote!(ts_aot_runtime::CaughtValue)
            } else {
                emit_type_id_with_ctx(f.ty, emit_env, ctx)
            };
            match f.storage {
                SharedStorage::Cell => quote!(#field: std::cell::Cell<Option<#t>>),
                SharedStorage::RefCell => quote!(#field: std::cell::RefCell<Option<#t>>),
            }
        })
        .collect();
    let inits: Vec<TokenStream> = env
        .fields
        .iter()
        .map(|f| {
            let field = &f.field;
            let init = if let Some(param) = &f.param_init {
                quote!(Some(#param))
            } else {
                quote!(None)
            };
            match f.storage {
                SharedStorage::Cell => quote!(#field: std::cell::Cell::new(#init)),
                SharedStorage::RefCell => quote!(#field: std::cell::RefCell::new(#init)),
            }
        })
        .collect();
    let env_type = &env.env_type;
    let env_ident = &env.env_ident;
    ctx.push_env_struct(quote! {
        pub struct #env_type<'a> {
            #(#fields,)*
            _marker: ::core::marker::PhantomData<&'a ()>,
        }
    });
    Some(quote!(
        let #env_ident: std::rc::Rc<#env_type<'a>> = std::rc::Rc::new(#env_type {
            #(#inits,)*
            _marker: ::core::marker::PhantomData,
        });
    ))
}

struct ClosureEmit<'a> {
    sig: usize,
    params: &'a [MirParam],
    captures: &'a [LocalId],
    locals: &'a [MirLocalDecl],
    body: &'a MirBlock,
    ret_ty: TypeId,
}

fn emit_closure_value(
    spec: &ClosureEmit<'_>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let closure_id = ctx.alloc_closure_id();
    let sig = spec.sig;
    let params = spec.params;
    let captures = spec.captures;
    let locals = spec.locals;
    let body = spec.body;
    let ret_ty = spec.ret_ty;
    let EmitEnv { types, .. } = emit_env;

    let mut envs: Vec<(Ident, Ident)> = Vec::new();
    for cid in captures {
        if let Some(acc) = body_ctx.shared_access_for_local(*cid)
            && !envs.iter().any(|(env, _)| *env == acc.env_ident)
        {
            envs.push((acc.env_ident.clone(), acc.env_type.clone()));
        }
    }
    let mut names: Vec<ts_aot_core::Atom> = Vec::new();
    collect_global_names(body, &mut names);
    for name in &names {
        if let Some(acc) = body_ctx.shared_access_for_name(name)
            && !envs.iter().any(|(env, _)| *env == acc.env_ident)
        {
            envs.push((acc.env_ident.clone(), acc.env_type.clone()));
        }
    }
    let capture_tys: Vec<TokenStream> = envs
        .iter()
        .map(|(_, env_type)| quote!(std::rc::Rc<#env_type<'a>>))
        .collect();
    let variant = ctx.register_closure(closure_id, sig, capture_tys.clone());

    let mut child =
        BodyCtx::for_closure(ret_ty, body_ctx.shared_name_map(), body_ctx.shared_id_map());
    child.inherit_dynamic(body_ctx);
    child.seed_capture_types(captures, body_ctx);
    child.seed_dynamic(body, params, locals, types);
    child.seed_move_safe(body, locals);
    let mut candidates: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();
    for l in locals {
        candidates.insert(l.id);
    }
    let assigned = collect_assigned_locals(&body.stmts, types, &candidates);
    for l in locals {
        let name = ident_from(&l.name);
        child.forget_inherited_name(&name);
        let mutable = l.mutable || assigned.contains(&l.id);
        child.register_local(l.id, name, l.ty, mutable);
    }
    for p in params {
        let name = ident_from(&p.name);
        child.forget_inherited_name(&name);
        let mutable = assigned.contains(&p.id);
        child.register_local(p.id, name, p.ty, mutable);
    }
    child.detect_own_env(&closure_id.to_string(), body, params, locals, types);

    let mut param_tokens: Vec<TokenStream> = Vec::new();
    for ((env_ident, _), ty) in envs.iter().zip(capture_tys.iter()) {
        param_tokens.push(quote!(#env_ident: #ty));
    }
    for p in params {
        let name = child.local_ident(p.id);
        let ty = emit_type_id_with_ctx(p.ty, emit_env, ctx);
        if assigned.contains(&p.id) {
            param_tokens.push(quote!(mut #name: #ty));
        } else {
            param_tokens.push(quote!(#name: #ty));
        }
    }
    let mut inner_stmts = emit_block_stmts(body, ctx, emit_env, &child)?;
    if let Some(prelude) = emit_env_prelude(&child, emit_env, ctx) {
        inner_stmts.insert(0, prelude);
    }
    if block_contains_try(body) {
        inner_stmts.insert(0, return_signal_enum());
    }
    let ret_tokens = emit_type_id_with_ctx(ret_ty, emit_env, ctx);
    let fn_ident = format_ident!("__ts_closure_{}", closure_id);
    ctx.push_closure_fn(quote!(
        fn #fn_ident<'a>(
            __ctx: &ts_aot_runtime::Ctx<'a>,
            #(#param_tokens),*
        ) -> #ret_tokens {
            #(#inner_stmts)*
        }
    ));

    let enum_name = EmitCtx::fn_value_ident(sig);
    if envs.is_empty() {
        Ok(quote!(#enum_name::#variant))
    } else {
        let fields: Vec<TokenStream> = envs
            .iter()
            .enumerate()
            .map(|(i, (env_ident, _))| {
                let field = format_ident!("c{}", i);
                quote!(#field: #env_ident.clone())
            })
            .collect();
        Ok(quote!(#enum_name::#variant { #(#fields),* }))
    }
}

fn emit_value_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Binary {
            op, left, right, ..
        } => emit_binary_expr(*op, left, right, ctx, emit_env, body_ctx),
        MirExpr::Unary { op, expr, .. } => emit_unary_expr(*op, expr, ctx, emit_env, body_ctx),
        MirExpr::Cast { expr, ty } => emit_cast_expr(expr, *ty, ctx, emit_env, body_ctx),
        _ => emit_expr(expr, ctx, emit_env, body_ctx),
    }
}

fn emit_exprs(
    exprs: &[MirExpr],
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    exprs
        .iter()
        .map(|expr| emit_expr(expr, ctx, emit_env, body_ctx))
        .collect()
}

fn emit_binary_expr(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) = emit_dynamic_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) =
        emit_nullish_literal_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    if matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe
    ) && let Some(tokens) =
        emit_mixed_optional_equality(op, left, right, ctx, emit_env, body_ctx)?
    {
        return Ok(tokens);
    }
    // Two optional operands are compared with Rust's `Option` equality below,
    // where `null` and `undefined` share one `Option::None`. That is JavaScript's
    // answer only while both sides carry the same nullish kinds, so a strict
    // comparison whose kinds make the `None`/`None` case ambiguous is rejected
    // instead of silently answering `true`.
    check_optional_strict_equality_kinds(op, left, right, ctx, emit_env, body_ctx)?;
    // The same `Option` comparison also needs both inner types to be the *same*
    // Rust type and to implement `PartialEq`; when the metadata cannot supply
    // that, the emitted `a == b` would not compile (E0308 for two different
    // numeric inner types, E0369 for a structure without `PartialEq`).
    check_optional_strict_equality_inner_types(op, left, right, ctx, emit_env, body_ctx)?;
    let target = if matches!(
        op,
        BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::StrictEq
            | BinaryOp::StrictNe
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
    ) {
        common_numeric_target(
            value_ty(left, ctx, emit_env, body_ctx),
            value_ty(right, ctx, emit_env, body_ctx),
            emit_env,
        )
    } else {
        None
    };
    let mut left_tokens = emit_expr(left, ctx, emit_env, body_ctx)?;
    let mut right_tokens = emit_expr(right, ctx, emit_env, body_ctx)?;
    if let Some(t) = target {
        left_tokens = numeric_coerce(left, left_tokens, t, ctx, emit_env, body_ctx);
        right_tokens = numeric_coerce(right, right_tokens, t, ctx, emit_env, body_ctx);
    }
    Ok(match op {
        BinaryOp::Add => quote!(#left_tokens + #right_tokens),
        BinaryOp::Sub => quote!(#left_tokens - #right_tokens),
        BinaryOp::Mul => quote!(#left_tokens * #right_tokens),
        BinaryOp::Div => quote!(#left_tokens / #right_tokens),
        BinaryOp::Mod => quote!(#left_tokens % #right_tokens),
        BinaryOp::Eq | BinaryOp::StrictEq => quote!(#left_tokens == #right_tokens),
        BinaryOp::Ne | BinaryOp::StrictNe => quote!(#left_tokens != #right_tokens),
        BinaryOp::Lt => quote!(#left_tokens < #right_tokens),
        BinaryOp::Le => quote!(#left_tokens <= #right_tokens),
        BinaryOp::Gt => quote!(#left_tokens > #right_tokens),
        BinaryOp::Ge => quote!(#left_tokens >= #right_tokens),
        BinaryOp::And => quote!(#left_tokens && #right_tokens),
        BinaryOp::Or => quote!(#left_tokens || #right_tokens),
        BinaryOp::BitAnd => quote!(#left_tokens & #right_tokens),
        BinaryOp::BitOr => quote!(#left_tokens | #right_tokens),
        BinaryOp::BitXor => quote!(#left_tokens ^ #right_tokens),
        BinaryOp::Shl => quote!(#left_tokens << #right_tokens),
        BinaryOp::Shr => quote!(#left_tokens >> #right_tokens),
    })
}

fn common_numeric_target(
    left: Option<TypeId>,
    right: Option<TypeId>,
    emit_env: &EmitEnv,
) -> Option<TypeId> {
    let (left_id, left_ty) = (left?, emit_env.types.resolve(left?)?);
    let (right_id, right_ty) = (right?, emit_env.types.resolve(right?)?);
    let left_num = is_float_type(left_ty) || is_integer_type(left_ty);
    let right_num = is_float_type(right_ty) || is_integer_type(right_ty);
    if !left_num || !right_num {
        return None;
    }
    if is_float_type(left_ty) && is_integer_type(right_ty) {
        return Some(left_id);
    }
    if is_integer_type(left_ty) && is_float_type(right_ty) {
        return Some(right_id);
    }
    None
}

/// The type of the Rust expression a binary arithmetic operator emits: the
/// numeric unification of its two operands. A float operand makes the result a
/// float (the wider of the two when both are floats), two integers keep an
/// integer (the left operand's type). `None` when the operands are not both
/// numeric, so the caller keeps the expression's placeholder type.
///
/// This is the *result* type, unlike [`common_numeric_target`], which reports the
/// type two operands have to be coerced to and returns `None` when they already
/// agree. `value_ty` needs the result type to tell the enclosing assignment or
/// return which coercion to apply to a composite expression.
fn numeric_binary_result_ty(
    left: Option<TypeId>,
    right: Option<TypeId>,
    emit_env: &EmitEnv,
) -> Option<TypeId> {
    let (left_id, left_ty) = (left?, emit_env.types.resolve(left?)?);
    let (right_id, right_ty) = (right?, emit_env.types.resolve(right?)?);
    if !is_numeric_type(left_ty) || !is_numeric_type(right_ty) {
        return None;
    }
    if is_float_type(left_ty) {
        return Some(left_id);
    }
    if is_float_type(right_ty) {
        return Some(right_id);
    }
    Some(left_id)
}

fn is_dynamic_expr(expr: &MirExpr, body_ctx: &BodyCtx) -> bool {
    match expr {
        MirExpr::Local(id) => body_ctx.is_dynamic(*id),
        MirExpr::Global(name) => body_ctx.is_dynamic_name(name),
        _ => false,
    }
}

fn dynamic_string_coerce(
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
    if is_nullish_literal(expr) {
        return quote!(::core::option::Option::None);
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

fn optional_coerce(
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
    // A parameter that has a default *and* a nullable declared type is emitted
    // with a nested ABI type `Option<Option<T>>`: the outer `Option` records
    // whether an argument was supplied at all (the callee's prologue applies the
    // default when it is `None`), the inner one is the value's own nullishness.
    // Coercing into that shape is "coerce one level down, then wrap once more":
    // a plain value becomes `Some(Some(v))`, a nullable source `Some(v)` and an
    // explicit `null` stays `Some(None)` instead of collapsing into the outer
    // `None`, which would silently select the default.
    if matches!(emit_env.types.resolve(inner), Some(Type::Optional { .. })) {
        // A missing argument reaches us as `MirExpr::Unit` (the passes pad
        // trailing slots with it, and an explicit `undefined` lowers to the same
        // node): the outer `None` is what asks the callee for its default.
        if matches!(expr, MirExpr::Unit) {
            return Ok(quote!(::core::option::Option::None));
        }
        // `null` is *not* a missing argument in JavaScript.
        if matches!(expr, MirExpr::Null { .. }) {
            return Ok(quote!(::core::option::Option::Some(
                ::core::option::Option::None
            )));
        }
        // The same distinction for a *call* declared `null`/`void`: it supplies a
        // nullish argument, and `undefined` (`void`) asks for the default.
        match source_nullish_kind(expr, ctx, emit_env, body_ctx) {
            Some(SourceNullish::Null) => {
                return Ok(quote!(::core::option::Option::Some(
                    ::core::option::Option::None
                )));
            }
            Some(SourceNullish::Undefined) => return Ok(quote!(::core::option::Option::None)),
            None => {}
        }
        // A source that already has the nested shape needs no extra wrapping.
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
    if matches!(emit_env.types.resolve(inner), Some(Type::String)) {
        return Ok(optional_string_coerce(
            expr, tokens, target, ctx, emit_env, body_ctx, owned,
        ));
    }
    // A nullish *type* (`void`/`null`) is as nullish as the literal syntax: a call
    // declared `void` yields `undefined`, so it has to become `None` and not
    // `Some(())`, which is not an `Option<T>` at all.
    if is_nullish_literal(expr) || source_nullish_kind(expr, ctx, emit_env, body_ctx).is_some() {
        return Ok(quote!(::core::option::Option::None));
    }
    let source_is_optional = matches!(
        value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)),
        Some(Type::Optional { .. })
    );
    if source_is_optional {
        // The source is already an `Option`, so only its present values need the
        // numeric coercion: `None` stays `None` and `Some(v)` becomes
        // `Some(v as T)`. Returning the source untouched would leave `Option<i64>`
        // where `Option<f64>` is required (E0308), while wrapping the whole source
        // in another `Some` would produce a nested `Option<Option<T>>`.
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
    let inner = numeric_coerce(expr, tokens, inner, ctx, emit_env, body_ctx);
    Ok(quote!(::core::option::Option::Some(#inner)))
}

fn is_float_type(ty: &Type) -> bool {
    matches!(ty, Type::F32 | Type::F64)
}

fn is_integer_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::U8 | Type::U16 | Type::U32 | Type::U64
    )
}

fn is_numeric_type(ty: &Type) -> bool {
    is_float_type(ty) || is_integer_type(ty)
}

/// Whether converting a value of `source` into `target` needs an `as` cast.
/// Both must be numeric; a change of kind (integer vs float) or of width
/// (`i32` vs `i64`) changes the Rust type, so the emitted value has to be cast.
/// Non-numeric and identical types need no conversion, which keeps the existing
/// emission for them.
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

/// A literal initializer of a `static` must have the static's type: Rust reads
/// `0` as an integer literal even when the static is an `f64`.
pub(super) fn coerce_static_init(
    expr: &MirExpr,
    tokens: TokenStream,
    ty: TypeId,
    emit_env: &EmitEnv,
) -> TokenStream {
    // An optional static uses the representation every other optional value
    // uses: `Option<T>`, with a nullish constant as `None` and any other
    // constant wrapped in `Some` (and coerced to the inner type). Without this
    // a `number | null` global emitted `()`/`5` where `Option<f64>` was
    // declared.
    if let Some(Type::Optional { inner, .. }) = emit_env.types.resolve(ty).cloned() {
        if is_nullish_literal(expr) {
            return quote!(::core::option::Option::None);
        }
        let inner_tokens = coerce_static_init(expr, tokens, inner, emit_env);
        return quote!(::core::option::Option::Some(#inner_tokens));
    }
    if !matches!(emit_env.types.resolve(ty), Some(t) if is_float_type(t)) {
        return tokens;
    }
    match expr {
        MirExpr::Int { value, .. } => emit_float_from_int(*value),
        MirExpr::Unary {
            op: UnaryOp::Neg,
            expr,
            ..
        } => match expr.as_ref() {
            MirExpr::Int { value, .. } => {
                let inner = emit_float_from_int(*value);
                quote!(-(#inner))
            }
            _ => tokens,
        },
        _ => tokens,
    }
}
fn numeric_coerce(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    numeric_coerce_impl(expr, tokens, target, ctx, emit_env, body_ctx, false)
}

fn numeric_coerce_exact(
    expr: &MirExpr,
    tokens: TokenStream,
    target: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    numeric_coerce_impl(expr, tokens, target, ctx, emit_env, body_ctx, true)
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
        // A negated *literal* keeps its special emission: `-1` is emitted as
        // `-(1)`, and against a float target the literal itself has to be a
        // float (`-(1.0)`); a negated float literal against an integer target
        // needs the same `as` cast as the plain float literal above.
        //
        // A negation of a typed value must not stop here: its Rust type follows
        // the operand (`i64` for an `i64` variable), so it takes the ordinary
        // source-type-driven cast below. Without that, `return -y` from a
        // `number` function emitted `-(y)` and assigned an `i64` where the
        // `f64` return type is required (E0308).
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

/// The ordinary numeric coercion: cast `tokens` to `target` when the source type
/// of `value` differs from the target in kind (integer vs float) or - for
/// [`numeric_coerce_exact`] - in width. `dst` is `emit_env.types.resolve(target)`.
///
/// The source type is the *value's* own type and not the enclosing expression's:
/// a `MirExpr::Unary` or `MirExpr::Binary` carries the frontend's placeholder
/// type, while the Rust expression's type comes from the operands.
#[allow(clippy::too_many_arguments)]
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
    let Some(src) = value_ty(value, ctx, emit_env, body_ctx) else {
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

fn is_caught_comparable(emit_env: &EmitEnv, ty: TypeId) -> bool {
    emit_env
        .types
        .resolve(ty)
        .is_some_and(super::types::is_comparable_primitive)
}

fn caught_operand_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TokenStream> {
    if matches!(expr, MirExpr::Bool(_)) {
        return Some(quote!(bool));
    }
    let ty = expr_base_ty(expr, body_ctx)?;
    if !is_caught_comparable(emit_env, ty) {
        return None;
    }
    Some(emit_type_id_with_ctx(ty, emit_env, ctx))
}

fn emit_caught_literal_eq(
    caught: &MirExpr,
    other: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let variant = match other {
        MirExpr::Null { .. } => quote!(ts_aot_runtime::CaughtValue::Null),
        MirExpr::Unit => quote!(ts_aot_runtime::CaughtValue::Undefined),
        _ => return Ok(None),
    };
    let caught = emit_expr(caught, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!(#caught == #variant)))
}

/// The nullish kind a value's *static type* carries, for a value that is not a
/// literal node: a call declared `void` (or `null`) yields `undefined` (`null`),
/// which an optional destination stores as `None` - wrapping it in `Some` emitted
/// `Some(())` for an `Option<f64>`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceNullish {
    Null,
    Undefined,
}

fn source_nullish_kind(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<SourceNullish> {
    match value_ty(expr, ctx, emit_env, body_ctx).and_then(|ty| emit_env.types.resolve(ty)) {
        Some(Type::Null) => Some(SourceNullish::Null),
        Some(Type::Void) => Some(SourceNullish::Undefined),
        _ => None,
    }
}
pub(super) fn is_nullish_literal(expr: &MirExpr) -> bool {
    matches!(expr, MirExpr::Null { .. } | MirExpr::Unit)
}

fn is_pure_operand(expr: &MirExpr) -> bool {
    match expr {
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_) => true,
        MirExpr::Field { base, .. } => is_pure_operand(base),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NullishKind {
    Null,
    Undefined,
}

fn nullish_kind(expr: &MirExpr) -> Option<NullishKind> {
    match expr {
        MirExpr::Null { .. } => Some(NullishKind::Null),
        MirExpr::Unit => Some(NullishKind::Undefined),
        _ => None,
    }
}

fn emit_nullish_literal_equality(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let strict = matches!(op, BinaryOp::StrictEq | BinaryOp::StrictNe);
    let neg = matches!(op, BinaryOp::Ne | BinaryOp::StrictNe);
    let (left_kind, right_kind) = (nullish_kind(left), nullish_kind(right));
    if let (Some(left_kind), Some(right_kind)) = (left_kind, right_kind) {
        let equal = left_kind == right_kind || !strict;
        return Ok(Some(if equal == neg {
            quote!(false)
        } else {
            quote!(true)
        }));
    }
    let (operand, literal) = match (left_kind, right_kind) {
        (Some(kind), None) => (right, kind),
        (None, Some(kind)) => (left, kind),
        _ => return Ok(None),
    };
    let Some(ty) = value_ty(operand, ctx, emit_env, body_ctx) else {
        return Ok(None);
    };
    let resolved = emit_env.types.resolve(ty);
    if let Some(Type::Optional { nullish, .. }) = resolved {
        // A strict comparison against `null`/`undefined` is decidable only when
        // the operand admits a single nullish kind: `null` and `undefined` share
        // one `Option::None`, so `x === null` and `x === undefined` cannot be
        // told apart at runtime when the operand allows both. That is exactly
        // the case `check_optional_strict_equality_kinds` rejects for two
        // operands, so reject it here too instead of lowering to `is_none()`
        // (which would answer `true` for an `undefined` value as well). Loose
        // comparisons keep the presence-test lowering, which is correct for both
        // kinds.
        if strict && nullish.null && nullish.undefined {
            let op_name = match op {
                BinaryOp::StrictNe => "!==",
                _ => "===",
            };
            return Err(BackendError::UndecidableNullishComparison {
                code: "P0020",
                message: format!(
                    "`{op_name}` compares an optional value that allows both `null` and \
                     `undefined` with a nullish literal; the AOT target stores both as one \
                     `Option::None`, so the runtime value cannot decide the comparison \
                     (`null === undefined` is `false` in JavaScript while `None == None` is \
                     `true`). Narrow the operand to a single nullish kind (`T | null` or \
                     `T | undefined`), or use a presence test (`x == null` / `x != null`)"
                ),
            });
        }
        let includes_literal = !strict
            || match literal {
                NullishKind::Null => nullish.null,
                NullishKind::Undefined => nullish.undefined,
            };
        if includes_literal {
            let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
            return Ok(Some(if neg {
                quote!((#tokens).is_some())
            } else {
                quote!((#tokens).is_none())
            }));
        }
        let folded = if neg { quote!(true) } else { quote!(false) };
        if is_pure_operand(operand) {
            return Ok(Some(folded));
        }
        let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
        return Ok(Some(quote!({
            let _ = &(#tokens);
            #folded
        })));
    }
    if resolved.is_none() || matches!(resolved, Some(Type::Error | Type::GenericParam { .. })) {
        return Ok(None);
    }
    let equal = match (resolved, literal) {
        (Some(Type::Null), NullishKind::Null) | (Some(Type::Void), NullishKind::Undefined) => true,
        (Some(Type::Null), NullishKind::Undefined) | (Some(Type::Void), NullishKind::Null) => {
            !strict
        }
        _ => false,
    };
    let folded = if equal == neg {
        quote!(false)
    } else {
        quote!(true)
    };
    if is_pure_operand(operand) {
        return Ok(Some(folded));
    }
    let tokens = emit_expr(operand, ctx, emit_env, body_ctx)?;
    Ok(Some(quote!({
        let _ = &(#tokens);
        #folded
    })))
}

/// `x === y` where one side is optional (`T | null`) and the other is a plain
/// `T`: JavaScript compares the *values*, and a nullish value never equals a
/// non-nullish one, so the plain side is compared against `Some(..)` of the
/// optional side instead of emitting `Option<T> == T` (which does not compile).
/// Whether a value of this type compiles as the operand of Rust's `==`.
///
/// The mixed optional comparison below turns `x == y` into
/// `x.as_ref() == Some(&(y))`, so both sides must have `PartialEq`. Scalars do:
/// their emitted Rust type is a primitive, `i64` (symbols and dates), `()` (the
/// nullish types) or `BigIntHandle` (which implements value equality).
/// Structures, arrays and the runtime handles do not, so they must not enter
/// this path.
fn is_rust_comparable_value(ty: &Type) -> bool {
    super::types::is_comparable_primitive(ty)
        || matches!(
            ty,
            Type::Symbol | Type::Date | Type::BigInt | Type::Null | Type::Void
        )
}

fn emit_mixed_optional_equality(
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
        // A literal is coerced to the inner type when the comparison is emitted
        // (`numeric_coerce` below), so the literal's own MIR type does not decide
        // this: an integer literal compares against an `f64` inner, while an
        // `i64` *variable* is coerced the same way.
        MirExpr::Bool(_) => matches!(inner_resolved, Some(Type::Bool)),
        MirExpr::Int { .. } | MirExpr::Float { .. } => matches!(
            inner_resolved,
            Some(ty) if is_float_type(ty) || is_integer_type(ty)
        ),
        MirExpr::String { .. } => matches!(inner_resolved, Some(Type::String)),
        _ => match (plain_resolved, inner_resolved) {
            // A struct/array pair used to pass the "same type" check and
            // emitted `a.as_ref() == Some(&(b.clone()))`, which does not compile
            // because those types have no `PartialEq`.
            (Some(plain), Some(inner)) => {
                (inner == plain && is_rust_comparable_value(plain))
                    || (is_numeric_type(plain) && is_numeric_type(inner))
            }
            _ => false,
        },
    };
    if !plain_matches_inner {
        // Different inner types: either a genuine type error or a pair the
        // generic path handles; leave it untouched.
        return Ok(None);
    }
    // JavaScript compares numbers by value, so both sides have to be compared
    // in their *common* numeric type. Coercing the plain side to the optional's
    // inner type compared `1` against `(1.5) as i64` for
    // `i64EqFloat(Some(1))` and answered `true`, while `1 === 1.5` is `false`.
    // If either side is a float, compare in that float type and widen the
    // optional's integer inner value to it; two integers keep the integer
    // comparison, and non-numeric pairs keep the inner type.
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
    // Keep the source evaluation order of the two operands.
    let cmp = if optional_is_left {
        quote!(#optional.as_ref() == ::core::option::Option::Some(&(#plain)))
    } else {
        quote!(::core::option::Option::Some(&(#plain)) == #optional.as_ref())
    };
    Ok(Some(if neg { quote!(!(#cmp)) } else { cmp }))
}

/// The nullish kinds an optional operand can hold, or `None` when the operand is
/// not a statically known optional value.
fn optional_operand_kinds(
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

/// `null`, `undefined` or `null | undefined` for a diagnostic message.
fn describe_nullish_kinds(kinds: NullishKinds) -> &'static str {
    match (kinds.null, kinds.undefined) {
        (true, true) => "`null | undefined`",
        (true, false) => "`null`",
        (false, true) => "`undefined`",
        (false, false) => "a non-nullish value",
    }
}

/// Reject a strict comparison between two optional operands whose nullish kinds
/// the `Option<T>` representation cannot tell apart.
///
/// `null` and `undefined` both lower to `Option::None`, so `a === b` emitted as
/// `a == b` answers `true` where JavaScript answers `null === undefined` as
/// `false`. That is correct while both operands admit the *same* single nullish
/// kind: each `None` then means the same value on both sides, and the two sides
/// differ only in the cases where Rust's `Option` equality agrees. It is wrong
/// when they admit *different* kinds (`T | null` vs `T | undefined`), and
/// undecidable when an operand admits both (the runtime value could be either),
/// so both need a diagnostic instead of a silent comparison.
///
/// Loose `==`/`!=` keeps the plain lowering: `null == undefined` is `true` in
/// JavaScript, which is exactly what `None == None` answers. Mixed
/// optional/plain comparisons and the `null`/`undefined` literal cases are
/// lowered before this check and never reach it.
fn check_optional_strict_equality_kinds(
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
        // No operand can be nullish, so there is no `None`/`None` case to get
        // wrong: `Option` equality is exact.
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

/// The inner type of a statically known optional operand, or `None` when the
/// operand is not optional.
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

/// Reject a comparison between two optional operands whose inner types the
/// ordinary `Option` comparison cannot handle.
///
/// `a === b` and `a == b` both lower to Rust's `Option<T> == Option<T>`, which
/// needs both sides to be the *same* Rust type and that type to implement
/// `PartialEq`. The metadata leaves two such comparisons undecidable:
///
/// * different numeric inner types (`number | null` vs `i64 | null`) emit
///   `Option<f64> == Option<i64>`, which does not compile at all (E0308). The
///   review prefers a diagnostic over a numeric unification here, because a
///   wrong comparison value would be worse than a compile-time rejection;
/// * an identical but non-comparable inner type (a structure or an array) has no
///   `PartialEq`, so the comparison is E0369. JavaScript compares such operands
///   by identity, which the emitted value equality would not express either.
///
/// The combinations that stay keep the plain `a == b`: two optional operands with
/// the same inner primitive/string/`Date`/`Symbol`/`BigInt`/nullish type. The
/// mixed optional/plain and literal cases are lowered before this check, so only
/// two optional operands reach it. Both strict and loose equality reach it: the
/// lowering is the same `Option` comparison, so a pair the Rust types cannot
/// express is undecidable for either. The operator-specific message spells the
/// operator the source used. An operand whose inner type the metadata cannot
/// resolve keeps the existing lowering.
fn check_optional_strict_equality_inner_types(
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
        // The metadata does not resolve the inner type, so there is nothing to
        // decide from: leave the comparison to the generic lowering.
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

fn emit_dynamic_equality(
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

pub(super) fn emit_float(value: f64) -> TokenStream {
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

fn emit_typeof(
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
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(ts_aot_runtime::JsString::from(__ts_aot_typeof(&#inner))))
        }
    }
}

fn emit_unary_expr(
    op: UnaryOp,
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let expr = emit_expr(expr, ctx, emit_env, body_ctx)?;
    Ok(match op {
        UnaryOp::Neg => quote!(-(#expr)),
        UnaryOp::Not | UnaryOp::BitNot => quote!(!(#expr)),
    })
}

fn emit_cast_expr(
    expr: &MirExpr,
    ty: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
    if !is_numeric_primitive_target(types.resolve(ty)) {
        return Err(BackendError::Internal(format!(
            "MirExpr::Cast target TypeId({}) is not a numeric primitive; \
             Rust's `as` cast cannot lower it. \
             Only I8/I16/I32/I64/U8/U16/U32/U64/F32/F64 are supported as cast targets.",
            ty.raw()
        )));
    }
    let cast_ty = emit_type_id_with_ctx(ty, emit_env, ctx);
    Ok(quote!(#inner as #cast_ty))
}

fn is_numeric_primitive_target(ty: Option<&Type>) -> bool {
    matches!(
        ty,
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
                | Type::F64
        )
    )
}

fn emit_default_for_weakmap_get(ty: TypeId, emit_env: &EmitEnv, ctx: &EmitCtx) -> TokenStream {
    let EmitEnv { types, .. } = emit_env;
    match types.resolve(ty) {
        Some(Type::Bool) => quote!(false),
        Some(
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64,
        ) => quote!(0),
        Some(Type::F32 | Type::F64) => quote!(0.0),
        Some(Type::String) => {
            let ty_tokens = emit_type_id_with_ctx(ty, emit_env, ctx);
            quote!(<#ty_tokens as ::std::default::Default>::default())
        }
        Some(Type::Void | Type::Null | Type::Optional { .. }) => {
            quote!(Default::default())
        }
        _ => quote!(Default::default()),
    }
}

fn emit_weakmap_key(
    key_expr: &MirExpr,
    body_ctx: &BodyCtx,
) -> Result<(TokenStream, Ident), BackendError> {
    let key_root = root_local_id_of(key_expr).ok_or(BackendError::NotImplemented(
        "weakmap key: not a local root",
    ))?;
    let liveness = body_ctx.weakmap_liveness_for_key(key_root);
    let key_ptr = quote!(std::rc::Rc::as_ptr(&#liveness).cast::<()>());
    Ok((key_ptr, liveness))
}

fn emit_weakmap_liveness_binding(liveness_ident: &Ident) -> TokenStream {
    quote! {
        let mut #liveness_ident: std::rc::Rc<()> = std::rc::Rc::new(());
    }
}

fn emit_weakmap_liveness_reset(target: &MirPlace, body_ctx: &BodyCtx) -> TokenStream {
    let MirPlace::Local { id } = target else {
        return quote!();
    };
    if !body_ctx.has_weakmap_liveness_for_key(*id) {
        return quote!();
    }
    let liveness = body_ctx.weakmap_liveness_for_key(*id);
    quote! {
        #liveness = std::rc::Rc::new(());
    }
}

fn weakmap_value_type_id(
    handle_expr: &MirExpr,
    body_ctx: &BodyCtx,
    types: &TypeTable,
) -> Result<TypeId, BackendError> {
    let handle_ty = expr_base_ty(handle_expr, body_ctx).ok_or(BackendError::NotImplemented(
        "weakmap: handle type unresolved",
    ))?;
    match types.resolve(handle_ty) {
        Some(Type::WeakMap { value, .. }) => Ok(*value),
        _ => Err(BackendError::NotImplemented(
            "weakmap: handle is not a WeakMap",
        )),
    }
}

fn root_local_id_of(expr: &MirExpr) -> Option<LocalId> {
    match expr {
        MirExpr::Local(id) => Some(*id),
        MirExpr::Field { base, .. } | MirExpr::Index { base, .. } => root_local_id_of(base),
        _ => None,
    }
}

fn emit_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    ctx.mark_runtime_used();
    match op {
        RuntimeOp::TypedArrayNew => {
            let length = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let kind_id = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_typed_array_new(
                (#length) as i64,
                (#kind_id) as i64
            )))
        }
        RuntimeOp::ArrayGetOrDefault => {
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let idx = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let element_ty = emit_type_id_with_ctx(ty, emit_env, ctx);
            Ok(quote!(__ts_aot_array_get_or_default::<#element_ty>(
                &#arr,
                #idx as i64
            )))
        }
        RuntimeOp::ArrayIsArray => {
            let value = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_is_array(&#value)))
        }
        RuntimeOp::ArrayIsArrayFalse => Ok(quote!(__ts_aot_array_is_array_false())),
        RuntimeOp::OpIn => {
            let value = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let object = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_op_in(&#value, &#object)))
        }
        RuntimeOp::ArrayFrom => {
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_from(__ctx, &#arr)))
        }
        RuntimeOp::ArrayFromMapped => {
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let mapfn = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let fn_value_kind = match &args[1] {
                MirExpr::Global(name) => ctx.fn_value_for_name(name).map(|(k, _)| k),
                _ => None,
            };
            let arity = match &args[1] {
                MirExpr::Global(name) => ctx.function_arity(name).unwrap_or(1),
                _ => 1,
            };
            let call = match fn_value_kind {
                Some(k) => {
                    let helper = EmitCtx::fn_call_ident(k);
                    if arity >= 2 {
                        quote!(#helper(__ctx, &#mapfn, __ts_aot_item, __ts_aot_idx))
                    } else {
                        quote!(#helper(__ctx, &#mapfn, __ts_aot_item))
                    }
                }
                None => {
                    if arity >= 2 {
                        quote!(#mapfn(__ctx, __ts_aot_item, __ts_aot_idx))
                    } else {
                        quote!(#mapfn(__ctx, __ts_aot_item))
                    }
                }
            };
            let wrapper = if arity >= 2 {
                quote!(|__ts_aot_item, __ts_aot_idx| #call)
            } else {
                quote!(|__ts_aot_item, _| #call)
            };
            Ok(quote!(__ts_aot_array_from_mapped(
                __ctx,
                &#arr,
                #wrapper
            )))
        }
        RuntimeOp::ObjectKeys => {
            if let Some(sid) =
                value_ty(&args[0], ctx, emit_env, body_ctx).and_then(|t| types.struct_id(t))
                && let Some(fields) = ctx.struct_field_names(sid)
            {
                let lits = fields.iter().map(|name| {
                    let lit = Literal::string(name.as_str());
                    quote!(ts_aot_runtime::JsString::from(#lit))
                });
                let eval = if matches!(&args[0], MirExpr::Global(_)) {
                    TokenStream::new()
                } else {
                    let operand = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
                    quote!(let _ = &(#operand);)
                };
                return Ok(quote!({
                    #eval
                    __ts_aot_array_create::<ts_aot_runtime::JsString>(
                        __ctx,
                        ts_aot_runtime::bumpalo::vec![in __ctx.arena; #(#lits),*],
                    )
                }));
            }
            let map = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_object_keys(__ctx, &#map)))
        }
        RuntimeOp::ArrayOwnKeys => {
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let include_length = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_own_keys(__ctx, &#arr, #include_length)))
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
            let comparison = emit_same_value(
                types,
                &a_ty,
                &b_ty,
                (quote!(__ts_aot_a), quote!(__ts_aot_b)),
                same_place(&args[0], &args[1]),
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
        RuntimeOp::JsonParseString => {
            let s = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_json_parse_string(__ctx, #s)))
        }
        RuntimeOp::JsonStringifyString => {
            let s = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_json_stringify_string(__ctx, #s)))
        }
        RuntimeOp::ArrayBufferSlice => {
            let buf = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let begin = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let end = emit_expr(&args[2], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_buffer_slice(&#buf, #begin, #end)))
        }
        RuntimeOp::ArrayCreate => emit_array_op_with_element_type(
            "__ts_aot_array_create",
            args,
            ty,
            true,
            ctx,
            emit_env,
            body_ctx,
        ),
        RuntimeOp::ArrayConcat => emit_array_op_with_element_type(
            "__ts_aot_array_concat",
            args,
            ty,
            false,
            ctx,
            emit_env,
            body_ctx,
        ),
        RuntimeOp::ArrayHole => {
            let element_ty_id = match types.resolve(ty) {
                Some(Type::Array { element }) => *element,
                _ => ty,
            };
            let element_ty = emit_type_id_with_ctx(element_ty_id, emit_env, ctx);
            Ok(quote!(__ts_aot_array_hole::<#element_ty>(__ctx)))
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
        RuntimeOp::ArrayPush => {
            let (arr, needs_unsafe) = emit_mut_receiver(&args[0], ctx, emit_env, body_ctx)?;
            let item = emit_owned_value(&args[1], ctx, emit_env, body_ctx)?;
            let item = match array_element_ty(&args[0], ctx, emit_env, body_ctx) {
                Some(element) => {
                    let item = numeric_coerce(&args[1], item, element, ctx, emit_env, body_ctx);
                    optional_coerce(&args[1], item, element, ctx, emit_env, body_ctx, false)?
                }
                None => item,
            };
            let call = quote!(__ts_aot_array_push(&mut #arr, #item));
            Ok(if needs_unsafe {
                quote!(unsafe { #call })
            } else {
                call
            })
        }
        RuntimeOp::ArraySet => {
            let (arr, needs_unsafe) = emit_mut_receiver(&args[0], ctx, emit_env, body_ctx)?;
            let idx = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let value = emit_owned_value(&args[2], ctx, emit_env, body_ctx)?;
            let value = match array_element_ty(&args[0], ctx, emit_env, body_ctx) {
                Some(element) => {
                    let value = numeric_coerce(&args[2], value, element, ctx, emit_env, body_ctx);
                    optional_coerce(&args[2], value, element, ctx, emit_env, body_ctx, false)?
                }
                None => value,
            };
            let call = quote!(__ts_aot_array_set(&mut #arr, #idx as i64, #value));
            Ok(if needs_unsafe {
                quote!(unsafe { #call })
            } else {
                call
            })
        }
        RuntimeOp::ArrayLen => {
            let name = runtime_op_ident(op)?;
            if let MirExpr::OptionalChain { base, ty: chain_ty } = &args[0]
                && matches!(types.resolve(*chain_ty), Some(Type::Optional { .. }))
            {
                let base = emit_expr(base, ctx, emit_env, body_ctx)?;
                return Ok(quote!(#base.as_ref().map(|o| #name(o))));
            }
            let arr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&#arr)))
        }
        RuntimeOp::MathMax | RuntimeOp::MathMin => {
            let name = runtime_op_ident(op)?;
            let args = emit_exprs(args, ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&[#(#args),*])))
        }
        RuntimeOp::StringConcat
        | RuntimeOp::StringEquals
        | RuntimeOp::StringLen
        | RuntimeOp::StringIndexOf
        | RuntimeOp::StringCharAt => {
            let string_arg_indices = string_op_string_arg_indices(op);
            let name = runtime_op_ident(op)?;
            let ctx_arg = if runtime_op_takes_ctx(op) {
                quote!(__ctx,)
            } else {
                quote!()
            };
            let emitted: Vec<TokenStream> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if string_arg_indices.contains(&i) {
                        emit_js_string_arg(a, ctx, emit_env, body_ctx)
                    } else {
                        emit_expr(a, ctx, emit_env, body_ctx)
                    }
                })
                .collect::<Result<Vec<_>, BackendError>>()?;
            Ok(quote!(#name(#ctx_arg #(#emitted),*)))
        }
        RuntimeOp::MapSet => {
            let name = runtime_op_ident(op)?;
            let map_expr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let key_expr = emit_js_string_owned(&args[1], ctx, emit_env, body_ctx)?;
            let value_expr = emit_js_string_owned(&args[2], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&mut #map_expr, #key_expr, #value_expr)))
        }
        RuntimeOp::MapGet => {
            let name = runtime_op_ident(op)?;
            let map_expr = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let key_expr = emit_js_string_arg(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(&#map_expr, #key_expr)))
        }
        RuntimeOp::ArrayFromString => {
            let name = runtime_op_ident(op)?;
            let source_expr = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(__ctx, #source_expr)))
        }
        RuntimeOp::HostConsoleLog | RuntimeOp::SymbolFor => {
            let name = runtime_op_ident(op)?;
            let arg = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(#arg)))
        }
        RuntimeOp::JsonParse | RuntimeOp::JsonStringify => {
            let name = runtime_op_ident(op)?;
            let target = target_ty.unwrap_or(ty);
            let ty_tokens = emit_type_id_with_ctx(target, emit_env, ctx);
            if op == RuntimeOp::JsonParse {
                let arg = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
                if let Some(Type::Array { element }) = types.resolve(target) {
                    if matches!(types.resolve(*element), Some(Type::String)) {
                        return Ok(quote!(ts_aot_runtime::__ts_aot_json_parse_vec_string(
                            __ctx, #arg
                        )));
                    }
                    let element_tokens = emit_type_id_with_ctx(*element, emit_env, ctx);
                    Ok(
                        quote!(ts_aot_runtime::bumpalo::collections::Vec::from_iter_in(
                            __ts_aot_json_parse::<Vec<#element_tokens>>(#arg),
                            __ctx.arena,
                        )),
                    )
                } else {
                    Ok(quote!(#name::<#ty_tokens>(#arg)))
                }
            } else {
                let arg = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
                Ok(quote!(#name::<#ty_tokens>(__ctx, &#arg)))
            }
        }
        RuntimeOp::SymbolNew => {
            let name = runtime_op_ident(op)?;
            if args.is_empty() || matches!(&args[0], MirExpr::Unit) {
                Ok(quote!(#name()))
            } else {
                let desc_expr: TokenStream = match &args[0] {
                    MirExpr::Null { .. } => quote!(ts_aot_runtime::JsString::from("null")),
                    _ => emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?,
                };
                Ok(quote!(__ts_aot_symbol_new_desc(&#desc_expr)))
            }
        }
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
                let reason = match &args[0] {
                    MirExpr::String { id, .. } => {
                        let lit = Literal::string(id.as_str());
                        quote!(ts_aot_runtime::JsString::from(#lit).to_string_lossy())
                    }
                    _ if matches!(
                        expr_base_ty(&args[0], body_ctx).and_then(|ty| types.resolve(ty)),
                        Some(Type::String)
                    ) =>
                    {
                        let js = emit_js_string_owned(&args[0], ctx, emit_env, body_ctx)?;
                        quote!(#js.to_string_lossy())
                    }
                    _ => {
                        let reason_ty =
                            expr_base_ty(&args[0], body_ctx).and_then(|ty| types.resolve(ty));
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
        RuntimeOp::WeakMapGet => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness) = emit_weakmap_key(&args[1], body_ctx)?;
            let default_tokens = emit_default_for_weakmap_get(ty, emit_env, ctx);
            let value_ty_tokens = emit_type_id_with_ctx(ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_get::<#value_ty_tokens>(
                &#handle,
                &#liveness,
                #key,
            ).unwrap_or(#default_tokens)))
        }
        RuntimeOp::WeakMapSet => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness) = emit_weakmap_key(&args[1], body_ctx)?;
            let value = emit_owned_value(&args[2], ctx, emit_env, body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            Ok(quote!({
                let _ = __ts_aot_weak_map_set::<#value_ty_tokens>(
                    &#handle,
                    &#liveness,
                    #key,
                    #value,
                );
                #handle.clone()
            }))
        }
        RuntimeOp::WeakMapHas => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness) = emit_weakmap_key(&args[1], body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_has::<#value_ty_tokens>(
                &#handle, &#liveness, #key,
            ) != 0))
        }
        RuntimeOp::WeakMapDelete => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness) = emit_weakmap_key(&args[1], body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_delete::<#value_ty_tokens>(
                &#handle, &#liveness, #key,
            ) != 0))
        }
        RuntimeOp::WeakMapClear => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_weak_map_clear(&#handle)))
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
        _ => {
            let name = runtime_op_ident(op)?;
            let ctx_arg = if runtime_op_takes_ctx(op) {
                quote!(__ctx,)
            } else {
                quote!()
            };
            let args = emit_exprs(args, ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(#ctx_arg #(#args),*)))
        }
    }
}

fn runtime_op_takes_ctx(op: RuntimeOp) -> bool {
    matches!(
        op,
        RuntimeOp::StringConcat
            | RuntimeOp::StringCharAt
            | RuntimeOp::StringFromCharCode
            | RuntimeOp::StringFromCodePoint
            | RuntimeOp::JsonParseString
            | RuntimeOp::JsonStringifyString
            | RuntimeOp::JsonStringify
            | RuntimeOp::ArrayFromString
            | RuntimeOp::ArrayCreateWithLen
            | RuntimeOp::ArrayFromLengthMapped
            | RuntimeOp::ObjectKeys
            | RuntimeOp::DateToIsoString
            | RuntimeOp::SymbolKeyFor
    )
}

fn string_op_string_arg_indices(op: RuntimeOp) -> &'static [usize] {
    match op {
        RuntimeOp::StringLen | RuntimeOp::StringCharAt => &[0],
        RuntimeOp::StringConcat | RuntimeOp::StringEquals | RuntimeOp::StringIndexOf => &[0, 1],
        _ => &[],
    }
}

fn emit_array_op_with_element_type(
    op_name: &'static str,
    args: &[MirExpr],
    ty: TypeId,
    coerce_optionals: bool,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let element_ty_id = match types.resolve(ty) {
        Some(Type::Array { element }) => *element,
        _ => ty,
    };
    let element_ty = emit_type_id_with_ctx(element_ty_id, emit_env, ctx);
    let parts: Vec<TokenStream> = args
        .iter()
        .map(|a| {
            let tokens = emit_owned_value(a, ctx, emit_env, body_ctx)?;
            let tokens = numeric_coerce(a, tokens, element_ty_id, ctx, emit_env, body_ctx);
            Ok::<_, BackendError>(if coerce_optionals {
                // Array literal elements are values of the element type; a
                // non-optional one needs `Some(..)` and a `null`/`undefined`
                // element needs `None`.
                optional_coerce(a, tokens, element_ty_id, ctx, emit_env, body_ctx, false)?
            } else {
                tokens
            })
        })
        .collect::<Result<_, _>>()?;
    let name = format_ident!("{op_name}");
    Ok(
        quote!(#name::<#element_ty>(__ctx, ts_aot_runtime::bumpalo::vec![in __ctx.arena; #(#parts),*])),
    )
}

/// Element type of an array-typed expression (`T` for `T[]`, `T` for
/// `(T[] | undefined)`).
fn array_element_ty(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TypeId> {
    let ty = value_ty(expr, ctx, emit_env, body_ctx)?;
    match emit_env.types.resolve(ty) {
        Some(Type::Array { element }) => Some(*element),
        Some(Type::Optional { inner, .. }) => match emit_env.types.resolve(*inner) {
            Some(Type::Array { element }) => Some(*element),
            _ => None,
        },
        _ => None,
    }
}

fn emit_js_string_arg(
    arg: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Borrowed, ctx, emit_env, body_ctx)
}

fn emit_js_string_owned(
    arg: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Owned, ctx, emit_env, body_ctx)
}

#[derive(Clone, Copy)]
enum StringOwnership {
    Borrowed,
    Owned,
}

fn emit_js_string_expr(
    arg: &MirExpr,
    ownership: StringOwnership,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let is_string = matches!(arg, MirExpr::String { .. })
        || value_ty(arg, ctx, emit_env, body_ctx)
            .and_then(|t| emit_env.types.resolve(t))
            .is_some_and(|t| matches!(t, Type::String));
    if !is_string {
        let value = emit_expr(arg, ctx, emit_env, body_ctx)?;
        let coerced = quote!(ts_aot_runtime::JsString::from((#value).to_string().as_str()));
        return Ok(match ownership {
            StringOwnership::Borrowed => quote!(&#coerced),
            StringOwnership::Owned => coerced,
        });
    }
    let core = match arg {
        MirExpr::String { id, .. } => {
            let lit = Literal::string(id.as_str());
            quote!(ts_aot_runtime::JsString::from(#lit))
        }
        MirExpr::Local(id) => body_ctx.local_read(*id),
        _ => emit_expr(arg, ctx, emit_env, body_ctx)?,
    };
    Ok(match ownership {
        StringOwnership::Borrowed => quote!(&#core),
        StringOwnership::Owned => quote!(#core.clone()),
    })
}

fn emit_switch(
    disc: &MirExpr,
    cases: &[ts_aot_ir_mir::SwitchCase],
    default: Option<&MirBlock>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let disc_expr = emit_expr(disc, ctx, emit_env, body_ctx)?;
    let switch_ident = format_ident!("__switch_{}", body_ctx.alloc_block_id());
    let prev_break = body_ctx.break_label();
    body_ctx.set_break_label(Some(switch_ident.clone()));
    body_ctx.enter_switch();
    let mut arms: Vec<TokenStream> = Vec::with_capacity(cases.len() + 1);
    for case in cases {
        let pat = match &case.value {
            ConstValue::Int(v) => {
                let lit = Literal::i128_unsuffixed(*v);
                quote!(#lit)
            }
            ConstValue::String(s) => {
                let lit = Literal::string(s.as_str());
                quote!(#lit)
            }
        };
        let body_stmts = emit_block_stmts(&case.body, ctx, emit_env, body_ctx)?;
        arms.push(quote!(#pat => { #(#body_stmts)* }));
    }
    if let Some(def) = default {
        let body_stmts = emit_block_stmts(def, ctx, emit_env, body_ctx)?;
        arms.push(quote!(_ => { #(#body_stmts)* }));
    } else {
        arms.push(quote!(_ => {}));
    }
    body_ctx.exit_switch();
    body_ctx.set_break_label(prev_break);
    let lt = loop_label(&switch_ident);
    Ok(quote!(#lt: { match #disc_expr { #(#arms),* } }))
}

fn await_helper_path(body_ctx: &BodyCtx) -> TokenStream {
    if body_ctx.in_try() {
        quote!(ts_aot_runtime::__ts_aot_await_value_raw)
    } else if body_ctx.is_async_generator() {
        quote!(ts_aot_runtime::__ts_aot_await_value_gen)
    } else {
        quote!(ts_aot_runtime::__ts_aot_await_value)
    }
}

fn emit_try(
    body: &MirBlock,
    catch_param: Option<LocalId>,
    catch: Option<&MirBlock>,
    finally: Option<&MirBlock>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let try_id = body_ctx.alloc_try_id();
    let label = format_ident!("__try_{}", try_id);
    let slot_ident = format_ident!("__return_slot_{}", try_id);
    let prev_in_try = body_ctx.in_try();
    let prev_try_label = body_ctx.try_label();
    let prev_return_slot = body_ctx.return_slot();
    body_ctx.set_in_try(true);
    body_ctx.set_try_label(Some(label.clone()));
    body_ctx.set_return_slot(Some(slot_ident.clone()));
    body_ctx.push_try_floor();
    let body_stmts = emit_block_stmts(body, ctx, emit_env, body_ctx);
    body_ctx.set_try_label(prev_try_label.clone());
    let body_stmts = body_stmts?;

    let catch_stmts = if let Some(catch_block) = catch {
        let prev = body_ctx.try_label();
        body_ctx.set_try_label(Some(label.clone()));
        let stmts = emit_block_stmts(catch_block, ctx, emit_env, body_ctx);
        body_ctx.set_try_label(prev);
        Some(stmts?)
    } else {
        None
    };

    let this_loop_floor = body_ctx.try_floor_loop().unwrap_or(0);
    let enclosing_loop_floor = body_ctx.enclosing_try_loop_floor();
    body_ctx.pop_try_floor();
    let loop_in_enclosing = this_loop_floor > enclosing_loop_floor;

    body_ctx.set_in_try(prev_in_try);
    body_ctx.set_try_label(prev_try_label);
    body_ctx.set_return_slot(prev_return_slot);
    let finally_stmts = if let Some(fin) = finally {
        Some(emit_block_stmts(fin, ctx, emit_env, body_ctx)?)
    } else {
        None
    };

    let catch_unwind = format_ident!("catch_unwind");
    let assert_unwind_safe = format_ident!("AssertUnwindSafe");
    let resume_unwind = format_ident!("resume_unwind");

    let inner_ret_ty = emit_type_id_with_ctx(body_ctx.return_type_id(), emit_env, ctx);
    let slot_ty = if body_ctx.is_generator() {
        quote!(Option<Option<#inner_ret_ty>>)
    } else {
        quote!(Option<#inner_ret_ty>)
    };
    let slot_decl = quote!(let mut #slot_ident: #slot_ty = None;);
    let is_nested = body_ctx.return_slot().is_some();
    let wrap_escape = body_ctx.is_async_generator() && !is_nested;
    let outer_slot_ident = body_ctx.return_slot();
    let replay_return = if is_nested {
        let outer_slot = outer_slot_ident.expect("nested try must have an outer return slot");
        quote! {
            if let Some(__v) = #slot_ident {
                #outer_slot = Some(__v);
                return __ReturnSignal::Break;
            }
        }
    } else if body_ctx.is_async_generator() {
        quote! {
            if let Some(__v) = #slot_ident {
                return Some(ts_aot_runtime::AsyncGenYield::finished(__v));
            }
        }
    } else {
        quote! {
            if let Some(__v) = #slot_ident {
                return __v;
            }
        }
    };

    let body_arm = if catch.is_some() {
        let catch_stmts = catch_stmts.expect("catch block present");
        if let Some(param) = catch_param {
            let param_ident = body_ctx.local_ident(param);
            let param_ty_id = body_ctx.local_ty(param);
            let is_any_catch =
                param_ty_id.is_some_and(|t| matches!(emit_env.types.resolve(t), Some(Type::Error)));
            // `map_or_default` would be the shortest form, but it is still
            // unstable on the toolchains this workspace builds with (rust-lang#138099).
            let shared_write = match body_ctx.shared_access_for_local(param) {
                Some(acc) => shared_write_tokens(&acc, &quote!(#param_ident)),
                None => TokenStream::default(),
            };
            let bind = if is_any_catch {
                quote!(let #param_ident: ts_aot_runtime::CaughtValue = ts_aot_runtime::__ts_aot_catch_bind(&*__e);)
            } else {
                let param_ty = param_ty_id
                    .map_or_else(|| quote!(()), |t| emit_type_id_with_ctx(t, emit_env, ctx));
                let is_string = param_ty_id
                    .is_some_and(|t| matches!(emit_env.types.resolve(t), Some(Type::String)));
                let extractable = param_ty_id.is_some_and(|t| {
                    emit_env
                        .types
                        .resolve(t)
                        .is_some_and(super::types::is_comparable_primitive)
                });
                if is_string {
                    quote! {
                        let #param_ident: #param_ty =
                            match ts_aot_runtime::__ts_aot_catch_string(__ctx, __e) {
                                Ok(v) => v,
                                Err(__e) => std::panic::#resume_unwind(__e),
                            };
                    }
                } else if extractable {
                    quote! {
                        let #param_ident: #param_ty =
                            match ts_aot_runtime::__ts_aot_catch_typed::<#param_ty>(__e) {
                                Ok(v) => v,
                                Err(__e) => std::panic::#resume_unwind(__e),
                            };
                    }
                } else {
                    quote! {
                        let #param_ident: #param_ty = match __e.downcast::<#param_ty>() {
                            Ok(v) => *v,
                            Err(__e) => std::panic::#resume_unwind(__e),
                        };
                    }
                }
            };
            quote! {
                let __catch_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #bind
                    #shared_write
                    #(#catch_stmts)*
                    __ReturnSignal::Continue
                }));
                match __catch_result {
                    Ok(__ReturnSignal::LoopBreak) => __loop_jump = 1,
                    Ok(__ReturnSignal::LoopContinue) => __loop_jump = 2,
                    Err(__e2) => __pending_throw = Some(__e2),
                    Ok(_) => {}
                }
            }
        } else {
            quote! {
                let __catch_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #(#catch_stmts)*
                    __ReturnSignal::Continue
                }));
                match __catch_result {
                    Ok(__ReturnSignal::LoopBreak) => __loop_jump = 1,
                    Ok(__ReturnSignal::LoopContinue) => __loop_jump = 2,
                    Err(__e2) => __pending_throw = Some(__e2),
                    Ok(_) => {}
                }
            }
        }
    } else if finally_stmts.is_some() {
        quote! {
            __pending_throw = Some(__e);
        }
    } else {
        quote! {}
    };

    let finally_block = if let Some(finally_stmts) = finally_stmts {
        quote! { #(#finally_stmts)* }
    } else {
        quote! {}
    };

    let final_rethrow = if wrap_escape {
        quote!(ts_aot_runtime::__ts_aot_async_generator_rethrow(__e))
    } else {
        quote!(__e)
    };

    let break_replay = if body_ctx.break_escapes_try() {
        quote!(return __ReturnSignal::LoopBreak;)
    } else if let Some(label) = body_ctx.break_label() {
        let lt = loop_label(&label);
        quote!(break #lt;)
    } else if loop_in_enclosing {
        quote!(break;)
    } else {
        quote!(unreachable!("break escapes a try without an enclosing loop");)
    };
    let continue_replay = if loop_in_enclosing {
        if let Some(label) = body_ctx.continue_label() {
            let lt = loop_label(&label);
            quote!(break #lt;)
        } else if let Some(target) = body_ctx.continue_target() {
            let lt = loop_label(&target);
            quote!(continue #lt;)
        } else {
            quote!(continue;)
        }
    } else if body_ctx.in_try() {
        quote!(return __ReturnSignal::LoopContinue;)
    } else {
        quote!(unreachable!("continue escapes a try without an enclosing loop");)
    };

    let label_lt = loop_label(&label);
    Ok(quote! {{
        #slot_decl
        let mut __pending_throw: Option<Box<dyn std::any::Any + Send>> = None;
        let mut __loop_jump: u8 = 0;
        #label_lt: loop {
            let __try_result: Result<__ReturnSignal, Box<dyn std::any::Any + Send>> =
                std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #(#body_stmts)*
                    __ReturnSignal::Continue
                }));
            match __try_result {
                Ok(__ReturnSignal::Break | __ReturnSignal::Continue) => break #label_lt,
                Ok(__ReturnSignal::LoopBreak) => {
                    __loop_jump = 1;
                    break #label_lt;
                }
                Ok(__ReturnSignal::LoopContinue) => {
                    __loop_jump = 2;
                    break #label_lt;
                }
                Err(__e) => {
                    #body_arm
                    break #label_lt;
                }
            }
        }
        #finally_block
        if let Some(__e) = __pending_throw {
            std::panic::#resume_unwind(#final_rethrow);
        }
        if __loop_jump == 1 {
            #break_replay
        }
        if __loop_jump == 2 {
            #continue_replay
        }
        #replay_return
    }})
}

#[cfg(test)]
mod same_value_tests {
    use super::*;

    #[test]
    fn object_is_checks_both_operand_types() {
        use quote::quote;
        let mut types = TypeTable::new();
        let number = types.intern(&Type::I64);
        let optional = Type::Optional {
            inner: number,
            nullish: ts_aot_core::NullishKinds::BOTH,
        };
        let emit = |a: &Type, b: &Type| {
            emit_same_value(&types, a, b, (quote!(a), quote!(b)), false, 0)
                .map(|tokens| tokens.to_string())
        };
        assert!(emit(&Type::I64, &Type::F64).unwrap().contains("as f64"));
        assert!(emit(&optional, &Type::I64).unwrap().contains("Some"));
        assert!(emit(&Type::I64, &optional).unwrap().contains("Some"));
        assert!(
            emit(&optional, &optional)
                .unwrap()
                .contains("(None , None) => true")
        );
        assert!(
            emit(&optional, &Type::Void)
                .unwrap()
                .contains("None => true")
        );
        assert_eq!(emit(&Type::Bool, &Type::I64).unwrap(), "false");
        let array = Type::Array { element: number };
        assert_eq!(emit(&array, &array).unwrap(), "false");
        for unsupported in [Type::Error, Type::BigInt, Type::Tuple { elements: vec![] }] {
            assert!(emit(&Type::I64, &unsupported).is_err());
            assert!(emit(&unsupported, &Type::I64).is_err());
        }
    }
}
