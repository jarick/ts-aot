use super::super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::super::runtime_op::runtime_op_ident;
use super::super::super::types::emit_type_id_with_ctx;
use super::super::expr::emit_expr;
use super::super::helpers::value_ty;
use super::runtime_op_takes_ctx;
use crate::error::BackendError;
use proc_macro2::{Literal, TokenStream};
use quote::quote;
use ts_aot_core::{Type, TypeId};
use ts_aot_ir_mir::{MirExpr, RuntimeOp};

fn string_op_string_arg_indices(op: RuntimeOp) -> &'static [usize] {
    match op {
        RuntimeOp::StringLen | RuntimeOp::StringCharAt => &[0],
        RuntimeOp::StringConcat | RuntimeOp::StringEquals | RuntimeOp::StringIndexOf => &[0, 1],
        _ => &[],
    }
}

pub(super) fn emit_js_string_arg(
    arg: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Borrowed, ctx, emit_env, body_ctx)
}

pub(super) fn emit_js_string_owned(
    arg: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Owned, ctx, emit_env, body_ctx)
}

#[derive(Clone, Copy)]
pub(super) enum StringOwnership {
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

pub(super) fn emit_strings_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match op {
        RuntimeOp::JsonParseString => {
            let s = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_json_parse_string(__ctx, #s)))
        }
        RuntimeOp::JsonStringifyString => {
            let s = emit_js_string_arg(&args[0], ctx, emit_env, body_ctx)?;
            Ok(quote!(__ts_aot_json_stringify_string(__ctx, #s)))
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
        _ => super::emit_default_runtime_call(op, args, ctx, emit_env, body_ctx),
    }
}
