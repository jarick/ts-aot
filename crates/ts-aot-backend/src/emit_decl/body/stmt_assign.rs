use super::super::ctx::{
    BodyCtx, EmitCtx, EmitEnv, SharedAccess, SharedStorage, place_root_global_name,
    shared_write_tokens,
};
use super::super::{DurableGlobal, global_durable_kind};
use super::binary::emit_optional_chain_assign;
use super::coerce::{dynamic_string_coerce, emit_owned_value, numeric_coerce, optional_coerce};
use super::globals::{is_durable_global, is_owned_global};
use super::place::emit_place;
use super::runtime_call::collections::emit_weakmap_liveness_reset;
use crate::error::BackendError;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{MirDecl, MirExpr, MirPlace, MirStmt};

pub(super) fn shared_write_capture(
    value: &MirExpr,
    declared: Option<TypeId>,
    acc: &SharedAccess,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
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

fn struct_identity_field(
    id: ts_aot_core::LocalId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<Ident> {
    let struct_id = emit_env
        .types
        .struct_id(body_ctx.local_ty(id).or_else(|| body_ctx.capture_ty(id))?)?;
    let field = emit_env
        .program
        .declarations
        .iter()
        .find_map(|decl| match decl {
            MirDecl::Struct(s) if s.id == struct_id => s.fields.first(),
            _ => None,
        })?;
    Some(ctx.struct_field_ident(struct_id, field.id))
}

fn struct_field_identity(
    id: ts_aot_core::LocalId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TokenStream> {
    let field_ident = struct_identity_field(id, ctx, emit_env, body_ctx)?;
    let name = body_ctx.local_ident(id);
    Some(quote!(::std::rc::Rc::as_ptr(&#name.#field_ident)))
}

fn shared_struct_field_identity(acc: &SharedAccess, field_ident: &Ident) -> TokenStream {
    let env = &acc.env_ident;
    let field = &acc.field;
    match acc.storage {
        SharedStorage::Cell => quote!(
            #env.#field.get().map(|__ts_key| ::std::rc::Rc::as_ptr(&__ts_key.#field_ident))
        ),
        SharedStorage::RefCell => quote!(
            #env
                .#field
                .borrow()
                .as_ref()
                .map(|__ts_key| ::std::rc::Rc::as_ptr(&__ts_key.#field_ident))
        ),
    }
}

fn shared_key_identity(
    id: ts_aot_core::LocalId,
    acc: &SharedAccess,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<TokenStream> {
    let field_ident = struct_identity_field(id, ctx, emit_env, body_ctx)?;
    Some(shared_struct_field_identity(acc, &field_ident))
}

fn guarded_weakmap_assignment(
    write: &TokenStream,
    reset: &TokenStream,
    identity: &TokenStream,
    body_ctx: &BodyCtx,
) -> TokenStream {
    let previous = body_ctx.alloc_hoist_ident(&format!(
        "__ts_aot_weakmap_key_{}",
        body_ctx.alloc_block_id()
    ));
    quote! {
        let #previous = #identity;
        #write
        if #previous != #identity {
            #reset
        }
    }
}

fn assigned_same_local(target: ts_aot_core::LocalId, value: &MirExpr, body_ctx: &BodyCtx) -> bool {
    match value {
        MirExpr::Local(id) => body_ctx.is_identity_assignment(target, *id),
        MirExpr::Cast { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            assigned_same_local(target, expr, body_ctx)
        }
        _ => false,
    }
}

fn emit_weakmap_key_assignment(
    target: &MirPlace,
    target_tokens: &TokenStream,
    value: &MirExpr,
    value_tokens: &TokenStream,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    let reset = emit_weakmap_liveness_reset(target, body_ctx);
    if reset.is_empty() {
        return quote!(#target_tokens = #value_tokens;);
    }
    let MirPlace::Local { id } = target else {
        return quote!(#target_tokens = #value_tokens; #reset);
    };
    let Some(identity) = struct_field_identity(*id, ctx, emit_env, body_ctx) else {
        if assigned_same_local(*id, value, body_ctx) {
            return quote!(#target_tokens = #value_tokens;);
        }
        return quote!(#target_tokens = #value_tokens; #reset);
    };
    guarded_weakmap_assignment(
        &quote!(#target_tokens = #value_tokens;),
        &reset,
        &identity,
        body_ctx,
    )
}

pub(super) fn emit_assign_stmt(
    stmt: &MirStmt,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirStmt::Assign { target, value } = stmt else {
        unreachable!()
    };
    if let MirPlace::Local { id } = target
        && let Some(acc) = body_ctx.shared_access_for_local(*id)
    {
        let write = shared_write_capture(
            value,
            body_ctx.local_ty(*id).or_else(|| body_ctx.capture_ty(*id)),
            &acc,
            ctx,
            emit_env,
            body_ctx,
        )?;
        let reset = emit_weakmap_liveness_reset(target, body_ctx);
        if reset.is_empty() {
            return Ok(write);
        }
        let Some(identity) = shared_key_identity(*id, &acc, ctx, emit_env, body_ctx) else {
            if assigned_same_local(*id, value, body_ctx) {
                return Ok(write);
            }
            return Ok(quote! {
                #write
                #reset
            });
        };
        return Ok(guarded_weakmap_assignment(
            &write, &reset, &identity, body_ctx,
        ));
    }
    if let MirPlace::Global(name) = target
        && let Some(acc) = body_ctx.shared_access_for_name(name)
    {
        return shared_write_capture(
            value,
            body_ctx
                .shared_ty_for_name(name)
                .or_else(|| ctx.global_ty(name)),
            &acc,
            ctx,
            emit_env,
            body_ctx,
        );
    }
    if let Some(chain_assign) = emit_optional_chain_assign(target, value, ctx, emit_env, body_ctx)?
    {
        return Ok(chain_assign);
    }
    let mut target_prologue: Vec<TokenStream> = Vec::new();
    let target_tokens = emit_place(target, ctx, emit_env, body_ctx, &mut target_prologue)?;
    let value_tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
    let value_tokens = match target {
        MirPlace::Local { id } => match body_ctx.local_ty(*id) {
            Some(target_ty) => {
                let coerced =
                    dynamic_string_coerce(value, value_tokens, target_ty, emit_env, body_ctx);
                let coerced =
                    optional_coerce(value, coerced, target_ty, ctx, emit_env, body_ctx, false)?;
                numeric_coerce(value, coerced, target_ty, ctx, emit_env, body_ctx)
            }
            None => value_tokens,
        },
        MirPlace::Global(name) if is_durable_global(name, ctx, emit_env) => {
            let target_ty = ctx.global_ty(name);
            let coerced = match target_ty {
                Some(target_ty) => {
                    dynamic_string_coerce(value, value_tokens, target_ty, emit_env, body_ctx)
                }
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
            let coerced = if let Some(target_ty) = ctx.global_ty(name) {
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
            };
            if is_owned_global(name, ctx, emit_env) {
                quote!(#coerced.iter().cloned().collect::<::std::vec::Vec<_>>())
            } else if ctx.is_struct_global(name) {
                quote!(::core::option::Option::Some(#coerced))
            } else {
                coerced
            }
        }
        MirPlace::Field { ty, .. } | MirPlace::Index { ty, .. } => {
            let coerced = dynamic_string_coerce(value, value_tokens, *ty, emit_env, body_ctx);
            let coerced = optional_coerce(value, coerced, *ty, ctx, emit_env, body_ctx, false)?;
            numeric_coerce(value, coerced, *ty, ctx, emit_env, body_ctx)
        }
    };
    let assignment = emit_weakmap_key_assignment(
        target,
        &target_tokens,
        value,
        &value_tokens,
        ctx,
        emit_env,
        body_ctx,
    );
    if place_root_global_name(target).is_some_and(|name| ctx.is_mutable_global(name)) {
        let assignment = quote! {
            unsafe { #assignment }
        };
        Ok(if target_prologue.is_empty() {
            assignment
        } else {
            quote!({
                #(#target_prologue)*
                #assignment
            })
        })
    } else if target_prologue.is_empty() {
        Ok(assignment)
    } else {
        Ok(quote!({
            #(#target_prologue)*
            #assignment
        }))
    }
}
