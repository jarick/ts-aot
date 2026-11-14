use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{Type, TypeId, TypeTable};

use super::ctx::EmitCtx;
use super::ctx::EmitEnv;
use super::ident::sanitize_ident;

#[cfg(test)]
use ts_aot_ir_mir::MirProgram;

#[cfg(test)]
use ts_aot_core::ModuleId;

/// Whether a type mentions a generic parameter, at any depth. A function whose
/// signature does is a generic original: there is no Rust type to give its
/// `__generic0` (the backend emits the bare identifier, which nothing defines), and
/// monomorphization replaced every call to it with a specialization, so it is
/// skipped rather than emitted.
pub(super) fn mentions_generic_param(ty: TypeId, types: &TypeTable) -> bool {
    fn walk(ty: TypeId, types: &TypeTable, visited: &mut Vec<TypeId>) -> bool {
        // A type is interned, so only a recursive alias can make the walk loop; remembering
        // what has been inspected terminates that. A fixed depth limit cannot: a generic
        // parameter nested deeper than it would be missed, and the emitted signature would
        // name `__generic0`, which nothing defines.
        if visited.contains(&ty) {
            return false;
        }
        visited.push(ty);
        match types.resolve(ty) {
            Some(Type::GenericParam { .. }) => true,
            Some(Type::Optional { inner, .. } | Type::Array { element: inner }) => {
                walk(*inner, types, visited)
            }
            Some(Type::Promise { ok, err }) => {
                walk(*ok, types, visited) || err.is_some_and(|e| walk(e, types, visited))
            }
            Some(Type::WeakMap { key, value }) => {
                walk(*key, types, visited) || walk(*value, types, visited)
            }
            Some(Type::Result { ok, err }) => {
                walk(*ok, types, visited) || walk(*err, types, visited)
            }
            Some(Type::Union { variants }) => variants.iter().any(|v| walk(*v, types, visited)),
            Some(Type::Intersection { parts }) => parts.iter().any(|v| walk(*v, types, visited)),
            Some(Type::Tuple { elements }) => elements.iter().any(|v| walk(*v, types, visited)),
            Some(
                Type::Generator { inner, .. }
                | Type::GeneratorResult { inner }
                | Type::AsyncGenYield { inner },
            ) => walk(*inner, types, visited),
            Some(Type::Fn { params, ret, .. }) => {
                params.iter().any(|p| walk(*p, types, visited)) || walk(*ret, types, visited)
            }
            _ => false,
        }
    }
    walk(ty, types, &mut Vec::new())
}

#[cfg(test)]
pub(super) fn emit_type_id(id: TypeId, types: &TypeTable) -> TokenStream {
    let empty_program = MirProgram::new(ModuleId::from_raw(0));
    let ctx = EmitCtx::build(&empty_program, types);
    let emit_env = EmitEnv {
        types,
        program: &empty_program,
    };
    emit_type_id_with_ctx(id, &emit_env, &ctx)
}

pub(super) fn emit_type_id_with_ctx(id: TypeId, emit_env: &EmitEnv, ctx: &EmitCtx) -> TokenStream {
    emit_type_id_impl(id, emit_env, ctx, LifetimeMode::Full)
}

pub(super) fn emit_static_type_id(id: TypeId, emit_env: &EmitEnv, ctx: &EmitCtx) -> TokenStream {
    emit_type_id_impl(id, emit_env, ctx, LifetimeMode::Static)
}

#[derive(Clone, Copy)]
pub(super) enum LifetimeMode {
    Full,
    Static,
}

