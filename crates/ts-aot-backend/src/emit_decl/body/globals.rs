use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::ident::ident_from;
use super::super::types::emit_static_type_id;
use super::super::{DurableGlobal, global_durable_kind};
use super::helpers::is_copy_type;
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{Atom, Type, TypeId};
use ts_aot_ir_mir::{MirBlock, MirDecl, MirExpr, MirPlace, MirPlaceBase, MirStmt};

fn durable_global_kind(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> Option<DurableGlobal> {
    ctx.global_ty(name)
        .and_then(|ty| global_durable_kind(ty, emit_env))
}

pub(super) fn is_durable_global(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    durable_global_kind(name, ctx, emit_env).is_some()
}

pub(super) fn is_owned_global(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    ctx.global_ty(name)
        .and_then(|ty| owned_array_ty(ty, ctx, emit_env))
        .is_some()
}

fn global_init_expr<'a>(name: &Atom, emit_env: &'a EmitEnv<'_>) -> Option<&'a MirExpr> {
    emit_env
        .program
        .declarations
        .iter()
        .find_map(|decl| match decl {
            MirDecl::Global(g) if g.name == *name => g.init.as_ref(),
            _ => None,
        })
}

fn struct_global_default(name: &Atom, ctx: &EmitCtx, emit_env: &EmitEnv) -> Option<TokenStream> {
    let ty = ctx.global_ty(name)?;
    if !ctx.is_struct_global(name) {
        return None;
    }
    struct_global_default_tokens(name, ty, global_init_expr(name, emit_env), ctx, emit_env).ok()
}

pub(crate) fn struct_global_default_tokens(
    name: &Atom,
    ty: TypeId,
    init: Option<&MirExpr>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<TokenStream, BackendError> {
    match init {
        None => struct_global_const_default(ty, ctx, emit_env),
        Some(MirExpr::Global(referenced)) => {
            if !referenced_struct_global(referenced, ty, ctx, emit_env)
                || struct_global_ref_chain_cycles(referenced, emit_env)
            {
                return Err(unrepresentable_struct_global_default(name, init));
            }
            let referenced_ty = ctx
                .global_ty(referenced)
                .ok_or_else(|| unrepresentable_struct_global_default(name, init))?;
            struct_global_default_tokens(
                referenced,
                referenced_ty,
                global_init_expr(referenced, emit_env),
                ctx,
                emit_env,
            )?;
            let access = struct_global_place_access(referenced, ctx, emit_env)
                .ok_or_else(|| unrepresentable_struct_global_default(name, init))?;
            Ok(quote!((#access).clone()))
        }
        Some(_) => Err(unrepresentable_struct_global_default(name, init)),
    }
}

fn struct_global_const_default(
    ty: TypeId,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Result<TokenStream, BackendError> {
    let mut budget = super::super::CONST_DEFAULT_BUDGET;
    super::super::emit_const_default(
        ty,
        ctx,
        emit_env,
        0,
        &mut budget,
        super::super::types::LifetimeMode::Full,
    )
}

fn referenced_struct_global(name: &Atom, ty: TypeId, ctx: &EmitCtx, emit_env: &EmitEnv) -> bool {
    if !ctx.is_struct_global(name) {
        return false;
    }
    ctx.global_ty(name)
        .and_then(|referenced| emit_env.types.struct_id(referenced))
        == emit_env.types.struct_id(ty)
}

fn struct_global_ref_chain_cycles(start: &Atom, emit_env: &EmitEnv) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut next = Some(start.clone());
    while let Some(name) = next {
        if !seen.insert(name.clone()) {
            return true;
        }
        next = match global_init_expr(&name, emit_env) {
            Some(MirExpr::Global(target)) => Some(target.clone()),
            _ => None,
        };
    }
    false
}

fn unrepresentable_struct_global_default(name: &Atom, init: Option<&MirExpr>) -> BackendError {
    let initializer = match init {
        Some(MirExpr::Null { .. }) => "`null`",
        Some(MirExpr::Int { .. } | MirExpr::Float { .. }) => "a number",
        Some(MirExpr::Bool(_)) => "a boolean",
        Some(MirExpr::Unit) => "`undefined`",
        Some(MirExpr::String { .. }) => "a string",
        Some(MirExpr::Global(_)) => "a reference that cannot provide this struct",
        Some(MirExpr::StructLiteral { .. }) => "a struct literal",
        _ => "a non-constant initializer",
    };
    BackendError::UnrepresentableStructGlobalDefault {
        name: name.as_str().to_owned(),
        initializer,
    }
}

pub(super) fn struct_global_place_access(
    name: &Atom,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
) -> Option<TokenStream> {
    let default = struct_global_default(name, ctx, emit_env)?;
    let ident = ident_from(name);
    Some(quote!((*unsafe { (*::core::ptr::addr_of_mut!(#ident)).get_or_insert_with(|| #default) })))
}

pub(crate) fn owned_array_ty(ty: TypeId, ctx: &EmitCtx, emit_env: &EmitEnv) -> Option<TokenStream> {
    if global_durable_kind(ty, emit_env).is_some() {
        return None;
    }
    let Some(Type::Array { element }) = emit_env.types.resolve(ty) else {
        return None;
    };
    if ctx.type_needs_lifetime(*element, emit_env.types) {
        return None;
    }
    let element = emit_static_type_id(*element, emit_env, ctx);
    Some(quote!(::std::vec::Vec<#element>))
}

pub(super) fn check_fn_value_init(
    init: &MirExpr,
    target_ty: TypeId,
    ctx: &EmitCtx,
) -> Result<(), BackendError> {
    let MirExpr::Global(name) = init else {
        return Ok(());
    };
    let Some(declared) = ctx.fn_value_kind(target_ty) else {
        return Ok(());
    };
    if ctx
        .fn_value_for_name(name)
        .is_some_and(|(registered, _)| registered == declared)
    {
        return Ok(());
    }
    Err(BackendError::Internal(format!(
        "`{}` cannot be stored in this function value: its own signature belongs to a different \
         function-value bucket, so the generated crate would name a variant the enum does not \
         have (the AOT target has no adapter for this function type yet)",
        name.as_str()
    )))
}

pub(super) fn emit_global_access(
    name: &Atom,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    owned: bool,
) -> TokenStream {
    if let Some(read) = body_ctx.shared_read_for_name(name) {
        return read;
    }
    if let Some((k, variant)) = ctx.fn_value_for_name(name) {
        let enum_name = EmitCtx::fn_value_ident(k);
        return quote!(#enum_name::#variant);
    }
    let ident = ident_from(name);
    if let Some(access) = struct_global_place_access(name, ctx, emit_env) {
        return if owned {
            quote!((#access).clone())
        } else {
            access
        };
    }
    let mutable = ctx.is_mutable_global(name) || is_owned_global(name, ctx, emit_env);
    let inner = if let Some(kind) = durable_global_kind(name, ctx, emit_env) {
        let base = if mutable {
            quote!((*::core::ptr::addr_of!(#ident)))
        } else {
            quote!(#ident)
        };
        match kind {
            DurableGlobal::OptionalString => quote!(
                #base.as_ref().map(|__s| ts_aot_runtime::JsString::Valid(
                    __ctx.arena.alloc_str(__s.as_str())
                ))
            ),
            DurableGlobal::String => {
                quote!(ts_aot_runtime::JsString::Valid(__ctx.arena.alloc_str(#base.as_str())))
            }
        }
    } else if is_owned_global(name, ctx, emit_env) {
        if owned {
            quote!(ts_aot_runtime::bumpalo::collections::Vec::from_iter_in(
                (*::core::ptr::addr_of!(#ident)).iter().cloned(),
                __ctx.arena
            ))
        } else {
            quote!(&*::core::ptr::addr_of!(#ident))
        }
    } else if owned
        && ctx
            .global_ty(name)
            .is_some_and(|t| !is_copy_type(emit_env.types, t))
    {
        quote!(#ident.clone())
    } else {
        quote!(#ident)
    };
    if mutable {
        quote!(unsafe { #inner })
    } else {
        quote!(#inner)
    }
}

pub(super) fn emit_global_place_base(
    name: &Atom,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> TokenStream {
    if let Some(tokens) = body_ctx.shared_place_base_for_name(name) {
        return tokens;
    }
    let ident = ident_from(name);
    if let Some(access) = struct_global_place_access(name, ctx, emit_env) {
        return access;
    }
    if is_owned_global(name, ctx, emit_env) {
        return quote!((&mut *::core::ptr::addr_of_mut!(#ident)));
    }
    quote!(#ident)
}

pub(super) fn collect_global_names(block: &MirBlock, out: &mut Vec<Atom>) {
    for stmt in &block.stmts {
        collect_global_names_stmt(stmt, out);
    }
}

fn collect_global_names_stmt(stmt: &MirStmt, out: &mut Vec<Atom>) {
    match stmt {
        MirStmt::Let { init: Some(e), .. }
        | MirStmt::LetInfer { init: e, .. }
        | MirStmt::Expr(e)
        | MirStmt::Return(Some(e)) => {
            collect_global_names_expr(e, out);
        }
        MirStmt::Let { init: None, .. }
        | MirStmt::Return(None)
        | MirStmt::Break
        | MirStmt::Continue => {}
        MirStmt::Assign { target, value } => {
            collect_global_names_place(target, out);
            collect_global_names_expr(value, out);
        }
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            collect_global_names_expr(error, out);
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            collect_global_names_expr(cond, out);
            collect_global_names(then_block, out);
            if let Some(b) = else_block {
                collect_global_names(b, out);
            }
        }
        MirStmt::While { cond, body } | MirStmt::DoWhile { body, cond } => {
            collect_global_names_expr(cond, out);
            collect_global_names(body, out);
        }
        MirStmt::ForOf { iterable, body, .. } | MirStmt::ForAwaitOf { iterable, body, .. } => {
            collect_global_names_expr(iterable, out);
            collect_global_names(body, out);
        }
        MirStmt::ForIn { object, body, .. } => {
            collect_global_names_expr(object, out);
            collect_global_names(body, out);
        }
        MirStmt::Runtime { args, .. } => {
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            collect_global_names_expr(disc, out);
            for c in cases {
                collect_global_names(&c.body, out);
            }
            if let Some(b) = default {
                collect_global_names(b, out);
            }
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            collect_global_names(body, out);
            if let Some(b) = catch {
                collect_global_names(b, out);
            }
            if let Some(b) = finally {
                collect_global_names(b, out);
            }
        }
    }
}

fn collect_global_names_place(place: &MirPlace, out: &mut Vec<Atom>) {
    match place {
        MirPlace::Local { .. } => {}
        MirPlace::Global(name) => out.push(name.clone()),
        MirPlace::Field { base, .. } => collect_global_names_place_base(base, out),
        MirPlace::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
    }
}

fn collect_global_names_place_base(base: &MirPlaceBase, out: &mut Vec<Atom>) {
    match base {
        MirPlaceBase::Local(_) => {}
        MirPlaceBase::Global(name) => out.push(name.clone()),
        MirPlaceBase::Field { base, .. } => collect_global_names_place_base(base, out),
        MirPlaceBase::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
        MirPlaceBase::Chain { base, .. } => collect_global_names_expr(base, out),
    }
}

fn collect_global_names_expr(expr: &MirExpr, out: &mut Vec<Atom>) {
    match expr {
        MirExpr::Global(name) => out.push(name.clone()),

        MirExpr::Closure { body, .. } => collect_global_names(body, out),
        MirExpr::Field { base, .. }
        | MirExpr::OptionalChain { base, .. }
        | MirExpr::ResultOk { value: base, .. }
        | MirExpr::ResultErr { error: base, .. }
        | MirExpr::Unary { expr: base, .. }
        | MirExpr::Await { expr: base, .. }
        | MirExpr::TypeOf { expr: base, .. }
        | MirExpr::Cast { expr: base, .. }
        | MirExpr::Import { source: base, .. } => collect_global_names_expr(base, out),
        MirExpr::Index { base, index, .. } => {
            collect_global_names_expr(base, out);
            collect_global_names_expr(index, out);
        }
        MirExpr::Binary { left, right, .. } => {
            collect_global_names_expr(left, out);
            collect_global_names_expr(right, out);
        }
        MirExpr::IsSome { expr, .. } | MirExpr::UnwrapOptional { expr, .. } => {
            collect_global_names_expr(expr, out);
        }
        MirExpr::Yield { expr: Some(e), .. } | MirExpr::Propagate { expr: e, .. } => {
            collect_global_names_expr(e, out);
        }
        MirExpr::Call { args, .. } => {
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_global_names_expr(callee, out);
            for a in args {
                collect_global_names_expr(a, out);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, v) in fields {
                collect_global_names_expr(v, out);
            }
        }
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. }
        | MirExpr::Yield { expr: None, .. } => {}
    }
}
