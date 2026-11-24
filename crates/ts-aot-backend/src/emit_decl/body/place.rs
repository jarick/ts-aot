use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv, SharedStorage};
use super::super::ident::ident_from;
use super::expr::emit_expr;
use super::globals::{emit_global_access, emit_global_place_base};
use super::helpers::{is_copy_type, value_ty};
use crate::error::BackendError;
use proc_macro2::TokenStream;
use quote::quote;
use ts_aot_core::{Atom, FieldId, LocalId, Type, TypeId};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirPlaceBase};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaceBorrow {
    Exclusive,
    Shared,
}

fn hoist_place_index(
    index: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
) -> Result<TokenStream, BackendError> {
    let index = emit_expr(index, ctx, emit_env, body_ctx)?;
    let ident = body_ctx.alloc_hoist_ident("__ts_place_index");
    prologue.push(quote!(let #ident = #index as usize;));
    Ok(quote!(#ident))
}

fn index_base_is_array_like(
    base: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    value_ty(base, ctx, emit_env, body_ctx)
        .and_then(|ty| emit_env.types.resolve(ty))
        .is_some_and(|ty| matches!(ty, Type::Array { .. } | Type::Tuple { .. }))
}

fn index_place_hoists(
    base: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> bool {
    if let Some(root) = root_local_id_of(base)
        && body_ctx.shared_access_for_local(root).is_some()
    {
        return false;
    }
    matches!(
        base,
        MirExpr::Local(_) | MirExpr::Field { .. } | MirExpr::Index { .. }
    ) && index_base_is_array_like(base, ctx, emit_env, body_ctx)
}

fn emit_index_place_tokens(
    base: &MirExpr,
    index: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
    borrow: PlaceBorrow,
) -> Result<TokenStream, BackendError> {
    let base_tokens = emit_place_ref_expr(base, ctx, emit_env, body_ctx, prologue, borrow)?;
    if index_place_hoists(base, ctx, emit_env, body_ctx) {
        let index = hoist_place_index(index, ctx, emit_env, body_ctx, prologue)?;
        Ok(quote!(#base_tokens[#index]))
    } else {
        let index = emit_expr(index, ctx, emit_env, body_ctx)?;
        Ok(quote!(#base_tokens[#index as usize]))
    }
}

pub(super) fn emit_place_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
    borrow: PlaceBorrow,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Global(name) => Ok(emit_global_access(name, ctx, emit_env, body_ctx, false)),
        MirExpr::Index { base, index, .. } if index_place_hoists(base, ctx, emit_env, body_ctx) => {
            emit_index_place_tokens(base, index, ctx, emit_env, body_ctx, prologue, borrow)
        }
        _ => match emit_shared_place(expr, ctx, emit_env, body_ctx, prologue, borrow)? {
            Some(place) => Ok(place),
            None => emit_expr(expr, ctx, emit_env, body_ctx),
        },
    }
}

pub(super) fn emit_place_ref_expr(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
    borrow: PlaceBorrow,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Global(name) => Ok(emit_global_place_base(name, ctx, emit_env, body_ctx)),
        MirExpr::Index { base, index, .. } if index_place_hoists(base, ctx, emit_env, body_ctx) => {
            emit_index_place_tokens(base, index, ctx, emit_env, body_ctx, prologue, borrow)
        }
        _ => match emit_shared_place(expr, ctx, emit_env, body_ctx, prologue, borrow)? {
            Some(place) => Ok(place),
            None => emit_expr(expr, ctx, emit_env, body_ctx),
        },
    }
}

fn emit_shared_place(
    expr: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
    borrow: PlaceBorrow,
) -> Result<Option<TokenStream>, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match expr {
        MirExpr::Field { base, field, .. } => {
            let checkpoint = prologue.len();
            let Some(base_place) =
                emit_shared_place_base(base, ctx, emit_env, body_ctx, prologue, borrow)?
            else {
                prologue.truncate(checkpoint);
                return Ok(None);
            };
            let Some(base_ty) = value_ty(base, ctx, emit_env, body_ctx) else {
                prologue.truncate(checkpoint);
                return Ok(None);
            };
            let Some(struct_id) = types.struct_id(base_ty) else {
                prologue.truncate(checkpoint);
                return Ok(None);
            };
            let field_ident = ctx.struct_field_ident(struct_id, *field);
            Ok(Some(match borrow {
                PlaceBorrow::Exclusive => quote!((*#base_place.#field_ident.borrow_mut())),
                PlaceBorrow::Shared => quote!((*#base_place.#field_ident.borrow())),
            }))
        }
        MirExpr::Index { base, index, .. } => {
            let checkpoint = prologue.len();
            let Some(base_place) =
                emit_shared_place_base(base, ctx, emit_env, body_ctx, prologue, borrow)?
            else {
                prologue.truncate(checkpoint);
                return Ok(None);
            };
            if !index_base_is_array_like(base, ctx, emit_env, body_ctx) {
                prologue.truncate(checkpoint);
                return Ok(None);
            }
            let index = hoist_place_index(index, ctx, emit_env, body_ctx, prologue)?;
            Ok(Some(quote!((#base_place)[#index])))
        }
        _ => Ok(None),
    }
}

fn emit_shared_place_base(
    base: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
    borrow: PlaceBorrow,
) -> Result<Option<TokenStream>, BackendError> {
    if let MirExpr::Local(id) = base
        && body_ctx
            .shared_access_for_local(*id)
            .is_some_and(|access| access.storage == SharedStorage::RefCell)
    {
        return Ok(Some(body_ctx.local_place_base(*id)));
    }
    if let MirExpr::Local(id) = base
        && value_ty(base, ctx, emit_env, body_ctx)
            .is_some_and(|ty| emit_env.types.struct_id(ty).is_some())
    {
        return Ok(Some(body_ctx.local_ref(*id)));
    }
    if let MirExpr::Local(id) = base
        && body_ctx.shared_access_for_local(*id).is_none()
        && value_ty(base, ctx, emit_env, body_ctx)
            .and_then(|ty| emit_env.types.resolve(ty))
            .is_some_and(|ty| matches!(ty, Type::Array { .. } | Type::Tuple { .. }))
    {
        return Ok(Some(body_ctx.local_ref(*id)));
    }
    if matches!(base, MirExpr::Field { .. } | MirExpr::Index { .. }) {
        return emit_shared_place(base, ctx, emit_env, body_ctx, prologue, borrow);
    }
    Ok(None)
}

pub(super) fn emit_place(
    place: &MirPlace,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match place {
        MirPlace::Local { id } => Ok(body_ctx.local_ref(*id)),
        MirPlace::Global(name) => {
            let name = ident_from(name);
            Ok(quote!(#name))
        }
        MirPlace::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, ctx, body_ctx).ok_or(
                BackendError::NotImplemented("field place: base type unresolved"),
            )?;
            let struct_id = types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented(
                    "field place: base is not a struct",
                ))?;
            let base_tokens = emit_place_base(base, ctx, emit_env, body_ctx, prologue)?;
            let base_tokens = if matches!(
                base.as_ref(),
                MirPlaceBase::Field { .. } | MirPlaceBase::Index { .. }
            ) && place_base_root_is_local(base)
            {
                let captured = body_ctx.alloc_hoist_ident("__ts_field_base");
                prologue.push(quote!(
                    let #captured = ::std::clone::Clone::clone(&(#base_tokens));
                ));
                quote!(#captured)
            } else {
                base_tokens
            };
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!((*#base_tokens.#field.borrow_mut())))
        }
        MirPlace::Index { base, index, .. } => emit_index_place_tokens(
            base,
            index,
            ctx,
            emit_env,
            body_ctx,
            prologue,
            PlaceBorrow::Exclusive,
        ),
    }
}

fn emit_place_base(
    base: &MirPlaceBase,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
    prologue: &mut Vec<TokenStream>,
) -> Result<TokenStream, BackendError> {
    let EmitEnv { types, .. } = emit_env;
    match base {
        MirPlaceBase::Local(id) => Ok(body_ctx.local_place_base(*id)),
        MirPlaceBase::Global(name) => Ok(emit_global_place_base(name, ctx, emit_env, body_ctx)),
        MirPlaceBase::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, ctx, body_ctx).ok_or(
                BackendError::NotImplemented("nested field place: base type unresolved"),
            )?;
            let struct_id = types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented(
                    "nested field place: base is not a struct",
                ))?;
            let base = emit_place_base(base, ctx, emit_env, body_ctx, prologue)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!((*#base.#field.borrow_mut())))
        }
        MirPlaceBase::Index { base, index, .. } => emit_index_place_tokens(
            base,
            index,
            ctx,
            emit_env,
            body_ctx,
            prologue,
            PlaceBorrow::Exclusive,
        ),
        MirPlaceBase::Chain { base, .. } => emit_expr(base, ctx, emit_env, body_ctx),
    }
}

fn place_base_ty(base: &MirPlaceBase, ctx: &EmitCtx, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirPlaceBase::Local(id) => body_ctx.local_ty(*id).or_else(|| body_ctx.capture_ty(*id)),
        MirPlaceBase::Global(name) => match body_ctx.shared_ty_for_name(name) {
            Some(ty) => Some(ty),
            None if ctx.is_mutable_global(name) => ctx.global_ty(name),
            None => None,
        },
        other => other.ty(),
    }
}

pub(super) fn expr_base_ty(base: &MirExpr, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirExpr::Local(id) => body_ctx.local_ty(*id).or_else(|| body_ctx.capture_ty(*id)),
        MirExpr::Global(name) => body_ctx.shared_ty_for_name(name),
        other => other.ty(),
    }
}

pub(super) fn is_non_copy_place(expr: &MirExpr, emit_env: &EmitEnv, body_ctx: &BodyCtx) -> bool {
    match expr {
        MirExpr::Local(id) => {
            if body_ctx.shared_access_for_local(*id).is_some() || body_ctx.is_move_safe(*id) {
                return false;
            }
            body_ctx
                .local_ty(*id)
                .is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        MirExpr::Field { base, .. } => {
            if is_shared_local_base(base, body_ctx) {
                return false;
            }
            expr.ty().is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        MirExpr::Index { base, index, .. } => {
            let simple_index = matches!(
                index.as_ref(),
                MirExpr::Int { .. }
                    | MirExpr::Float { .. }
                    | MirExpr::Bool(_)
                    | MirExpr::Local(_)
                    | MirExpr::Global(_)
            );
            if simple_index && is_shared_local_base(base, body_ctx) {
                return false;
            }
            expr.ty().is_some_and(|t| !is_copy_type(emit_env.types, t))
        }
        _ => false,
    }
}

fn is_shared_local_base(base: &MirExpr, body_ctx: &BodyCtx) -> bool {
    matches!(base, MirExpr::Local(id)
        if body_ctx
            .shared_access_for_local(*id)
            .is_some_and(|acc| acc.storage == SharedStorage::RefCell))
}

pub(super) fn root_local_id_of(expr: &MirExpr) -> Option<LocalId> {
    match expr {
        MirExpr::Local(id) => Some(*id),
        MirExpr::Field { base, .. } | MirExpr::Index { base, .. } => root_local_id_of(base),
        _ => None,
    }
}

pub(super) enum WeakMapKeyRoot {
    Local(LocalId),
    Name(Atom),
}

pub(super) fn weakmap_key_root(expr: &MirExpr) -> Option<WeakMapKeyRoot> {
    match expr {
        MirExpr::Local(id) => Some(WeakMapKeyRoot::Local(*id)),
        MirExpr::Global(name) => Some(WeakMapKeyRoot::Name(name.clone())),
        MirExpr::Field { base, .. } | MirExpr::Index { base, .. } => weakmap_key_root(base),
        _ => None,
    }
}

fn place_base_root_is_local(base: &MirPlaceBase) -> bool {
    match base {
        MirPlaceBase::Local(_) => true,
        MirPlaceBase::Field { base, .. } => place_base_root_is_local(base),
        MirPlaceBase::Index { base, .. } => root_local_id_of(base).is_some(),
        MirPlaceBase::Global(_) | MirPlaceBase::Chain { .. } => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaceSegment {
    Field(FieldId),
    Dynamic,
}

pub(super) fn place_root_and_path(expr: &MirExpr) -> Option<(LocalId, Vec<PlaceSegment>)> {
    match expr {
        MirExpr::Local(id) => Some((*id, Vec::new())),
        MirExpr::Field { base, field, .. } => {
            let (root, mut path) = place_root_and_path(base)?;
            path.push(PlaceSegment::Field(*field));
            Some((root, path))
        }
        MirExpr::Index { base, .. } => {
            let (root, mut path) = place_root_and_path(base)?;
            path.push(PlaceSegment::Dynamic);
            Some((root, path))
        }
        _ => None,
    }
}

pub(super) fn place_paths_overlap(
    left: Option<&(LocalId, Vec<PlaceSegment>)>,
    right: Option<&(LocalId, Vec<PlaceSegment>)>,
) -> bool {
    let (Some((left_root, left_path)), Some((right_root, right_path))) = (left, right) else {
        return false;
    };
    if left_root != right_root {
        return false;
    }
    let shared = left_path.len().min(right_path.len());
    left_path[..shared]
        .iter()
        .zip(right_path[..shared].iter())
        .all(|(left, right)| match (left, right) {
            (PlaceSegment::Field(left), PlaceSegment::Field(right)) => left == right,
            _ => true,
        })
}
