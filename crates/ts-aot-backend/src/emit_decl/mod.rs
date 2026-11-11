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

use ts_aot_core::{FieldId, StructId, Type, TypeId, TypeTable, Visibility};
use ts_aot_ir_mir::{
    FunctionKind, MirDecl, MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirProgram, MirStructDecl,
};

use self::body::emit_body;
use self::ctx::BodyCtx;
use self::ctx::EmitCtx;
use self::ctx::EmitEnv;
use self::ident::ident_from;
use self::literals::emit_const_expr;
use self::types::{LifetimeMode, emit_static_type_id, emit_type_id_with_ctx};
use crate::error::BackendError;

pub fn emit_decls(program: &MirProgram, types: &TypeTable) -> Result<TokenStream, BackendError> {
    emit_decls_with_style(program, types, false)
}

pub fn emit_decls_with_style(
    program: &MirProgram,
    types: &TypeTable,
    lib_style: bool,
) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::build(program, types);
    let emit_env = EmitEnv { types, program };
    let mut tokens = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for decl in &program.declarations {
        let (decl_tokens, entries) = emit_decl(decl, &ctx, &emit_env)?;
        tokens.extend(decl_tokens);
        dispatch_entries.extend(entries);
    }
    if lib_style {
        if program.tla_main_name.is_some() {
            tokens.extend(emit_tla_init_entry(program));
        }
    } else if program.tla_main_name.is_some() {
        tokens.extend(emit_tla_main_entry(program));
    } else if program.is_module {
        tokens.extend(emit_tla_main_noop_entry());
    }
    if !dispatch_entries.is_empty() {
        let vis = if lib_style { quote!(pub) } else { quote!() };
        tokens.extend(quote! {
            #vis const __TS_AOT_DISPATCH_TABLE: &[(&str, for<'a> fn(&ts_aot_runtime::Ctx<'a>, &[u64]) -> u64)] = &[
                #(#dispatch_entries,)*
            ];
        });
    }
    tokens.extend(emit_fn_values(&ctx, &emit_env));
    if lib_style {
        tokens.extend(quote!(
            pub use ts_aot_runtime::*;
        ));
    } else if ctx.runtime_used() {
        tokens.extend(quote!(
            use ts_aot_runtime::*;
        ));
    }
    Ok(tokens)
}

