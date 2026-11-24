use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, shared_borrow_tokens};
use super::super::literals::{emit_float_from_int, emit_whole_number_literal};
use super::super::types::emit_type_id_with_ctx;
use super::binary::{emit_binary_expr, emit_cast_expr, emit_unary_expr, optional_chain_map_arm};
use super::call::{emit_call_expr, emit_indirect_call_expr};
use super::coerce::emit_float;
use super::equality_optional::emit_typeof;
use super::expr_ops::{
    emit_await_expr, emit_is_some_expr, emit_unwrap_optional_expr, emit_yield_expr,
};
use super::globals::{emit_global_access, is_owned_global};
use super::helpers::{is_copy_type, optional_chain_inner_ty, struct_field_decl_ty, value_ty};
use super::place::expr_base_ty;
use super::self_env::emit_closure_expr;
use super::struct_literal::emit_struct_literal_expr;
use crate::error::BackendError;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::Type;
use ts_aot_ir_mir::MirExpr;

pub(super) fn emit_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let __ts_scrutinee = expr;
    match __ts_scrutinee {
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
        MirExpr::Propagate { expr, .. } => {
            let inner = emit_expr(expr, ctx, emit_env, body_ctx)?;
            Ok(quote!(#inner?))
        }
        MirExpr::Global(name) => Ok(emit_global_access(name, ctx, emit_env, body_ctx, true)),
        MirExpr::Field { .. } => emit_field_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::Index { .. } => emit_index_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::Call { .. } => emit_call_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::IndirectCall { .. } => {
            emit_indirect_call_expr(__ts_scrutinee, ctx, emit_env, body_ctx)
        }
        MirExpr::StructLiteral { .. } => {
            emit_struct_literal_expr(__ts_scrutinee, ctx, emit_env, body_ctx)
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
        MirExpr::Await { .. } => emit_await_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::OptionalChain { base, .. } => emit_expr(base, ctx, emit_env, body_ctx),
        MirExpr::IsSome { .. } => emit_is_some_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::UnwrapOptional { .. } => {
            emit_unwrap_optional_expr(__ts_scrutinee, ctx, emit_env, body_ctx)
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
        MirExpr::Yield { .. } => emit_yield_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
        MirExpr::Closure { .. } => emit_closure_expr(__ts_scrutinee, ctx, emit_env, body_ctx),
    }
}

pub(super) fn emit_value_expr(
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

pub(super) fn emit_exprs(
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

fn emit_field_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Field { base, field, .. } = expr else {
        unreachable!()
    };
    let EmitEnv { types, .. } = emit_env;
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
        let access = quote!(o.#field.borrow().clone());
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
            Some(ts_aot_core::Type::GeneratorResult { .. })
        )
    {
        let base = emit_expr(base, ctx, emit_env, body_ctx)?;
        if field.raw() == 0 {
            return Ok(quote!(#base.value.clone()));
        }
        return Ok(quote!(#base.done));
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
    let base_ty =
        value_ty(base, ctx, emit_env, body_ctx).ok_or(BackendError::NotImplemented(match base
            .as_ref()
        {
            MirExpr::Local(_) => "field base unresolved: local",
            MirExpr::Global(_) => "field base unresolved: global",
            MirExpr::Call { .. } => "field base unresolved: call",
            MirExpr::IndirectCall { .. } => "field base unresolved: indirect-call",
            MirExpr::Field { .. } => "field base unresolved: nested-field",
            MirExpr::Index { .. } => "field base unresolved: index",
            MirExpr::Cast { .. } => "field base unresolved: cast",
            _ => "field base unresolved: other",
        }))?;
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
        return Ok(quote!(#borrowed.#field.borrow().clone()));
    }
    let base = emit_expr(base, ctx, emit_env, body_ctx)?;
    Ok(quote!(#base.#field.borrow().clone()))
}

fn emit_index_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Index { base, index, .. } = expr else {
        unreachable!()
    };
    let EmitEnv { types, .. } = emit_env;
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
        if let Some(inner_ty) = optional_chain_inner_ty(emit_env, base)
            && matches!(types.resolve(inner_ty), Some(Type::String))
        {
            return Ok(quote!(#map.map(|o| ts_aot_runtime::__ts_aot_string_char_at(
                __ctx,
                o,
                #index as i64,
            ))));
        }
        let element_ty =
            optional_chain_inner_ty(emit_env, base).and_then(|inner| match types.resolve(inner) {
                Some(Type::Array { element }) => Some(*element),
                _ => None,
            });
        let access = if element_ty.is_some_and(|ty| !is_copy_type(types, ty)) {
            quote!(o.get(#index as usize).cloned())
        } else {
            quote!(o.get(#index as usize).copied())
        };
        if let Some(element_ty) = element_ty
            && matches!(
                types.resolve(element_ty),
                Some(ts_aot_core::Type::Optional { .. })
            )
        {
            return Ok(quote!(#map.and_then(|o| #access.flatten())));
        }
        return Ok(quote!(#map.and_then(|o| #access)));
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
    if let MirExpr::Global(base_name) = base.as_ref()
        && is_owned_global(base_name, ctx, emit_env)
    {
        let base = emit_global_access(base_name, ctx, emit_env, body_ctx, false);
        if expr
            .ty()
            .is_some_and(|ty| !is_copy_type(emit_env.types, ty))
        {
            return Ok(quote!({
                let __ts_index = #index as usize;
                (#base)[__ts_index].clone()
            }));
        }
        return Ok(quote!({
            let __ts_index = #index as usize;
            (#base)[__ts_index]
        }));
    }
    let base = emit_expr(base, ctx, emit_env, body_ctx)?;
    Ok(quote!(#base[#index as usize]))
}
