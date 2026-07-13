mod body;
mod ctx;
mod ident;
mod literals;
#[cfg(test)]
mod tests;
mod types;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{Type, TypeId, TypeTable, Visibility};
use ts_aot_ir_mir::{
    FunctionKind, MirDecl, MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirProgram, MirStructDecl,
};

use self::body::emit_body;
use self::ctx::EmitCtx;
use self::ident::ident_from;
use self::literals::emit_const_expr;
use self::types::emit_type_id_with_ctx;
use crate::error::BackendError;

pub fn emit_decls(program: &MirProgram, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::new(program, types);
    let mut tokens = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for decl in &program.declarations {
        let (decl_tokens, entries) = emit_decl(decl, &ctx)?;
        tokens.extend(decl_tokens);
        dispatch_entries.extend(entries);
    }
    if !dispatch_entries.is_empty() {
        tokens.extend(quote! {
            const __TS_AOT_DISPATCH_TABLE: &[(&str, fn(&[u64]) -> u64)] = &[
                #(#dispatch_entries,)*
            ];
        });
    }
    Ok(tokens)
}

fn emit_decl(
    decl: &MirDecl,
    ctx: &EmitCtx<'_>,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    match decl {
        MirDecl::Function(f) => emit_function_with_ctx(f, ctx),
        MirDecl::Struct(s) => emit_struct_with_ctx(s, ctx),
        MirDecl::Global(g) => emit_global_with_ctx(g, ctx),
    }
}

#[cfg(test)]
fn emit_function(f: &MirFunctionDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::standalone(types);
    Ok(emit_function_with_ctx(f, &ctx)?.0)
}

