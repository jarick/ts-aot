use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};

#[cfg(test)]
use ts_aot_core::TypeTable;
use ts_aot_core::{Type, TypeId};

use super::ctx::EmitCtx;
use super::ident::sanitize_ident;

#[cfg(test)]
pub(super) fn emit_type_id(id: TypeId, types: &TypeTable) -> TokenStream {
    let ctx = EmitCtx::standalone(types);
    emit_type_id_with_ctx(id, &ctx)
}

pub(super) fn emit_type_id_with_ctx(id: TypeId, ctx: &EmitCtx<'_>) -> TokenStream {
    if let Some(ty) = ctx.types.resolve(id) {
        emit_type(ty, ctx)
    } else {
        let ident = format_ident!("__ty{}", id.raw());
        quote!(#ident)
    }
}

fn emit_type(ty: &Type, ctx: &EmitCtx<'_>) -> TokenStream {
    match ty {
        Type::Void | Type::Null | Type::Error | Type::Fn { .. } | Type::Union { .. } => quote!(()),
        Type::Never => quote!(!),
        Type::Bool => quote!(bool),
        Type::I8 => quote!(i8),
        Type::I16 => quote!(i16),
        Type::I32 => quote!(i32),
        Type::I64 => quote!(i64),
        Type::U8 => quote!(u8),
        Type::U16 => quote!(u16),
        Type::U32 => quote!(u32),
        Type::U64 => quote!(u64),
        Type::F32 => quote!(f32),
        Type::F64 => quote!(f64),
        Type::String => quote!(String),
        Type::Optional { inner } => {
            let inner_tokens = emit_type_id_with_ctx(*inner, ctx);
            quote!(Option<#inner_tokens>)
        }
        Type::Array { element } => {
            let element_tokens = emit_type_id_with_ctx(*element, ctx);
            quote!(Vec<#element_tokens>)
        }
        Type::Struct { id } => {
            let ident = ctx.struct_ident(*id);
            quote!(#ident)
        }
        Type::Result { ok, err } => {
            let ok_tokens = emit_type_id_with_ctx(*ok, ctx);
            let err_tokens = emit_type_id_with_ctx(*err, ctx);
            quote!(Result<#ok_tokens, #err_tokens>)
        }
        Type::Promise { .. } => quote!(ts_aot_runtime::Promise),
        Type::Named { symbol } => {
            let raw = symbol.as_str();
            let sanitized = sanitize_ident(raw);
            let ident = Ident::new(&sanitized, Span::call_site());
            quote!(#ident)
        }
        Type::GenericParam { id } => {
            let ident = format_ident!("__generic{}", id.raw());
            quote!(#ident)
        }
    }
}
