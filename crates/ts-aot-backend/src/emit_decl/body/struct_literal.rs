use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::literals::emit_float_from_int;
use super::coerce::{emit_owned_value, numeric_coerce, optional_coerce};
use super::equality::is_nullish_literal;
use super::helpers::is_float_type;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use ts_aot_core::{FieldId, Type, TypeId};
use ts_aot_ir_mir::{MirDecl, MirExpr, UnaryOp};

pub(crate) fn coerce_static_init(
    expr: &MirExpr,
    tokens: TokenStream,
    ty: TypeId,
    emit_env: &EmitEnv,
) -> TokenStream {
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

pub(super) fn emit_struct_literal_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::StructLiteral {
        struct_id, fields, ..
    } = expr
    else {
        unreachable!()
    };
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
    let field_tys: std::collections::HashMap<FieldId, TypeId> = declared.iter().copied().collect();
    let mut emitted: Vec<TokenStream> = Vec::with_capacity(fields.len());
    for (field_id, value) in fields {
        let field = ctx.struct_field_ident(*struct_id, *field_id);
        let value_tokens = emit_owned_value(value, ctx, emit_env, body_ctx)?;
        let value_tokens = match field_tys.get(field_id) {
            Some(fty) => {
                let coerced = numeric_coerce(value, value_tokens, *fty, ctx, emit_env, body_ctx);
                optional_coerce(value, coerced, *fty, ctx, emit_env, body_ctx, false)?
            }
            None => value_tokens,
        };
        emitted.push(quote!(#field: ::std::rc::Rc::new(::core::cell::RefCell::new(#value_tokens))));
    }
    let listed: HashSet<FieldId> = fields.iter().map(|(id, _)| *id).collect();
    let mut budget = super::super::CONST_DEFAULT_BUDGET;
    for (fid, fty) in declared {
        if listed.contains(&fid) {
            continue;
        }
        let fname = ctx.struct_field_ident(*struct_id, fid);
        let fdef = super::super::emit_const_default(
            fty,
            ctx,
            emit_env,
            0,
            &mut budget,
            super::super::types::LifetimeMode::Full,
        )?;
        emitted.push(quote!(#fname: ::std::rc::Rc::new(::core::cell::RefCell::new(#fdef))));
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