fn emit_function_with_ctx(
    f: &MirFunctionDecl,
    ctx: &EmitCtx<'_>,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let name = ident_from(&f.name);
    let params = emit_params(f, ctx);
    let ret = emit_type_id_with_ctx(f.ret, ctx);
    let vis = if f.export_name.is_some() {
        quote!(pub)
    } else {
        quote!()
    };
    let asyncness = if f.effects.is_async {
        quote!(async)
    } else {
        quote!()
    };
    let self_token = self_param_token(f.kind);
    let body = emit_body(f, ctx)?;

    let fn_tokens = quote! {
        #vis #asyncness fn #name(#self_token #(#params),*) -> #ret #body
    };

    let dispatch_entry = emit_dispatch_entry(f, ctx);
    let (tokens, entries) = match dispatch_entry {
        Some((wrapper_name, entry)) => {
            let wrapper = build_dispatch_wrapper(f, &wrapper_name, ctx)?;
            (quote! { #fn_tokens #wrapper }, vec![entry])
        }
        None => (fn_tokens, Vec::new()),
    };
    Ok((tokens, entries))
}

fn dispatch_wrapper_ident(name: &Ident) -> Ident {
    let raw = name.to_string();
    format_ident!("__ts_aot_dispatch_{}", raw)
}

fn emit_dispatch_entry(f: &MirFunctionDecl, ctx: &EmitCtx<'_>) -> Option<(Ident, TokenStream)> {
    if !is_dispatchable(f) {
        return None;
    }
    if f.params.iter().any(|p| !is_u64_arg_packable(p.ty, ctx)) {
        return None;
    }
    if !is_u64_ret_packable(f.ret, ctx) {
        return None;
    }
    let name = ident_from(&f.name);
    let wrapper = dispatch_wrapper_ident(&name);
    let name_lit = proc_macro2::Literal::string(f.name.as_str());
    let entry = quote! { (#name_lit, #wrapper as fn(&[u64]) -> u64) };
    Some((wrapper, entry))
}

fn is_dispatchable(f: &MirFunctionDecl) -> bool {
    if f.effects.is_async {
        return false;
    }
    matches!(
        f.kind,
        FunctionKind::Plain | FunctionKind::Closure | FunctionKind::RuntimeShim
    )
}

fn is_u64_arg_packable(ty: TypeId, ctx: &EmitCtx<'_>) -> bool {
    let Some(resolved) = ctx.types.resolve(ty) else {
        return false;
    };
    matches!(
        resolved,
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
    )
}

fn is_u64_ret_packable(ty: TypeId, ctx: &EmitCtx<'_>) -> bool {
    let Some(resolved) = ctx.types.resolve(ty) else {
        return false;
    };
    matches!(
        resolved,
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
            | Type::Void
    )
}

fn build_dispatch_wrapper(
    f: &MirFunctionDecl,
    wrapper_name: &Ident,
    ctx: &EmitCtx<'_>,
) -> Result<TokenStream, BackendError> {
    let name = ident_from(&f.name);
    let mut unpack_stmts: Vec<TokenStream> = Vec::new();
    let mut call_args: Vec<TokenStream> = Vec::new();
    for (idx, p) in f.params.iter().enumerate() {
        let pname = ident_from(&p.name);
        let unpacked = unpack_arg_stmt(&pname, idx, p.ty, ctx)?;
        unpack_stmts.push(unpacked);
        call_args.push(quote!(#pname));
    }
    let ret_ty = emit_type_id_with_ctx(f.ret, ctx);
    let ret_expr = pack_return_stmt(f.ret, ctx)?;
    Ok(quote! {
        fn #wrapper_name(args: &[u64]) -> u64 {
            #(#unpack_stmts)*
            let __result: #ret_ty = #name(#(#call_args),*);
            #ret_expr
        }
    })
}

fn unpack_arg_stmt(
    pname: &Ident,
    idx: usize,
    ty: TypeId,
    ctx: &EmitCtx<'_>,
) -> Result<TokenStream, BackendError> {
    let resolved = ctx.types.resolve(ty).expect("is_u64_packable checked");
    let pty = emit_type_id_with_ctx(ty, ctx);
    let slot = format_ident!("__slot_{}", idx);
    let get = quote!(let #slot = args[#idx];);
    let cast = match resolved {
        Type::Bool
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64 => {
            quote!(let #pname: #pty = #slot as #pty;)
        }
        Type::F32 | Type::F64 => {
            quote!(let #pname: #pty = <#pty>::from_bits(#slot);)
        }
        Type::Void => quote!(),
        _ => return Err(BackendError::NotImplemented),
    };
    Ok(quote! {
        #get
        #cast
    })
}

fn pack_return_stmt(ty: TypeId, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    let resolved = ctx.types.resolve(ty).expect("is_u64_packable checked");
    let stmt = match resolved {
        Type::Void => quote!(0),
        Type::Bool
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64 => {
            quote!(__result as u64)
        }
        Type::F32 | Type::F64 => {
            quote!(__result.to_bits())
        }
        _ => return Err(BackendError::NotImplemented),
    };
    Ok(stmt)
}

fn emit_struct_with_ctx(
    s: &MirStructDecl,
    ctx: &EmitCtx<'_>,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let name = ctx.struct_ident(s.id);
    let fields = s.fields.iter().map(|f| emit_field(f, ctx));
    let mut methods = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for m in &s.methods {
        let (m_tokens, m_entries) = emit_function_with_ctx(m, ctx)?;
        methods.extend(m_tokens);
        dispatch_entries.extend(m_entries);
    }
    let tokens = quote! {
        pub struct #name {
            #(#fields,)*
        }

        impl #name {
            #methods
        }
    };
    Ok((tokens, dispatch_entries))
}

fn emit_field(field: &MirFieldDecl, ctx: &EmitCtx<'_>) -> TokenStream {
    let name = ident_from(&field.name);
    let ty = emit_type_id_with_ctx(field.ty, ctx);
    let vis = visibility_token(field.visibility);
    quote! {
        #vis #name: #ty
    }
}

#[cfg(test)]
fn emit_global(g: &MirGlobalDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::standalone(types);
    Ok(emit_global_with_ctx(g, &ctx)?.0)
}

fn emit_global_with_ctx(
    g: &MirGlobalDecl,
    ctx: &EmitCtx<'_>,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let Some(expr) = &g.init else {
        return Err(BackendError::NotImplemented);
    };
    let name = ident_from(&g.name);
    let ty = emit_type_id_with_ctx(g.ty, ctx);
    let vis = visibility_token(g.visibility);
    let mutability = if g.mutable { quote!(mut) } else { quote!() };
    let init = emit_const_expr(expr)?;
    Ok((
        quote! {
            #vis static #mutability #name: #ty = #init;
        },
        Vec::new(),
    ))
}

fn emit_params(f: &MirFunctionDecl, ctx: &EmitCtx<'_>) -> Vec<TokenStream> {
    let self_param = match f.kind {
        FunctionKind::Method { self_param, .. } => Some(self_param),
        _ => None,
    };
    f.params
        .iter()
        .filter(|p| Some(p.id) != self_param)
        .map(|p| {
            let name = ident_from(&p.name);
            let ty = emit_type_id_with_ctx(p.ty, ctx);
            quote!(#name: #ty)
        })
        .collect()
}

fn self_param_token(kind: FunctionKind) -> TokenStream {
    match kind {
        FunctionKind::Method { .. } | FunctionKind::Constructor { .. } => quote!(self,),
        _ => TokenStream::new(),
    }
}

fn visibility_token(vis: Visibility) -> TokenStream {
    match vis {
        Visibility::Public => quote!(pub),
        Visibility::Private | Visibility::Protected => quote!(),
    }
}
