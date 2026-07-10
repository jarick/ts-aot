mod body;
mod ctx;
mod ident;
mod literals;
#[cfg(test)]
mod tests;
mod types;

use proc_macro2::TokenStream;
use quote::quote;

use ts_aot_core::{TypeTable, Visibility};
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
    for decl in &program.declarations {
        tokens.extend(emit_decl(decl, &ctx)?);
    }
    Ok(tokens)
}

fn emit_decl(decl: &MirDecl, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    match decl {
        MirDecl::Function(f) => emit_function_with_ctx(f, ctx),
        MirDecl::Struct(s) => emit_struct_with_ctx(s, ctx),
        MirDecl::Global(g) => emit_global_with_ctx(g, ctx),
    }
}

#[cfg(test)]
fn emit_function(f: &MirFunctionDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::standalone(types);
    emit_function_with_ctx(f, &ctx)
}

fn emit_function_with_ctx(
    f: &MirFunctionDecl,
    ctx: &EmitCtx<'_>,
) -> Result<TokenStream, BackendError> {
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

    Ok(quote! {
        #vis #asyncness fn #name(#self_token #(#params),*) -> #ret #body
    })
}

fn emit_struct_with_ctx(s: &MirStructDecl, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    let name = ctx.struct_ident(s.id);
    let fields = s.fields.iter().map(|f| emit_field(f, ctx));
    let mut methods = TokenStream::new();
    for m in &s.methods {
        methods.extend(emit_function_with_ctx(m, ctx)?);
    }
    Ok(quote! {
        pub struct #name {
            #(#fields,)*
        }

        impl #name {
            #methods
        }
    })
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
    emit_global_with_ctx(g, &ctx)
}

fn emit_global_with_ctx(g: &MirGlobalDecl, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    let Some(expr) = &g.init else {
        return Err(BackendError::NotImplemented);
    };
    let name = ident_from(&g.name);
    let ty = emit_type_id_with_ctx(g.ty, ctx);
    let vis = visibility_token(g.visibility);
    let mutability = if g.mutable { quote!(mut) } else { quote!() };
    let init = emit_const_expr(expr)?;
    Ok(quote! {
        #vis static #mutability #name: #ty = #init;
    })
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
