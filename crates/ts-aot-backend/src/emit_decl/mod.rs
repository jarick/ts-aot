mod body;
mod ctx;
mod ident;
mod literals;
mod runtime_op;
#[cfg(test)]
mod tests;
mod types;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{Type, TypeId, TypeTable, Visibility};
use ts_aot_ir_mir::{
    FunctionKind, MirDecl, MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirProgram, MirStructDecl,
};

use self::body::emit_body;
use self::ctx::BodyCtx;
use self::ctx::EmitCtx;
use self::ctx::EmitEnv;
use self::ident::ident_from;
use self::literals::emit_const_expr;
use self::types::emit_type_id_with_ctx;
use crate::error::BackendError;

pub fn emit_decls(program: &MirProgram, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::build(program);
    let emit_env = EmitEnv { types, program };
    let mut tokens = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for decl in &program.declarations {
        let (decl_tokens, entries) = emit_decl(decl, &ctx, &emit_env)?;
        tokens.extend(decl_tokens);
        dispatch_entries.extend(entries);
    }
    if program.tla_main_name.is_some() {
        tokens.extend(emit_tla_main_entry(program));
    } else if program.is_module {
        tokens.extend(emit_tla_main_noop_entry());
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

fn is_generated_tla_main(f: &MirFunctionDecl, program: &MirProgram) -> bool {
    program
        .tla_main_name
        .as_ref()
        .is_some_and(|name| *name == f.name)
}

fn tla_main_rust_name() -> Ident {
    Ident::new("__ts_aot_tla_main", Span::call_site())
}

fn emit_tla_main_entry(program: &MirProgram) -> TokenStream {
    if tla_main_is_async(program) {
        quote! {
            fn main() {
                ts_aot_runtime::__ts_aot_runtime_run(__ts_aot_tla_main());
            }
        }
    } else {
        quote! {
            fn main() {
                __ts_aot_tla_main();
            }
        }
    }
}

fn tla_main_is_async(program: &MirProgram) -> bool {
    let Some(name) = program.tla_main_name.as_ref() else {
        return false;
    };
    program.declarations.iter().any(|decl| match decl {
        MirDecl::Function(f) => f.name == *name && f.effects.is_async,
        _ => false,
    })
}

fn emit_tla_main_noop_entry() -> TokenStream {
    quote! {
        fn main() {}
    }
}

fn emit_decl(
    decl: &MirDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    match decl {
        MirDecl::Function(f) => emit_function_with_ctx(f, ctx, emit_env),
        MirDecl::Struct(s) => emit_struct_with_ctx(s, ctx, emit_env),
        MirDecl::Global(g) => emit_global_with_ctx(g, ctx, emit_env),
    }
}

#[cfg(test)]
use ts_aot_core::ModuleId;

#[cfg(test)]
fn emit_function(f: &MirFunctionDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let empty_program = MirProgram::new(ModuleId::from_raw(0));
    let ctx = EmitCtx::build(&empty_program);
    let emit_env = EmitEnv {
        types,
        program: &empty_program,
    };
    Ok(emit_function_with_ctx(f, &ctx, &emit_env)?.0)
}

fn emit_function_with_ctx(
    f: &MirFunctionDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let EmitEnv { types, program } = emit_env;
    let is_tla_main = is_generated_tla_main(f, program);
    let emitted_name = if is_tla_main {
        tla_main_rust_name()
    } else {
        ident_from(&f.name)
    };
    let mut body_ctx = BodyCtx::new(f, types);
    if is_tla_main {
        body_ctx.set_in_tla_main(true);
    }
    let params = emit_params(f, ctx, emit_env, &body_ctx);
    let ret = emit_type_id_with_ctx(f.ret, emit_env, ctx);
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
    let self_token = self_param_token(&f.kind, &body_ctx);
    let body = emit_body(f, ctx, emit_env, &body_ctx)?;

    let fn_tokens = quote! {
        #vis #asyncness fn #emitted_name(#self_token #(#params),*) -> #ret #body
    };

    let dispatch_entry = emit_dispatch_entry(f, emit_env);
    let (tokens, entries) = match dispatch_entry {
        Some((wrapper_name, entry)) => {
            let wrapper = build_dispatch_wrapper(f, &wrapper_name, &body_ctx, ctx, emit_env)?;
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

fn emit_dispatch_entry(f: &MirFunctionDecl, emit_env: &EmitEnv) -> Option<(Ident, TokenStream)> {
    let EmitEnv { types, program } = emit_env;
    if !is_dispatchable(f) {
        return None;
    }
    if f.params.iter().any(|p| !is_u64_arg_packable(p.ty, types)) {
        return None;
    }
    if !is_u64_ret_packable(f.ret, types) {
        return None;
    }
    let name = if is_generated_tla_main(f, program) {
        tla_main_rust_name()
    } else {
        ident_from(&f.name)
    };
    let wrapper = dispatch_wrapper_ident(&name);
    let name_lit = Literal::string(f.name.as_str());
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

fn is_u64_arg_packable(ty: TypeId, types: &TypeTable) -> bool {
    let Some(resolved) = types.resolve(ty) else {
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
            | Type::Date
            | Type::Symbol
    )
}

fn is_u64_ret_packable(ty: TypeId, types: &TypeTable) -> bool {
    let Some(resolved) = types.resolve(ty) else {
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
            | Type::Date
            | Type::Symbol
            | Type::Void
    )
}

fn build_dispatch_wrapper(
    f: &MirFunctionDecl,
    wrapper_name: &Ident,
    body_ctx: &BodyCtx,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, program } = emit_env;
    let name = if is_generated_tla_main(f, program) {
        tla_main_rust_name()
    } else {
        ident_from(&f.name)
    };
    let mut unpack_stmts: Vec<TokenStream> = Vec::new();
    let mut call_args: Vec<TokenStream> = Vec::new();
    for (idx, p) in f.params.iter().enumerate() {
        let pname = body_ctx.local_ident(p.id);
        let unpacked = unpack_arg_stmt(&pname, idx, p.ty, ctx, emit_env)?;
        unpack_stmts.push(unpacked);
        call_args.push(quote!(#pname));
    }
    let ret_ty = emit_type_id_with_ctx(f.ret, emit_env, ctx);
    let ret_expr = pack_return_stmt(f.ret, types)?;
    Ok(quote! {
        pub fn #wrapper_name(_args: &[u64]) -> u64 {
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
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    let resolved = types.resolve(ty).expect("is_u64_packable checked");
    let pty = emit_type_id_with_ctx(ty, emit_env, ctx);
    let slot = format_ident!("__slot_{}", idx);
    let get = quote!(let #slot = _args[#idx];);
    let cast = match resolved {
        Type::Bool
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::Date
        | Type::Symbol
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

fn pack_return_stmt(ty: TypeId, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let resolved = types.resolve(ty).expect("is_u64_packable checked");
    let stmt = match resolved {
        Type::Void => quote!(0),
        Type::Bool
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::Date
        | Type::Symbol
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
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let name = ctx.struct_ident(s.id);
    let class_id_raw: u32 = s.id.raw();
    let fields = s.fields.iter().map(|f| emit_field(f, ctx, emit_env));
    let mut methods = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for m in &s.methods {
        let (m_tokens, m_entries) = emit_function_with_ctx(m, ctx, emit_env)?;
        methods.extend(m_tokens);
        dispatch_entries.extend(m_entries);
    }
    let tokens = quote! {
        #[derive(Clone, Debug)]
        pub struct #name {
            #(#fields,)*
        }

        impl TsClassId for #name {
            fn class_id() -> u32 {
                #class_id_raw
            }
        }

        impl #name {
            #methods
        }
    };
    Ok((tokens, dispatch_entries))
}

fn emit_field(field: &MirFieldDecl, ctx: &EmitCtx, emit_env: &EmitEnv) -> TokenStream {
    let name = ident_from(&field.name);
    let ty = emit_type_id_with_ctx(field.ty, emit_env, ctx);
    let vis = visibility_token(field.visibility);
    quote! {
        #vis #name: #ty
    }
}

#[cfg(test)]
fn emit_global(g: &MirGlobalDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let empty_program = MirProgram::new(ModuleId::from_raw(0));
    let ctx = EmitCtx::build(&empty_program);
    let emit_env = EmitEnv {
        types,
        program: &empty_program,
    };
    Ok(emit_global_with_ctx(g, &ctx, &emit_env)?.0)
}

fn emit_global_with_ctx(
    g: &MirGlobalDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let name = ident_from(&g.name);
    let ty = emit_type_id_with_ctx(g.ty, emit_env, ctx);
    let vis = visibility_token(g.visibility);
    let mutability = if g.mutable { quote!(mut) } else { quote!() };
    let init = if let Some(expr) = &g.init {
        emit_const_expr(expr)?
    } else {
        quote!(Default::default())
    };
    Ok((
        quote! {
            #vis static #mutability #name: #ty = #init;
        },
        Vec::new(),
    ))
}

fn emit_params(
    f: &MirFunctionDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Vec<TokenStream> {
    f.params
        .iter()
        .filter(|p| Some(p.id) != body_ctx.self_param())
        .map(|p| {
            let name = body_ctx.local_ident(p.id);
            let ty = emit_type_id_with_ctx(p.ty, emit_env, ctx);
            let mutability = if body_ctx.local_mut(p.id) {
                quote!(mut)
            } else {
                quote!()
            };
            quote!(#mutability #name: #ty)
        })
        .collect()
}

fn self_param_token(kind: &FunctionKind, body_ctx: &BodyCtx) -> TokenStream {
    match *kind {
        FunctionKind::Method { self_param, .. }
        | FunctionKind::GeneratorMethod { self_param, .. } => {
            if body_ctx.local_mut(self_param) {
                quote!(mut self,)
            } else {
                quote!(self,)
            }
        }
        FunctionKind::Constructor { .. } => quote!(self,),
        _ => TokenStream::new(),
    }
}

fn visibility_token(vis: Visibility) -> TokenStream {
    match vis {
        Visibility::Public => quote!(pub),
        Visibility::Private | Visibility::Protected => quote!(),
    }
}
