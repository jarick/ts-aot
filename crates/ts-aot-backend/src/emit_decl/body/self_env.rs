use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, EnvInit, SharedStorage};
use super::super::ident::ident_from;
use super::super::types::emit_type_id_with_ctx;
use super::globals::collect_global_names;
use super::return_signal_enum;
use super::runtime_call::collections::emit_weakmap_liveness_binding;
use super::stmt::emit_block_stmts;
use super::stmt_flow::{block_contains_try, collect_assigned_locals};
use crate::error::BackendError;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::{LocalId, TypeId};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirLocalDecl, MirParam};

pub(super) fn emit_env_prelude(
    body_ctx: &BodyCtx,
    emit_env: &EmitEnv,
    ctx: &EmitCtx,
) -> Option<TokenStream> {
    let env = body_ctx.own_env()?;
    let mut fields: Vec<TokenStream> = env
        .fields
        .iter()
        .map(|f| {
            let field = &f.field;
            let t = if f.dynamic {
                quote!(ts_aot_runtime::CaughtValue)
            } else {
                emit_type_id_with_ctx(f.ty, emit_env, ctx)
            };
            match f.storage {
                SharedStorage::Cell => quote!(#field: std::cell::Cell<Option<#t>>),
                SharedStorage::RefCell => quote!(#field: std::cell::RefCell<Option<#t>>),
            }
        })
        .collect();
    fields.extend(
        env.liveness_fields
            .iter()
            .map(|field| quote!(#field: std::cell::RefCell<std::rc::Rc<()>>)),
    );
    let mut inits: Vec<TokenStream> = env
        .fields
        .iter()
        .map(|f| {
            let field = &f.field;
            let init = match &f.init {
                EnvInit::Empty => quote!(None),
                EnvInit::Param(param) => quote!(Some(#param)),
                EnvInit::Receiver => quote!(Some(self.clone())),
            };
            match f.storage {
                SharedStorage::Cell => quote!(#field: std::cell::Cell::new(#init)),
                SharedStorage::RefCell => quote!(#field: std::cell::RefCell::new(#init)),
            }
        })
        .collect();
    inits.extend(
        env.liveness_fields
            .iter()
            .map(|field| quote!(#field: std::cell::RefCell::new(std::rc::Rc::new(())))),
    );
    let env_type = &env.env_type;
    let env_ident = &env.env_ident;
    ctx.push_env_struct(quote! {
        pub struct #env_type<'a> {
            #(#fields,)*
            _marker: ::core::marker::PhantomData<&'a ()>,
        }
    });
    let prelude = quote!(
        let #env_ident: std::rc::Rc<#env_type<'a>> = std::rc::Rc::new(#env_type {
            #(#inits,)*
            _marker: ::core::marker::PhantomData,
        });
    );
    Some(prelude)
}

pub(super) struct ClosureEmit<'a> {
    sig: usize,
    params: &'a [MirParam],
    captures: &'a [LocalId],
    locals: &'a [MirLocalDecl],
    body: &'a MirBlock,
    ret_ty: TypeId,
}

fn emit_closure_value(
    spec: &ClosureEmit<'_>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let closure_id = ctx.alloc_closure_id();
    let sig = spec.sig;
    let params = spec.params;
    let captures = spec.captures;
    let locals = spec.locals;
    let body = spec.body;
    let ret_ty = spec.ret_ty;
    let EmitEnv { types, .. } = emit_env;

    let mut envs: Vec<(Ident, Ident)> = Vec::new();
    for cid in captures {
        if let Some(acc) = body_ctx.shared_access_for_local(*cid)
            && !envs.iter().any(|(env, _)| *env == acc.env_ident)
        {
            envs.push((acc.env_ident.clone(), acc.env_type.clone()));
        }
    }
    let mut names: Vec<ts_aot_core::Atom> = Vec::new();
    collect_global_names(body, &mut names);
    for name in &names {
        if let Some(acc) = body_ctx.shared_access_for_name(name)
            && !envs.iter().any(|(env, _)| *env == acc.env_ident)
        {
            envs.push((acc.env_ident.clone(), acc.env_type.clone()));
        }
    }
    let capture_tys: Vec<TokenStream> = envs
        .iter()
        .map(|(_, env_type)| quote!(std::rc::Rc<#env_type<'a>>))
        .collect();
    let variant = ctx.register_closure(closure_id, sig, capture_tys.clone());

    let mut child =
        BodyCtx::for_closure(ret_ty, body_ctx.shared_name_map(), body_ctx.shared_id_map());
    child.set_module_idents(std::rc::Rc::clone(ctx.module_idents()));
    child.inherit_dynamic(body_ctx);
    child.seed_capture_types(captures, body_ctx);
    child.seed_dynamic(body, params, locals, types);
    child.seed_move_safe(body, locals);
    let mut candidates: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();
    for l in locals {
        candidates.insert(l.id);
    }
    let assigned = collect_assigned_locals(&body.stmts, types, &candidates);
    for (env_ident, _) in &envs {
        child.reserve_ident(env_ident.clone());
    }
    child.reserve_ident(format_ident!("__env_c{closure_id}"));
    for l in locals {
        let name = ident_from(&l.name);
        child.forget_inherited_name(&name);
        let mutable = l.mutable || assigned.contains(&l.id);
        child.register_local(l.id, name, l.ty, mutable);
    }
    for p in params {
        let name = ident_from(&p.name);
        child.forget_inherited_name(&name);
        let mutable = assigned.contains(&p.id);
        child.register_local(p.id, name, p.ty, mutable);
    }
    child.detect_own_env(&closure_id.to_string(), body, params, locals, types);
    child.inherit_captures(captures, body_ctx);

    let mut param_tokens: Vec<TokenStream> = Vec::new();
    for ((env_ident, _), ty) in envs.iter().zip(capture_tys.iter()) {
        param_tokens.push(quote!(#env_ident: #ty));
    }
    for p in params {
        let name = child.local_ident(p.id);
        let ty = emit_type_id_with_ctx(p.ty, emit_env, ctx);
        if assigned.contains(&p.id) {
            param_tokens.push(quote!(mut #name: #ty));
        } else {
            param_tokens.push(quote!(#name: #ty));
        }
    }
    let mut inner_stmts = emit_block_stmts(body, ctx, emit_env, &child)?;
    if let Some(prelude) = emit_env_prelude(&child, emit_env, ctx) {
        inner_stmts.insert(0, prelude);
    }
    if block_contains_try(body) {
        inner_stmts.insert(0, return_signal_enum());
    }
    if child.has_weakmap_liveness() {
        let cache = child.weakmap_liveness_per_key.borrow();
        let mut prelude: Vec<TokenStream> =
            cache.values().map(emit_weakmap_liveness_binding).collect();
        prelude.extend(inner_stmts);
        inner_stmts = prelude;
    }
    let ret_tokens = emit_type_id_with_ctx(ret_ty, emit_env, ctx);
    let fn_ident = format_ident!("__ts_closure_{}", closure_id);
    ctx.push_closure_fn(quote!(
        fn #fn_ident<'a>(
            __ctx: &ts_aot_runtime::Ctx<'a>,
            #(#param_tokens),*
        ) -> #ret_tokens {
            #(#inner_stmts)*
        }
    ));

    let enum_name = EmitCtx::fn_value_ident(sig);
    if envs.is_empty() {
        Ok(quote!(#enum_name::#variant))
    } else {
        let fields: Vec<TokenStream> = envs
            .iter()
            .enumerate()
            .map(|(i, (env_ident, _))| {
                let field = format_ident!("c{}", i);
                quote!(#field: #env_ident.clone())
            })
            .collect();
        Ok(quote!(#enum_name::#variant { #(#fields),* }))
    }
}

pub(super) fn emit_closure_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let MirExpr::Closure {
        id: _,
        params,
        captures,
        locals,
        body,
        ret_ty,
        fn_ty,
    } = expr
    else {
        unreachable!()
    };
    let EmitEnv { types, .. } = emit_env;
    if let Some(sig) = ctx.fn_value_kind(*fn_ty) {
        return emit_closure_value(
            &ClosureEmit {
                sig,
                params,
                captures,
                locals,
                body,
                ret_ty: *ret_ty,
            },
            ctx,
            emit_env,
            body_ctx,
        );
    }
    let mut child_ctx = BodyCtx::for_closure(
        *ret_ty,
        body_ctx.shared_name_map(),
        body_ctx.shared_id_map(),
    );
    child_ctx.set_module_idents(std::rc::Rc::clone(ctx.module_idents()));
    child_ctx.inherit_dynamic(body_ctx);
    child_ctx.seed_dynamic(body, params, locals, types);
    child_ctx.seed_move_safe(body, locals);
    let mut candidates: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();
    for l in locals {
        candidates.insert(l.id);
    }
    let assigned: std::collections::HashSet<LocalId> =
        collect_assigned_locals(&body.stmts, types, &candidates);
    for l in locals {
        let mutable = l.mutable || assigned.contains(&l.id);
        let name = ident_from(&l.name);
        child_ctx.register_local(l.id, name, l.ty, mutable);
    }
    for p in params {
        let mutable = assigned.contains(&p.id);
        child_ctx.register_local(p.id, ident_from(&p.name), p.ty, mutable);
    }
    child_ctx.inherit_captures(captures, body_ctx);
    child_ctx.set_narrowed_origins(body, locals);
    let param_tokens: Vec<TokenStream> = params
        .iter()
        .map(|p| {
            let name = child_ctx.local_ident(p.id);
            let ty = emit_type_id_with_ctx(p.ty, emit_env, ctx);
            if assigned.contains(&p.id) {
                quote!(mut #name: #ty)
            } else {
                quote!(#name: #ty)
            }
        })
        .collect();
    let ret_ty_tokens = emit_type_id_with_ctx(*ret_ty, emit_env, ctx);
    let mut inner_stmts = emit_block_stmts(body, ctx, emit_env, &child_ctx)?;
    if block_contains_try(body) {
        inner_stmts.insert(0, return_signal_enum());
    }
    let stmts = if child_ctx.has_weakmap_liveness() {
        let cache = child_ctx.weakmap_liveness_per_key.borrow();
        let mut prelude: Vec<TokenStream> =
            cache.values().map(emit_weakmap_liveness_binding).collect();
        prelude.extend(inner_stmts);
        prelude
    } else {
        inner_stmts
    };
    Ok(quote!(move |#(#param_tokens),*| -> #ret_ty_tokens { #(#stmts)* }))
}
