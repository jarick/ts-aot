use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, WeakMapLiveness};
use super::super::super::runtime_op::runtime_op_ident;
use super::super::super::types::emit_type_id_with_ctx;
use super::super::coerce::emit_owned_value;
use super::super::expr::emit_expr;
use super::super::helpers::value_ty;
use super::super::place::{WeakMapKeyRoot, expr_base_ty, weakmap_key_root};
use super::strings::{emit_js_string_arg, emit_js_string_owned};
use crate::error::BackendError;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use ts_aot_core::{Atom, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{MirExpr, MirPlace, RuntimeOp};

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
) -> Result<(TokenStream, TokenStream, TokenStream), BackendError> {
    let key_root = weakmap_key_root(key_expr).ok_or(BackendError::NotImplemented(
        "weakmap key: not a local root",
    ))?;
    let liveness = match key_root {
        WeakMapKeyRoot::Local(id) => body_ctx.weakmap_liveness_for_key(id),
        WeakMapKeyRoot::Name(name) => body_ctx.weakmap_liveness_for_name(&name).ok_or(
            BackendError::NotImplemented(
                "weakmap key: this captured name has no shared liveness binding, so key identity \
                 cannot be tracked",
            ),
        )?,
    };
    let (key_ptr, liveness_ref, liveness_token) = match &liveness {
        WeakMapLiveness::Local(ident) => (
            quote!(std::rc::Rc::as_ptr(&#ident).cast::<()>()),
            quote!(&#ident),
            quote!(#ident.clone()),
        ),
        WeakMapLiveness::Shared { env, field } => (
            quote!(std::rc::Rc::as_ptr(&*#env.#field.borrow()).cast::<()>()),
            quote!(&*#env.#field.borrow()),
            quote!(#env.#field.borrow().clone()),
        ),
    };
    Ok((key_ptr, liveness_ref, liveness_token))
}

pub(crate) fn emit_weakmap_liveness_binding(liveness_ident: &Ident) -> TokenStream {
    quote! {
        let mut #liveness_ident: std::rc::Rc<()> = std::rc::Rc::new(());
    }
}

pub(crate) fn emit_weakmap_liveness_reset(target: &MirPlace, body_ctx: &BodyCtx) -> TokenStream {
    let MirPlace::Local { id } = target else {
        return quote!();
    };
    if !body_ctx.has_weakmap_liveness_for_key(*id) {
        return quote!();
    }
    match body_ctx.weakmap_liveness_for_key(*id) {
        WeakMapLiveness::Local(ident) => quote! {
            #ident = std::rc::Rc::new(());
        },
        WeakMapLiveness::Shared { env, field } => quote! {
            *#env.#field.borrow_mut() = std::rc::Rc::new(());
        },
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

pub(super) fn emit_collections_runtime_call(
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
        RuntimeOp::ObjectKeys => {
            if let Some(sid) =
                value_ty(&args[0], ctx, emit_env, body_ctx).and_then(|t| types.struct_id(t))
                && let Some(fields) = ctx.struct_field_names(sid)
            {
                let mut ordered: Vec<&Atom> = fields.iter().collect();
                ordered.sort_by_key(|name| match name.as_str().parse::<u32>() {
                    Ok(index) if index.to_string() == name.as_str() && index != u32::MAX => {
                        (0u8, index)
                    }
                    _ => (1u8, 0),
                });
                let lits = ordered.into_iter().map(|name| {
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
        RuntimeOp::WeakMapGet => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness, _) = emit_weakmap_key(&args[1], body_ctx)?;
            let default_tokens = emit_default_for_weakmap_get(ty, emit_env, ctx);
            let value_ty_tokens = emit_type_id_with_ctx(ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_get::<#value_ty_tokens>(
                    &#handle,
                    #liveness,
                    #key,
                ).unwrap_or(#default_tokens)))
        }
        RuntimeOp::WeakMapSet => {
            let receiver = emit_owned_value(&args[0], ctx, emit_env, body_ctx)?;
            let (_, _, liveness_token) = emit_weakmap_key(&args[1], body_ctx)?;
            let value = emit_owned_value(&args[2], ctx, emit_env, body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            let handle = body_ctx.alloc_hoist_ident("__ts_aot_weakmap_receiver");
            let liveness = body_ctx.alloc_hoist_ident("__ts_aot_weakmap_liveness");
            let key = body_ctx.alloc_hoist_ident("__ts_aot_weakmap_ptr");
            Ok(quote!({
                let #handle = #receiver;
                let #liveness: std::rc::Rc<()> = #liveness_token;
                let #key = std::rc::Rc::as_ptr(&#liveness).cast::<()>();
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
            let (key, liveness, _) = emit_weakmap_key(&args[1], body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_has::<#value_ty_tokens>(
                    &#handle, #liveness, #key,
                ) != 0))
        }
        RuntimeOp::WeakMapDelete => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let (key, liveness, _) = emit_weakmap_key(&args[1], body_ctx)?;
            let value_ty = weakmap_value_type_id(&args[0], body_ctx, types)?;
            let value_ty_tokens = emit_type_id_with_ctx(value_ty, emit_env, ctx);
            Ok(quote!(__ts_aot_weak_map_delete::<#value_ty_tokens>(
                    &#handle, #liveness, #key,
                ) != 0))
        }
        RuntimeOp::WeakMapClear => {
            let handle = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_weak_map_clear(&#handle)))
        }
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