fn emit_fn_values(ctx: &EmitCtx, emit_env: &EmitEnv) -> TokenStream {
    let mut tokens = TokenStream::new();
    for k in 0..ctx.fn_value_count() {
        let enum_name = EmitCtx::fn_value_ident(k);
        let helper = EmitCtx::fn_call_ident(k);
        let sig = ctx.fn_value_sig(k);
        let variants = ctx.fn_value_variants(k);
        let closure_variants = ctx.closure_variants_for(k);
        let param_tys: Vec<TokenStream> = sig
            .params
            .iter()
            .map(|t| emit_type_id_with_ctx(*t, emit_env, ctx))
            .collect();
        let ret = emit_type_id_with_ctx(sig.ret, emit_env, ctx);
        let arg_names: Vec<Ident> = (0..sig.params.len())
            .map(|i| format_ident!("__a{}", i))
            .collect();

        let mut enum_variants: Vec<TokenStream> =
            variants.iter().map(|(_, v, _, _)| quote!(#v)).collect();
        let mut arms: Vec<TokenStream> = Vec::new();
        let mut debug_arms: Vec<TokenStream> = Vec::new();
        for (fname, variant, is_throwing, is_async) in variants {
            let fident = ident_from(fname);
            let mut call = quote!(#fident(__ctx, #(#arg_names),*));
            if *is_async {
                call = quote!(ts_aot_runtime::__ts_aot_run_or_poll(#call));
            }
            if *is_throwing {
                call = quote!(match #call {
                    Ok(__v) => __v,
                    Err(__e) => ts_aot_runtime::__ts_aot_throw(__e),
                });
            }
            arms.push(quote!(#enum_name::#variant => #call,));
            debug_arms.push(
                quote!(#enum_name::#variant => f.write_str(stringify!(#enum_name::#variant)),),
            );
        }
        for (id, variant, capture_types) in &closure_variants {
            let fields: Vec<TokenStream> = capture_types
                .iter()
                .enumerate()
                .map(|(i, ty)| {
                    let field = format_ident!("c{}", i);
                    quote!(#field: #ty)
                })
                .collect();
            let fn_ident = format_ident!("__ts_closure_{}", id);
            if capture_types.is_empty() {
                enum_variants.push(quote!(#variant));
                arms.push(quote!(#enum_name::#variant => #fn_ident(__ctx, #(#arg_names),*),));
                debug_arms.push(
                    quote!(#enum_name::#variant => f.write_str(stringify!(#enum_name::#variant)),),
                );
            } else {
                enum_variants.push(quote!(#variant { #(#fields),* }));
                let cap_bindings: Vec<TokenStream> = capture_types
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        let field = format_ident!("c{}", i);
                        let bind = format_ident!("__c{}", i);
                        quote!(#field: #bind)
                    })
                    .collect();
                let cap_names: Vec<Ident> = (0..capture_types.len())
                    .map(|i| format_ident!("__c{}", i))
                    .collect();
                arms.push(quote! {
                    #enum_name::#variant { #(#cap_bindings),* } => {
                        #fn_ident(__ctx, #(#cap_names.clone(),)* #(#arg_names),*)
                    },
                });
                debug_arms.push(quote! {
                    #enum_name::#variant { .. } => {
                        f.write_str(stringify!(#enum_name::#variant))
                    },
                });
            }
        }

        if enum_variants.is_empty() {
            let placeholder = format_ident!("__Uninhabited");
            tokens.extend(quote! {
                #[derive(Clone)]
                pub enum #enum_name<'a> {
                    #placeholder,
                    __Phantom(::core::marker::PhantomData<&'a ()>),
                }
                impl ::std::fmt::Debug for #enum_name<'_> {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        match self {
                            #enum_name::#placeholder => {
                                f.write_str(stringify!(#enum_name::#placeholder))
                            }
                            #enum_name::__Phantom(_) => f.write_str("__Phantom"),
                        }
                    }
                }
                pub fn #helper<'a>(
                    __ctx: &ts_aot_runtime::Ctx<'a>,
                    __f: &#enum_name<'a>,
                    #(#arg_names: #param_tys),*
                ) -> #ret {
                    match __f {
                        #enum_name::#placeholder => unreachable!(),
                        #enum_name::__Phantom(_) => unreachable!(),
                    }
                }
            });
            continue;
        }
        tokens.extend(quote! {
            #[derive(Clone)]
            pub enum #enum_name<'a> {
                #(#enum_variants,)*
                __Phantom(::core::marker::PhantomData<&'a ()>),
            }
            impl ::std::fmt::Debug for #enum_name<'_> {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    match self {
                        #(#debug_arms)*
                        #enum_name::__Phantom(_) => f.write_str("__Phantom"),
                    }
                }
            }
            pub fn #helper<'a>(
                __ctx: &ts_aot_runtime::Ctx<'a>,
                __f: &#enum_name<'a>,
                #(#arg_names: #param_tys),*
            ) -> #ret {
                match __f {
                    #(#arms)*
                    #enum_name::__Phantom(_) => unreachable!(),
                }
            }
        });
    }
    tokens.extend(ctx.take_env_structs());
    tokens.extend(ctx.take_closure_fns());
    tokens
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

fn tla_run_body(program: &MirProgram, async_runner: &TokenStream) -> TokenStream {
    let call = if tla_main_is_async(program) {
        quote!(#async_runner(__ts_aot_tla_main(__ctx)))
    } else {
        quote!(__ts_aot_tla_main(__ctx))
    };
    quote! {
        struct __TsAotHost;
        impl ts_aot_runtime::Host for __TsAotHost {}
        ts_aot_runtime::run_in_arena(&__TsAotHost, |__ctx| #call);
    }
}

fn emit_tla_main_entry(program: &MirProgram) -> TokenStream {
    let run = tla_run_body(program, &quote!(ts_aot_runtime::__ts_aot_runtime_run));
    quote! {
        fn main() {
            #run
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

fn emit_tla_init_entry(program: &MirProgram) -> TokenStream {
    let run = tla_run_body(program, &quote!(ts_aot_runtime::__ts_aot_run_or_poll));
    quote! {
        pub fn __ts_aot_init() {
            #run
        }
    }
}

fn emit_decl(
    decl: &MirDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    match decl {
        MirDecl::Function(f) => emit_function_with_ctx(f, ctx, emit_env, None),
        MirDecl::Struct(s) => emit_struct_with_ctx(s, ctx, emit_env),
        MirDecl::Global(g) => emit_global_with_ctx(g, ctx, emit_env),
    }
}

#[cfg(test)]
use ts_aot_core::ModuleId;

#[cfg(test)]
fn emit_function(f: &MirFunctionDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let empty_program = MirProgram::new(ModuleId::from_raw(0));
    let ctx = EmitCtx::build(&empty_program, types);
    let emit_env = EmitEnv {
        types,
        program: &empty_program,
    };
    Ok(emit_function_with_ctx(f, &ctx, &emit_env, None)?.0)
}

fn emit_function_with_ctx(
    f: &MirFunctionDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    enclosing: Option<StructId>,
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
    let asyncness = if f.effects.is_async && !body_ctx.is_generator() {
        quote!(async)
    } else {
        quote!()
    };
    let self_token = self_param_token(&f.kind, &body_ctx);
    let body = emit_body(f, ctx, emit_env, &body_ctx)?;

    let ctx_param = quote!(__ctx: &ts_aot_runtime::Ctx<'a>);
    let impl_declares_lifetime = enclosing
        .or_else(|| ctx.function_owner(f.id))
        .is_some_and(|owner| ctx.struct_needs_lifetime(owner));
    let generics = if impl_declares_lifetime {
        quote!()
    } else {
        quote!(<'a>)
    };
    let fn_tokens = quote! {
        #vis #asyncness fn #emitted_name #generics (#self_token #ctx_param, #(#params),*) -> #ret #body
    };

    let dispatch_entry = emit_dispatch_entry(f, emit_env, ctx);
    let (tokens, entries) = match dispatch_entry {
        Some((wrapper_name, entry)) => {
            let wrapper = build_dispatch_wrapper(f, &wrapper_name, &body_ctx, ctx, emit_env)?;
            (quote! { #fn_tokens #wrapper }, vec![entry])
        }
        None => (fn_tokens, Vec::new()),
    };
    Ok((tokens, entries))
}

fn emit_dispatch_entry(
    f: &MirFunctionDecl,
    emit_env: &EmitEnv,
    ctx: &EmitCtx,
) -> Option<(Ident, TokenStream)> {
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
    let is_new_source = ctx.register_dispatch_source(f.name.as_str());
    debug_assert!(
        is_new_source,
        "duplicate dispatch table key `{}`; dispatch is by name and would be ambiguous",
        f.name.as_str()
    );
    let name = if is_generated_tla_main(f, program) {
        tla_main_rust_name()
    } else {
        ident_from(&f.name)
    };
    let wrapper = ctx.dispatch_wrapper_ident(&name);
    let name_lit = Literal::string(f.name.as_str());
    let entry = quote! {
        (#name_lit, #wrapper as for<'a> fn(&ts_aot_runtime::Ctx<'a>, &[u64]) -> u64)
    };
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
        pub fn #wrapper_name<'a>(__ctx: &ts_aot_runtime::Ctx<'a>, _args: &[u64]) -> u64 {
            #(#unpack_stmts)*
            let __result: #ret_ty = #name(__ctx, #(#call_args),*);
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
        Type::Bool => {
            quote!(let #pname: #pty = #slot != 0;)
        }
        Type::I8
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
        _ => {
            return Err(BackendError::NotImplemented(
                "dispatch arg type not u64-packable",
            ));
        }
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
        _ => {
            return Err(BackendError::NotImplemented(
                "dispatch return type not u64-packable",
            ));
        }
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
    let fields: Vec<TokenStream> = s
        .fields
        .iter()
        .map(|f| emit_field(f, ctx, emit_env))
        .collect();
    let mut methods = TokenStream::new();
    let mut free_fns = TokenStream::new();
    let mut dispatch_entries: Vec<TokenStream> = Vec::new();
    for m in &s.methods {
        let is_static = matches!(m.kind, FunctionKind::Plain | FunctionKind::Generator);
        let enclosing = if is_static { None } else { Some(s.id) };
        let (m_tokens, m_entries) = emit_function_with_ctx(m, ctx, emit_env, enclosing)?;
        if is_static {
            free_fns.extend(m_tokens);
        } else {
            methods.extend(m_tokens);
        }
        dispatch_entries.extend(m_entries);
    }
    let class_id_fn = quote! {
        fn class_id() -> u32 {
            #class_id_raw
        }
    };
    let tokens = if ctx.struct_needs_lifetime(s.id) {
        quote! {
            #[derive(Clone, Debug)]
            pub struct #name<'a> {
                #(#fields,)*
                _marker: ::core::marker::PhantomData<&'a ()>,
            }

            impl ts_aot_runtime::TsClassId for #name<'_> {
                #class_id_fn
            }

            impl<'a> #name<'a> {
                #methods
            }
        }
    } else {
        quote! {
            #[derive(Clone, Debug)]
            pub struct #name {
                #(#fields,)*
            }

            impl ts_aot_runtime::TsClassId for #name {
                #class_id_fn
            }

            impl #name {
                #methods
            }
        }
    };
    Ok((quote! { #tokens #free_fns }, dispatch_entries))
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
    let ctx = EmitCtx::build(&empty_program, types);
    let emit_env = EmitEnv {
        types,
        program: &empty_program,
    };
    Ok(emit_global_with_ctx(g, &ctx, &emit_env)?.0)
}

const CONST_DEFAULT_MAX_DEPTH: usize = 64;
const CONST_DEFAULT_BUDGET: u32 = 8192;

fn emit_struct_const_default(
    id: StructId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    depth: usize,
    budget: &mut u32,
    mode: LifetimeMode,
) -> Result<TokenStream, BackendError> {
    let name = ctx.struct_ident(id);
    let fields: Vec<(FieldId, TypeId)> = emit_env
        .program
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Struct(s) if s.id == id => {
                Some(s.fields.iter().map(|f| (f.id, f.ty)).collect())
            }
            _ => None,
        })
        .ok_or(BackendError::NotImplemented(
            "default initializer: struct has no declaration",
        ))?;
    let mut inits = Vec::with_capacity(fields.len());
    for (fid, fty) in fields {
        let fname = ctx.struct_field_ident(id, fid);
        let fdef = emit_const_default(fty, ctx, emit_env, depth, budget, mode)?;
        inits.push(quote!(#fname: #fdef));
    }
    if ctx.struct_needs_lifetime(id) {
        Ok(quote!(#name {
            #(#inits,)*
            _marker: ::core::marker::PhantomData,
        }))
    } else {
        Ok(quote!(#name { #(#inits),* }))
    }
}

fn emit_const_default(
    ty: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    depth: usize,
    budget: &mut u32,
    mode: LifetimeMode,
) -> Result<TokenStream, BackendError> {
    if depth > CONST_DEFAULT_MAX_DEPTH || *budget == 0 {
        return Err(BackendError::NotImplemented(
            "default initializer: recursive or too large struct graph",
        ));
    }
    *budget -= 1;
    let EmitEnv { types, .. } = emit_env;
    match types.resolve(ty) {
        Some(Type::Bool) => Ok(quote!(false)),
        Some(
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::Symbol
            | Type::Date,
        ) => Ok(quote!(0)),
        Some(Type::F32 | Type::F64) => Ok(quote!(0.0)),
        Some(Type::String) => Ok(quote!(ts_aot_runtime::JsString::Valid(""))),
        Some(Type::Array { .. }) => match mode {
            LifetimeMode::Full => Ok(quote!(ts_aot_runtime::bumpalo::collections::Vec::new_in(
                __ctx.arena
            ))),
            LifetimeMode::Static => Err(BackendError::NotImplemented(
                "global/static initializer: an array-typed value cannot live in a `static` \
                 because the AOT array is an arena-backed `bumpalo::collections::Vec` that \
                 needs a `Ctx<'a>` to allocate; keep the array inside a function, or make the \
                 global a scalar (number, bool, or string)",
            )),
        },
        Some(Type::Optional { .. }) => Ok(quote!(::core::option::Option::None)),
        Some(Type::Struct { id }) => {
            emit_struct_const_default(*id, ctx, emit_env, depth + 1, budget, mode)
        }
        Some(Type::Named { .. }) => match types.struct_id(ty) {
            Some(id) => emit_struct_const_default(id, ctx, emit_env, depth + 1, budget, mode),
            None => Err(BackendError::NotImplemented(
                "default initializer: named type is not a known struct",
            )),
        },
        Some(Type::Error | Type::Void | Type::Null) | None => Ok(quote!(())),
        _ => Err(BackendError::NotImplemented(
            "default initializer: unsupported type",
        )),
    }
}

fn global_is_durable_string(ty: TypeId, emit_env: &EmitEnv) -> bool {
    matches!(emit_env.types.resolve(ty), Some(Type::String))
}

fn emit_global_with_ctx(
    g: &MirGlobalDecl,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<(TokenStream, Vec<TokenStream>), BackendError> {
    let name = ident_from(&g.name);
    let durable_string = global_is_durable_string(g.ty, emit_env);
    if !durable_string && ctx.type_needs_lifetime(g.ty, emit_env.types) {
        return Err(BackendError::NotImplemented(
            "global storage: this arena-backed type borrows the arena and cannot live in a static without a supported durable storage representation",
        ));
    }
    let ty = if durable_string {
        quote!(::std::string::String)
    } else {
        emit_static_type_id(g.ty, emit_env, ctx)
    };
    let vis = visibility_token(g.visibility);
    let mutability = if g.mutable { quote!(mut) } else { quote!() };
    let init = if let Some(expr) = &g.init {
        emit_const_expr(expr, emit_env.types)?
    } else if durable_string {
        quote!(::std::string::String::new())
    } else {
        let mut budget = CONST_DEFAULT_BUDGET;
        emit_const_default(g.ty, ctx, emit_env, 0, &mut budget, LifetimeMode::Static)?
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