impl LifetimeMode {
    fn lifetime(self) -> TokenStream {
        match self {
            LifetimeMode::Full => quote!('a),
            LifetimeMode::Static => quote!('static),
        }
    }
}

fn emit_type_id_impl(
    id: TypeId,
    emit_env: &EmitEnv,
    ctx: &EmitCtx,
    mode: LifetimeMode,
) -> TokenStream {
    let EmitEnv { types, .. } = emit_env;
    if let Some(ty) = types.resolve(id) {
        if matches!(ty, Type::Fn { .. }) {
            if let Some(k) = ctx.fn_value_kind(id) {
                let enum_name = EmitCtx::fn_value_ident(k);
                let lt = mode.lifetime();
                return quote!(#enum_name<#lt>);
            }
            return quote!(());
        }
        emit_type(ty, emit_env, ctx, mode)
    } else {
        let ident = format_ident!("__ty{}", id.raw());
        quote!(#ident)
    }
}

fn emit_type(ty: &Type, emit_env: &EmitEnv, ctx: &EmitCtx, mode: LifetimeMode) -> TokenStream {
    let rec = |inner: TypeId| emit_type_id_impl(inner, emit_env, ctx, mode);
    match ty {
        Type::Void
        | Type::Null
        | Type::Error
        | Type::Fn { .. }
        | Type::Union { .. }
        | Type::Intersection { .. }
        | Type::Tuple { .. } => quote!(()),
        Type::Bool => quote!(bool),
        Type::I8 => quote!(i8),
        Type::I16 => quote!(i16),
        Type::I32 => quote!(i32),
        Type::I64 | Type::Date | Type::Symbol => quote!(i64),
        Type::BigInt => quote!(ts_aot_runtime::BigIntHandle),
        Type::U8 => quote!(u8),
        Type::U16 => quote!(u16),
        Type::U32 => quote!(u32),
        Type::U64 => quote!(u64),
        Type::F32 => quote!(f32),
        Type::F64 => quote!(f64),
        Type::String => {
            let lt = mode.lifetime();
            quote!(ts_aot_runtime::JsString<#lt>)
        }
        Type::Optional { inner, .. } => {
            let inner_tokens = rec(*inner);
            quote!(Option<#inner_tokens>)
        }
        Type::Array { element } => {
            let element_tokens = rec(*element);
            let lt = mode.lifetime();
            quote!(ts_aot_runtime::bumpalo::collections::Vec<#lt, #element_tokens>)
        }
        Type::Struct { id } => {
            let ident = ctx.struct_ident(*id);
            if ctx.struct_needs_lifetime(*id) {
                let lt = mode.lifetime();
                quote!(#ident<#lt>)
            } else {
                quote!(#ident)
            }
        }
        Type::Result { ok, err } => {
            let ok_tokens = rec(*ok);
            let err_tokens = rec(*err);
            quote!(Result<#ok_tokens, #err_tokens>)
        }
        Type::Promise { ok, err: _ } => {
            let ok_tokens = rec(*ok);
            quote!(ts_aot_runtime::Promise<#ok_tokens>)
        }
        Type::ArrayBuffer => quote!(ts_aot_runtime::ArrayBufferHandle),
        Type::WeakMap { value, .. } => {
            let value_tokens = rec(*value);
            quote!(ts_aot_runtime::WeakMapHandle<#value_tokens>)
        }
        Type::Int8Array
        | Type::Uint8Array
        | Type::Uint8ClampedArray
        | Type::Int16Array
        | Type::Uint16Array
        | Type::Int32Array
        | Type::Uint32Array
        | Type::Float32Array
        | Type::Float64Array => quote!(ts_aot_runtime::TypedArrayHandle),
        Type::Named { symbol } => {
            let raw = symbol.as_str();
            let sanitized = sanitize_ident(raw);
            let ident = Ident::new(&sanitized, Span::call_site());
            quote!(#ident)
        }
        Type::Generator { inner, is_async } => {
            let inner = rec(*inner);
            if *is_async {
                quote!(ts_aot_runtime::AsyncGenerator<#inner>)
            } else {
                quote!(ts_aot_runtime::Generator<#inner>)
            }
        }
        Type::GeneratorResult { inner } => {
            let inner = rec(*inner);
            quote!(ts_aot_runtime::GeneratorResult<#inner>)
        }
        Type::AsyncGenYield { inner } => {
            let inner = rec(*inner);
            quote!(ts_aot_runtime::AsyncGenYield<#inner>)
        }
        Type::GenericParam { id } => {
            let ident = format_ident!("__generic{}", id.raw());
            quote!(#ident)
        }
    }
}

pub(super) fn is_comparable_primitive(ty: &Type) -> bool {
    matches!(
        ty,
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
            | Type::String
    )
}
