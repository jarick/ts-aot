use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::super::ident::ident_from;
use super::super::super::runtime_op::runtime_op_ident;
use super::super::super::types::emit_type_id_with_ctx;
use super::super::call::{emit_array_slice_arg, emit_mut_receiver, mir_arg_requires_staging};
use super::super::coerce::{
    emit_owned_value, numeric_coerce, numeric_coerce_tokens, optional_coerce,
};
use super::super::expr::emit_expr;
use super::super::globals::is_owned_global;
use super::super::helpers::value_ty;
use super::super::place::{place_paths_overlap, place_root_and_path};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ts_aot_core::{Atom, Type, TypeId};
use ts_aot_ir_mir::{MirDecl, MirExpr, MirProgram, RuntimeOp};

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

fn mapped_function_param_ty(program: &MirProgram, name: &Atom, index: usize) -> Option<TypeId> {
    for decl in &program.declarations {
        match decl {
            MirDecl::Function(f) if f.name == *name => {
                return f.params.get(index).map(|p| p.ty);
            }
            MirDecl::Struct(s) => {
                for method in &s.methods {
                    if method.name == *name {
                        return method.params.get(index).map(|p| p.ty);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn array_element_ty(
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

pub(super) fn emit_arrays_runtime_call(
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
        RuntimeOp::TypedArrayNew => {
            let length = emit_expr(&args[0], ctx, emit_env, body_ctx)?;
            let kind_id = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_typed_array_new(
                (#length) as i64,
                (#kind_id) as i64
            )))
        }
        RuntimeOp::ArrayGetOrDefault => {
            let element_ty = emit_type_id_with_ctx(ty, emit_env, ctx);
            let idx = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            match &args[0] {
                MirExpr::Global(name) if is_owned_global(name, ctx, emit_env) => {
                    let arr = emit_array_slice_arg(&args[0], ctx, emit_env, body_ctx)?;
                    Ok(quote!({
                        let __ts_index = #idx as i64;
                        __ts_aot_array_get_or_default::<#element_ty>(#arr, __ts_index)
                    }))
                }
                MirExpr::Local(id) if body_ctx.shared_access_for_local(*id).is_none() => {
                    let place = body_ctx.local_ref(*id);
                    Ok(quote!({
                        let __ts_array = &(#place);
                        let __ts_index = #idx as i64;
                        __ts_aot_array_get_or_default::<#element_ty>(__ts_array, __ts_index)
                    }))
                }
                other => {
                    let arr = emit_array_slice_arg(other, ctx, emit_env, body_ctx)?;
                    Ok(quote!({
                        let __ts_array = #arr;
                        let __ts_index = #idx as i64;
                        __ts_aot_array_get_or_default::<#element_ty>(__ts_array, __ts_index)
                    }))
                }
            }
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
            let index_ty = match &args[1] {
                MirExpr::Global(name) => match fn_value_kind {
                    Some(k) => ctx.fn_value_sig(k).params.get(1).copied(),
                    None => mapped_function_param_ty(emit_env.program, name, 1),
                },
                _ => None,
            };
            let index =
                numeric_coerce_tokens(quote!(__ts_aot_idx), &Type::I64, index_ty, ctx, emit_env);
            let call = match fn_value_kind {
                Some(k) => {
                    let helper = EmitCtx::fn_call_ident(k);
                    if arity >= 2 {
                        quote!(#helper(__ctx, &#mapfn, __ts_aot_item, #index))
                    } else {
                        quote!(#helper(__ctx, &#mapfn, __ts_aot_item))
                    }
                }
                None => {
                    if arity >= 2 {
                        quote!(#mapfn(__ctx, __ts_aot_item, #index))
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
        RuntimeOp::ArrayPush => {
            let item = emit_owned_value(&args[1], ctx, emit_env, body_ctx)?;
            let item = match array_element_ty(&args[0], ctx, emit_env, body_ctx) {
                Some(element) => {
                    let item = numeric_coerce(&args[1], item, element, ctx, emit_env, body_ctx);
                    optional_coerce(&args[1], item, element, ctx, emit_env, body_ctx, false)?
                }
                None => item,
            };
            if let MirExpr::Global(name) = &args[0]
                && is_owned_global(name, ctx, emit_env)
            {
                let ident = ident_from(name);
                return Ok(quote!(unsafe {
                    {
                        let __ts_item = #item;
                        let __ts_arr = &mut *::core::ptr::addr_of_mut!(#ident);
                        __ts_arr.push(__ts_item);
                        i64::try_from(__ts_arr.len()).unwrap_or(0)
                    }
                }));
            }
            let mut receiver_prologue: Vec<TokenStream> = Vec::new();
            let (arr, needs_unsafe) =
                emit_mut_receiver(&args[0], ctx, emit_env, body_ctx, &mut receiver_prologue)?;
            let receiver_path = place_root_and_path(&args[0]);
            let item_path = place_root_and_path(&args[1]);
            let stage_item = mir_arg_requires_staging(&args[1], body_ctx)
                || place_paths_overlap(receiver_path.as_ref(), item_path.as_ref());
            let (item, item_bind) = if stage_item {
                let name = body_ctx.alloc_hoist_ident("__ts_item");
                (quote!(#name), Some(quote!(let #name = #item;)))
            } else {
                (item, None)
            };
            let call = quote!(__ts_aot_array_push(&mut #arr, #item));
            let call = if needs_unsafe {
                quote!(unsafe { #call })
            } else {
                call
            };
            Ok(if receiver_prologue.is_empty() && item_bind.is_none() {
                call
            } else {
                quote!({
                    #(#receiver_prologue)*
                    #item_bind
                    #call
                })
            })
        }
        RuntimeOp::ArraySet => {
            let mut receiver_prologue: Vec<TokenStream> = Vec::new();
            let (arr, needs_unsafe) =
                emit_mut_receiver(&args[0], ctx, emit_env, body_ctx, &mut receiver_prologue)?;
            let receiver_path = place_root_and_path(&args[0]);
            let idx_path = place_root_and_path(&args[1]);
            let value_path = place_root_and_path(&args[2]);
            let stage_value = mir_arg_requires_staging(&args[2], body_ctx)
                || place_paths_overlap(receiver_path.as_ref(), value_path.as_ref());
            let stage_idx = stage_value
                || mir_arg_requires_staging(&args[1], body_ctx)
                || place_paths_overlap(receiver_path.as_ref(), idx_path.as_ref());
            let idx = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let (idx, idx_bind) = if stage_idx {
                let name = body_ctx.alloc_hoist_ident("__ts_index");
                (quote!(#name), Some(quote!(let #name = #idx;)))
            } else {
                (idx, None)
            };
            let value = emit_owned_value(&args[2], ctx, emit_env, body_ctx)?;
            let value = match array_element_ty(&args[0], ctx, emit_env, body_ctx) {
                Some(element) => {
                    let value = numeric_coerce(&args[2], value, element, ctx, emit_env, body_ctx);
                    optional_coerce(&args[2], value, element, ctx, emit_env, body_ctx, false)?
                }
                None => value,
            };
            let (value, value_bind) = if stage_value {
                let name = body_ctx.alloc_hoist_ident("__ts_value");
                (quote!(#name), Some(quote!(let #name = #value;)))
            } else {
                (value, None)
            };
            let call = quote!(__ts_aot_array_set(&mut #arr, #idx as i64, #value));
            let call = if needs_unsafe {
                quote!(unsafe { #call })
            } else {
                call
            };
            Ok(
                if receiver_prologue.is_empty() && idx_bind.is_none() && value_bind.is_none() {
                    call
                } else {
                    quote!({
                        #(#receiver_prologue)*
                        #idx_bind
                        #value_bind
                        #call
                    })
                },
            )
        }
        RuntimeOp::ArrayLen => {
            let name = runtime_op_ident(op)?;
            if let MirExpr::OptionalChain { base, ty: chain_ty } = &args[0]
                && matches!(types.resolve(*chain_ty), Some(Type::Optional { .. }))
            {
                let base = emit_expr(base, ctx, emit_env, body_ctx)?;
                return Ok(quote!(#base.as_ref().map(|o| #name(o))));
            }
            let arr = emit_array_slice_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(#name(#arr)))
        }
        RuntimeOp::ArraySlice => {
            let element_ty_id = match types.resolve(ty) {
                Some(Type::Array { element }) => *element,
                _ => ty,
            };
            let element_ty = emit_type_id_with_ctx(element_ty_id, emit_env, ctx);
            let arr = emit_array_slice_arg(&args[0], ctx, emit_env, body_ctx)?;
            let start = emit_expr(&args[1], ctx, emit_env, body_ctx)?;
            let end = emit_expr(&args[2], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_array_slice::<#element_ty>(
                __ctx, #arr, (#start) as i64, (#end) as i64
            )))
        }
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
