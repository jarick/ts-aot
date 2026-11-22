use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, shared_write_tokens};
use super::super::types::emit_type_id_with_ctx;
use super::helpers::{loop_label, type_requires_arena};
use super::stmt::emit_block_stmts;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ts_aot_core::{LocalId, Type};
use ts_aot_ir_mir::MirBlock;

pub(super) fn await_helper_path(body_ctx: &BodyCtx) -> TokenStream {
    if body_ctx.in_try() {
        quote!(ts_aot_runtime::__ts_aot_await_value_raw)
    } else if body_ctx.is_async_generator() {
        quote!(ts_aot_runtime::__ts_aot_await_value_gen)
    } else {
        quote!(ts_aot_runtime::__ts_aot_await_value)
    }
}

pub(super) fn emit_try(
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
    let slot_ident = body_ctx.alloc_hoist_ident(&format!("__return_slot_{try_id}"));
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
                        .is_some_and(super::super::types::is_comparable_primitive)
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
                } else if param_ty_id.is_some_and(|t| type_requires_arena(t, ctx, emit_env)) {
                    return Err(BackendError::NotImplemented(
                        "catch binding: this type borrows the arena and cannot be recovered from \
                         the `'static` panic boundary of `try`/`catch`; catch a primitive, a \
                         string, or a type that owns its data instead",
                    ));
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
