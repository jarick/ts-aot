use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::{LocalId, Type, TypeId, TypeTable};

use ts_aot_ir_mir::{
    BinaryOp, ConstValue, MirBlock, MirExpr, MirFunctionDecl, MirPlace, MirPlaceBase, MirStmt,
    RuntimeOp, UnaryOp,
};

use super::ctx::{BodyCtx, EmitCtx};
use super::ident::ident_from;
use super::literals::emit_whole_number_literal;
use super::runtime_op::runtime_op_ident;
use super::types::emit_type_id_with_ctx;
use crate::error::BackendError;

pub(super) fn emit_body(
    f: &MirFunctionDecl,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if f.body.block.is_empty() {
        return Ok(quote!({ unimplemented!() }));
    }
    let stmts = emit_block_stmts(&f.body.block, ctx, body_ctx)?;
    if body_ctx.is_generator() {
        let co = body_ctx.gen_co().ok_or_else(|| {
            BackendError::Internal(
                "is_generator() returned true but gen_co identifier is unset (BodyCtx invariant violation)"
                    .to_string(),
            )
        })?;
        Ok(quote!({
            ts_aot_runtime::__ts_aot_generator_new(|#co| async move { #(#stmts)* })
        }))
    } else {
        Ok(quote!({ #(#stmts)* }))
    }
}

fn collect_assigned_locals(
    stmts: &[ts_aot_ir_mir::MirStmt],
    types: &TypeTable,
    candidates: &std::collections::HashSet<LocalId>,
) -> std::collections::HashSet<LocalId> {
    super::ctx::collect_assigned_locals(stmts, types, candidates)
}

fn emit_block_stmts(
    block: &MirBlock,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    block
        .stmts
        .iter()
        .map(|stmt| emit_stmt(stmt, ctx, body_ctx))
        .collect()
}

fn emit_if_stmt(
    cond: &MirExpr,
    then_block: &MirBlock,
    else_block: Option<&MirBlock>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let cond = emit_expr(cond, ctx, body_ctx)?;
    let then_stmts = emit_block_stmts(then_block, ctx, body_ctx)?;
    if let Some(else_block) = else_block {
        let else_stmts = emit_block_stmts(else_block, ctx, body_ctx)?;
        Ok(quote!(if #cond { #(#then_stmts)* } else { #(#else_stmts)* }))
    } else {
        Ok(quote!(if #cond { #(#then_stmts)* }))
    }
}

fn emit_runtime_stmt(
    op: RuntimeOp,
    args: &[MirExpr],
    dest: Option<LocalId>,
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let call = emit_runtime_call(op, args, ty, target_ty, ctx, body_ctx)?;
    if let Some(dest) = dest {
        let dest = body_ctx.local_ident(dest);
        let ty = emit_type_id_with_ctx(ty, ctx);
        let mutability = quote!();
        Ok(quote!(let #mutability #dest: #ty = #call;))
    } else {
        Ok(quote!(#call;))
    }
}

fn emit_return_stmt(
    slot: Option<&MirExpr>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    if body_ctx.try_label().is_some() {
        let slot_ident = body_ctx
            .return_slot()
            .expect("emit_return_stmt inside try requires return_slot to be set by emit_try");
        let value_ts = if body_ctx.is_generator() {
            if let Some(expr) = slot {
                let expr = emit_expr(expr, ctx, body_ctx)?;
                quote!(Some(#expr))
            } else {
                quote!(None)
            }
        } else if let Some(expr) = slot {
            let expr = emit_expr(expr, ctx, body_ctx)?;
            quote!(#expr)
        } else {
            quote!(())
        };
        Ok(quote!(#slot_ident = Some(#value_ts); return Ok(__ReturnSignal::Break);))
    } else if body_ctx.is_generator() {
        if let Some(expr) = slot {
            let expr = emit_expr(expr, ctx, body_ctx)?;
            Ok(quote!(return Some(#expr);))
        } else {
            Ok(quote!(return None;))
        }
    } else if let Some(expr) = slot {
        let expr = emit_expr(expr, ctx, body_ctx)?;
        Ok(quote!(return #expr;))
    } else {
        Ok(quote!(return;))
    }
}

fn emit_do_while(
    body: &MirBlock,
    cond: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let label = format_ident!("__do_while_{}", body.stmts.len());
    let prev_label = body_ctx.continue_label();
    body_ctx.set_continue_label(Some(label.clone()));
    let body_stmts = emit_block_stmts(body, ctx, body_ctx);
    body_ctx.set_continue_label(prev_label);
    let body_stmts = body_stmts?;
    let cond = emit_expr(cond, ctx, body_ctx)?;
    Ok(quote!(#label: loop { #(#body_stmts)* if !(#cond) { break #label; } }))
}

fn emit_stmt(
    stmt: &MirStmt,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match stmt {
        MirStmt::Let {
            local,
            ty,
            init,
            mutable,
        } => {
            let name = body_ctx.local_ident(*local);
            let ty = emit_type_id_with_ctx(*ty, ctx);
            let mutability = if *mutable || body_ctx.local_mut(*local) {
                quote!(mut)
            } else {
                quote!()
            };
            if let Some(init) = init {
                let init = emit_expr(init, ctx, body_ctx)?;
                Ok(quote!(let #mutability #name: #ty = #init;))
            } else {
                Ok(quote!(let #mutability #name: #ty;))
            }
        }
        MirStmt::Assign { target, value } => {
            if let Some(chain_assign) = emit_optional_chain_assign(target, value, ctx, body_ctx)? {
                return Ok(chain_assign);
            }
            let target = emit_place(target, ctx, body_ctx)?;
            let value = emit_expr(value, ctx, body_ctx)?;
            Ok(quote!(#target = #value;))
        }
        MirStmt::Expr(expr) => {
            if matches!(expr, MirExpr::Local(_)) {
                return Ok(quote!());
            }
            let expr = emit_expr(expr, ctx, body_ctx)?;
            Ok(quote!(#expr;))
        }
        MirStmt::Return(slot) => emit_return_stmt(slot.as_ref(), ctx, body_ctx),
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            let error = emit_expr(error, ctx, body_ctx)?;
            if body_ctx.in_try() {
                Ok(quote!(__ts_aot_throw(#error);))
            } else {
                Ok(quote!(return Err(#error);))
            }
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => emit_if_stmt(cond, then_block, else_block.as_ref(), ctx, body_ctx),
        MirStmt::While { cond, body } => {
            let cond = emit_expr(cond, ctx, body_ctx)?;
            let body_stmts = emit_block_stmts(body, ctx, body_ctx)?;
            Ok(quote!(while #cond { #(#body_stmts)* }))
        }
        MirStmt::ForOf {
            item,
            iterable,
            iter_ty,
            body,
        }
        | MirStmt::ForAwaitOf {
            item,
            iterable,
            iter_ty,
            body,
        } => {
            let item_ident = body_ctx.local_ident(*item);
            let item = if body_ctx.local_mut(*item) {
                quote!(mut #item_ident)
            } else {
                quote!(#item_ident)
            };
            let iterable = emit_expr(iterable, ctx, body_ctx)?;
            let body_stmts = emit_block_stmts(body, ctx, body_ctx)?;
            let needs_mut_ref = matches!(
                ctx.types.resolve(*iter_ty),
                Some(ts_aot_core::Type::Generator { .. })
            );
            if needs_mut_ref {
                Ok(quote!(for #item in &mut (#iterable) { #(#body_stmts)* }))
            } else {
                Ok(quote!(for #item in #iterable { #(#body_stmts)* }))
            }
        }
        MirStmt::ForIn { key, object, body } => {
            let key = body_ctx.local_ident(*key);
            let object = emit_expr(object, ctx, body_ctx)?;
            let body_stmts = emit_block_stmts(body, ctx, body_ctx)?;
            Ok(quote!(for #key in #object { #(#body_stmts)* }))
        }
        MirStmt::Break => Ok(quote!(break;)),
        MirStmt::Continue => {
            if let Some(label) = body_ctx.continue_label() {
                Ok(quote!(continue #label;))
            } else {
                Ok(quote!(continue;))
            }
        }
        MirStmt::Runtime {
            op,
            args,
            dest,
            ty,
            target_ty,
        } => emit_runtime_stmt(*op, args, *dest, *ty, *target_ty, ctx, body_ctx),
        MirStmt::DoWhile { body, cond } => emit_do_while(body, cond, ctx, body_ctx),
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => emit_switch(disc, cases, default.as_ref(), ctx, body_ctx),
        MirStmt::Try {
            body,
            catch_param,
            catch,
            finally,
        } => emit_try(
            body,
            *catch_param,
            catch.as_ref(),
            finally.as_ref(),
            ctx,
            body_ctx,
        ),
    }
}

fn emit_place(
    place: &MirPlace,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match place {
        MirPlace::Local { id } => Ok(body_ctx.local_ref(*id)),
        MirPlace::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented)?;
            let struct_id = ctx
                .types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented)?;
            let base = emit_place_base(base, ctx, body_ctx)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!(#base.#field))
        }
        MirPlace::Index { base, index, .. } => {
            let base = emit_expr(base, ctx, body_ctx)?;
            let index = emit_expr(index, ctx, body_ctx)?;
            Ok(quote!(#base[#index]))
        }
    }
}

fn emit_place_base(
    base: &MirPlaceBase,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match base {
        MirPlaceBase::Local(id) => Ok(body_ctx.local_ref(*id)),
        MirPlaceBase::Field { base, field, .. } => {
            let base_ty = place_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented)?;
            let struct_id = ctx
                .types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented)?;
            let base = emit_place_base(base, ctx, body_ctx)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            Ok(quote!(#base.#field))
        }
        MirPlaceBase::Index { base, index, .. } => {
            let base = emit_expr(base, ctx, body_ctx)?;
            let index = emit_expr(index, ctx, body_ctx)?;
            Ok(quote!(#base[#index]))
        }
        MirPlaceBase::Chain { base, .. } => emit_expr(base, ctx, body_ctx),
    }
}

fn place_base_ty(base: &MirPlaceBase, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirPlaceBase::Local(id) => body_ctx.local_ty(*id),
        other => other.ty(),
    }
}

fn expr_base_ty(base: &MirExpr, body_ctx: &BodyCtx) -> Option<TypeId> {
    match base {
        MirExpr::Local(id) => body_ctx.local_ty(*id),
        other => other.ty(),
    }
}

fn is_display_supported_reason(reason_ty: Option<&Type>) -> bool {
    matches!(
        reason_ty,
        Some(Type::I32 | Type::I64 | Type::F64 | Type::String | Type::Bool)
    )
}

fn optional_chain_map_arm(
    base: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let MirExpr::OptionalChain { base: inner, ty } = base else {
        return Ok(None);
    };
    let Some(resolved) = ctx.types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, body_ctx)?;
    Ok(Some(quote!(#inner_tokens.as_ref())))
}

fn optional_call_map_arm(
    callee: &MirExpr,
    args: &[MirExpr],
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let MirExpr::OptionalChain { base: inner, ty } = callee else {
        return Ok(None);
    };
    let Some(resolved) = ctx.types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, body_ctx)?;
    let args_tokens = emit_exprs(args, ctx, body_ctx)?;
    Ok(Some(
        quote!(#inner_tokens.as_ref().map(|f| f(#(#args_tokens),*))),
    ))
}

fn emit_optional_chain_assign(
    target: &MirPlace,
    value: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<Option<TokenStream>, BackendError> {
    let MirPlace::Field { base, field, .. } = target else {
        return Ok(None);
    };
    let MirPlaceBase::Chain {
        base: chain_base, ..
    } = base.as_ref()
    else {
        return Ok(None);
    };
    let MirExpr::OptionalChain { base: inner, ty } = chain_base.as_ref() else {
        return Ok(None);
    };
    let Some(resolved) = ctx.types.resolve(*ty) else {
        return Ok(None);
    };
    if !matches!(resolved, ts_aot_core::Type::Optional { .. }) {
        return Ok(None);
    }
    let inner_tokens = emit_expr(inner, ctx, body_ctx)?;
    let inner_ty = match ctx.types.resolve(*ty) {
        Some(ts_aot_core::Type::Optional { inner }) => *inner,
        _ => return Err(BackendError::NotImplemented),
    };
    let field_ident = match ctx.types.resolve(inner_ty) {
        Some(_) => {
            let struct_id = ctx
                .types
                .struct_id(inner_ty)
                .ok_or(BackendError::NotImplemented)?;
            ctx.struct_field_ident(struct_id, *field)
        }
        None => return Err(BackendError::NotImplemented),
    };
    let value = emit_expr(value, ctx, body_ctx)?;
    Ok(Some(quote! {
        if #inner_tokens.is_some() {
            #inner_tokens.as_mut().unwrap().#field_ident = #value;
        }
    }))
}

fn emit_expr(
    expr: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Unit | MirExpr::Null { .. } => Ok(quote!(())),
        MirExpr::Bool(value) => Ok(quote!(#value)),
        MirExpr::Int { value, .. } => Ok(emit_whole_number_literal(*value)),
        MirExpr::Float { value, .. } => Ok(emit_float(*value)),
        MirExpr::String { id, .. } => {
            let literal = Literal::string(id.as_str());
            Ok(quote!(ts_aot_runtime::JsString::from(#literal)))
        }
        MirExpr::Local(id) => Ok(body_ctx.local_ref(*id)),
        MirExpr::Global(name) => {
            let name = ident_from(name);
            Ok(quote!(#name))
        }
        MirExpr::Field { base, field, .. } => {
            if let Some(map) = optional_chain_map_arm(base, ctx, body_ctx)? {
                let base_ty = expr_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented)?;
                let inner_ty = match ctx.types.resolve(base_ty) {
                    Some(ts_aot_core::Type::Optional { inner }) => *inner,
                    _ => return Err(BackendError::NotImplemented),
                };
                let struct_id = ctx
                    .types
                    .struct_id(inner_ty)
                    .ok_or(BackendError::NotImplemented)?;
                let field = ctx.struct_field_ident(struct_id, *field);
                return Ok(quote!(#map.map(|o| o.#field)));
            }
            let base_ty = expr_base_ty(base, body_ctx).ok_or(BackendError::NotImplemented)?;
            let struct_id = ctx
                .types
                .struct_id(base_ty)
                .ok_or(BackendError::NotImplemented)?;
            let field = ctx.struct_field_ident(struct_id, *field);
            let base = emit_expr(base, ctx, body_ctx)?;
            Ok(quote!(#base.#field))
        }
        MirExpr::Index { base, index, .. } => {
            let index = emit_expr(index, ctx, body_ctx)?;
            if let Some(map) = optional_chain_map_arm(base, ctx, body_ctx)? {
                return Ok(quote!(#map.map(|o| o[#index])));
            }
            let base = emit_expr(base, ctx, body_ctx)?;
            Ok(quote!(#base[#index]))
        }
        MirExpr::Call { callee, args, .. } => {
            let callee = ctx.function_ident(*callee);
            let args = emit_exprs(args, ctx, body_ctx)?;
            Ok(quote!(#callee(#(#args),*)))
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            if let Some(map) = optional_call_map_arm(callee, args, ctx, body_ctx)? {
                return Ok(map);
            }
            let callee = emit_expr(callee, ctx, body_ctx)?;
            let args = emit_exprs(args, ctx, body_ctx)?;
            Ok(quote!(#callee(#(#args),*)))
        }
        MirExpr::StructLiteral {
            struct_id, fields, ..
        } => {
            let name = ctx.struct_ident(*struct_id);
            let fields = fields
                .iter()
                .map(|(field_id, value)| {
                    let field = ctx.struct_field_ident(*struct_id, *field_id);
                    let value = emit_expr(value, ctx, body_ctx)?;
                    Ok(quote!(#field: #value))
                })
                .collect::<Result<Vec<_>, BackendError>>()?;
            Ok(quote!(#name { #(#fields),* }))
        }
        MirExpr::ResultOk { value, .. } => {
            let value = emit_expr(value, ctx, body_ctx)?;
            Ok(quote!(Ok(#value)))
        }
        MirExpr::ResultErr { error, .. } => {
            let error = emit_expr(error, ctx, body_ctx)?;
            Ok(quote!(Err(#error)))
        }
        MirExpr::Binary {
            op, left, right, ..
        } => emit_binary_expr(*op, left, right, ctx, body_ctx),
        MirExpr::Unary { op, expr, .. } => emit_unary_expr(*op, expr, ctx, body_ctx),
        MirExpr::Await { expr, ty, .. } => {
            let inner = emit_expr(expr, ctx, body_ctx)?;
            let needs_helper = matches!(expr.as_ref(), MirExpr::Import { .. })
                || expr_base_ty(expr, body_ctx)
                    .and_then(|ty_id| ctx.types.resolve(ty_id))
                    .is_some_and(|ty| matches!(ty, Type::Promise { .. }));
            if needs_helper {
                let ok_ty = emit_type_id_with_ctx(*ty, ctx);
                Ok(quote!(ts_aot_runtime::__ts_aot_await_value::<#ok_ty>(&#inner)))
            } else {
                Ok(inner)
            }
        }
        MirExpr::OptionalChain { base, .. } => emit_expr(base, ctx, body_ctx),
        MirExpr::TypeOf { expr, .. } => emit_typeof(expr, ctx, body_ctx),
        MirExpr::Cast { expr, ty } => {
            let inner = emit_expr(expr, ctx, body_ctx)?;
            if !is_numeric_primitive_target(ctx.types.resolve(*ty)) {
                return Err(BackendError::Internal(format!(
                    "MirExpr::Cast target TypeId({}) is not a numeric primitive; \
                     Rust's `as` cast cannot lower it. \
                     Only I8/I16/I32/I64/U8/U16/U32/U64/F32/F64 are supported as cast targets.",
                    ty.raw()
                )));
            }
            let cast_ty = emit_type_id_with_ctx(*ty, ctx);
            Ok(quote!((#inner as #cast_ty)))
        }
        MirExpr::TemplateStringsArray { cooked, .. } => {
            let cooked_lits: Vec<TokenStream> = cooked
                .iter()
                .map(|p| {
                    let lit = Literal::string(p.as_str());
                    quote!(ts_aot_runtime::JsString::from(#lit))
                })
                .collect();
            Ok(quote!(vec![#(#cooked_lits),*]))
        }
        MirExpr::RegExp { pattern, flags, .. } => {
            let pattern_lit = Literal::string(pattern);
            let flags_lit = Literal::string(flags);
            Ok(quote!(ts_aot_runtime::__ts_aot_regex_new(#pattern_lit, #flags_lit)))
        }
        MirExpr::BigInt { value, .. } => {
            let value_lit = Literal::string(value);
            Ok(quote!(ts_aot_runtime::__ts_aot_bigint_new(#value_lit)))
        }
        MirExpr::Import { source, ty } => {
            let source = emit_expr(source, ctx, body_ctx)?;
            let payload_ty = emit_type_id_with_ctx(*ty, ctx);
            Ok(
                quote!(ts_aot_runtime::__ts_aot_dynamic_import::<#payload_ty>(
                    &#source.to_string_lossy()
                )),
            )
        }
        MirExpr::Yield { expr, .. } => {
            let co = body_ctx.gen_co().ok_or_else(|| {
                BackendError::Internal(
                    "MirExpr::Yield reached for non-generator body \
                     (HIR->MIR contract violation: lower_generators must \
                     transform every yield into a generator body)"
                        .to_string(),
                )
            })?;
            let value = if let Some(inner) = expr {
                emit_expr(inner, ctx, body_ctx)?
            } else {
                quote!(())
            };
            Ok(quote!(#co.yield_(#value).await))
        }
        MirExpr::Closure {
            params,
            captures,
            locals,
            body,
            ret_ty,
            ..
        } => {
            if !captures.is_empty() {
                return Err(BackendError::NotImplemented);
            }
            let mut child_ctx = BodyCtx::for_closure(*ret_ty);
            let mut candidates: std::collections::HashSet<LocalId> =
                params.iter().map(|p| p.id).collect();
            for l in locals {
                candidates.insert(l.id);
            }
            let assigned: std::collections::HashSet<LocalId> =
                collect_assigned_locals(&body.stmts, ctx.types, &candidates);
            for l in locals {
                let mutable = l.mutable || assigned.contains(&l.id);
                let name = ident_from(&l.name);
                child_ctx.register_local(l.id, name, l.ty, mutable);
            }
            for p in params {
                let mutable = assigned.contains(&p.id);
                child_ctx.register_local(p.id, ident_from(&p.name), p.ty, mutable);
            }
            let param_tokens: Vec<TokenStream> = params
                .iter()
                .map(|p| {
                    let name = child_ctx.local_ident(p.id);
                    let ty = emit_type_id_with_ctx(p.ty, ctx);
                    if assigned.contains(&p.id) {
                        quote!(mut #name: #ty)
                    } else {
                        quote!(#name: #ty)
                    }
                })
                .collect();
            let ret_ty_tokens = emit_type_id_with_ctx(*ret_ty, ctx);
            let inner_stmts = emit_block_stmts(body, ctx, &child_ctx)?;
            Ok(quote!(move |#(#param_tokens),*| -> #ret_ty_tokens { #(#inner_stmts)* }))
        }
    }
}

fn emit_exprs(
    exprs: &[MirExpr],
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    exprs
        .iter()
        .map(|expr| emit_expr(expr, ctx, body_ctx))
        .collect()
}

fn emit_binary_expr(
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let left = emit_expr(left, ctx, body_ctx)?;
    let right = emit_expr(right, ctx, body_ctx)?;
    Ok(match op {
        BinaryOp::Add => quote!((#left + #right)),
        BinaryOp::Sub => quote!((#left - #right)),
        BinaryOp::Mul => quote!((#left * #right)),
        BinaryOp::Div => quote!((#left / #right)),
        BinaryOp::Mod => quote!((#left % #right)),
        BinaryOp::Eq => quote!((#left == #right)),
        BinaryOp::Ne => quote!((#left != #right)),
        BinaryOp::Lt => quote!((#left < #right)),
        BinaryOp::Le => quote!((#left <= #right)),
        BinaryOp::Gt => quote!((#left > #right)),
        BinaryOp::Ge => quote!((#left >= #right)),
        BinaryOp::And => quote!((#left && #right)),
        BinaryOp::Or => quote!((#left || #right)),
        BinaryOp::BitAnd => quote!((#left & #right)),
        BinaryOp::BitOr => quote!((#left | #right)),
        BinaryOp::BitXor => quote!((#left ^ #right)),
        BinaryOp::Shl => quote!((#left << #right)),
        BinaryOp::Shr => quote!((#left >> #right)),
    })
}

pub(super) fn emit_float(value: f64) -> TokenStream {
    if value.is_nan() {
        quote!(f64::NAN)
    } else if value.is_infinite() && value.is_sign_positive() {
        quote!(f64::INFINITY)
    } else if value.is_infinite() {
        quote!(f64::NEG_INFINITY)
    } else {
        let literal = Literal::f64_unsuffixed(value);
        quote!(#literal)
    }
}

fn emit_typeof(
    expr: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Unit => Ok(quote!(String::from(__ts_aot_typeof_unit()))),
        MirExpr::Null { .. } => Ok(quote!(String::from(__ts_aot_typeof_null()))),
        _ => {
            let inner = emit_expr(expr, ctx, body_ctx)?;
            Ok(quote!(String::from(__ts_aot_typeof(&#inner))))
        }
    }
}

fn emit_unary_expr(
    op: UnaryOp,
    expr: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let expr = emit_expr(expr, ctx, body_ctx)?;
    Ok(match op {
        UnaryOp::Neg => quote!((-#expr)),
        UnaryOp::Not | UnaryOp::BitNot => quote!((!#expr)),
    })
}

fn is_numeric_primitive_target(ty: Option<&Type>) -> bool {
    matches!(
        ty,
        Some(
            Type::I8
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
    )
}

fn emit_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ty: TypeId,
    target_ty: Option<TypeId>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match op {
        RuntimeOp::TypedArrayNew => {
            let length = emit_expr(&args[0], ctx, body_ctx)?;
            let kind_id = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_typed_array_new(
                (#length) as i64,
                (#kind_id) as i64
            )))
        }
        RuntimeOp::ArrayGetOrDefault => {
            let arr = emit_expr(&args[0], ctx, body_ctx)?;
            let idx = emit_expr(&args[1], ctx, body_ctx)?;
            let element_ty = emit_type_id_with_ctx(ty, ctx);
            Ok(quote!(__ts_aot_array_get_or_default::<#element_ty>(
                &#arr,
                #idx
            )))
        }
        RuntimeOp::ArrayCreate => {
            emit_array_op_with_element_type("__ts_aot_array_create", args, ty, ctx, body_ctx)
        }
        RuntimeOp::ArrayConcat => {
            emit_array_op_with_element_type("__ts_aot_array_concat", args, ty, ctx, body_ctx)
        }
        RuntimeOp::ArrayHole => {
            let element_ty_id = match ctx.types.resolve(ty) {
                Some(Type::Array { element }) => *element,
                _ => ty,
            };
            let element_ty = emit_type_id_with_ctx(element_ty_id, ctx);
            Ok(quote!(__ts_aot_array_hole::<#element_ty>()))
        }
        RuntimeOp::GeneratorNext => {
            let owner = emit_expr(&args[0], ctx, body_ctx)?;
            Ok(quote!((#owner).next()))
        }
        RuntimeOp::OpInstanceof => {
            let value = emit_expr(&args[0], ctx, body_ctx)?;
            let target_type_id: u32 = match args.get(2) {
                Some(MirExpr::Int { value, .. }) => (*value).try_into().unwrap_or(0),
                _ => 0,
            };
            Ok(quote!(__ts_aot_op_instanceof(&#value, #target_type_id)))
        }
        RuntimeOp::TypeOf => emit_typeof(&args[0], ctx, body_ctx),
        RuntimeOp::ArrayPush => {
            let arr = emit_expr(&args[0], ctx, body_ctx)?;
            let item = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_array_push(&mut #arr, #item)))
        }
        RuntimeOp::ArraySet => {
            let arr = emit_expr(&args[0], ctx, body_ctx)?;
            let idx = emit_expr(&args[1], ctx, body_ctx)?;
            let value = emit_expr(&args[2], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_array_set(&mut #arr, #idx, #value)))
        }
        RuntimeOp::MathMax | RuntimeOp::MathMin => {
            let name = runtime_op_ident(op)?;
            let args = emit_exprs(args, ctx, body_ctx)?;
            Ok(quote!(#name(&[#(#args),*])))
        }
        RuntimeOp::StringConcat
        | RuntimeOp::StringEquals
        | RuntimeOp::StringLen
        | RuntimeOp::StringIndexOf
        | RuntimeOp::StringCharAt => {
            let string_arg_indices = string_op_string_arg_indices(op);
            let name = runtime_op_ident(op)?;
            let emitted: Vec<TokenStream> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if string_arg_indices.contains(&i) {
                        emit_js_string_arg(a, ctx, body_ctx)
                    } else {
                        emit_expr(a, ctx, body_ctx)
                    }
                })
                .collect::<Result<Vec<_>, BackendError>>()?;
            Ok(quote!(#name(#(#emitted),*)))
        }
        RuntimeOp::MapSet => {
            let name = runtime_op_ident(op)?;
            let map_expr = emit_expr(&args[0], ctx, body_ctx)?;
            let key_expr = emit_js_string_owned(&args[1], ctx, body_ctx)?;
            let value_expr = emit_js_string_owned(&args[2], ctx, body_ctx)?;
            Ok(quote!(#name(&mut #map_expr, #key_expr, #value_expr)))
        }
        RuntimeOp::MapGet => {
            let name = runtime_op_ident(op)?;
            let map_expr = emit_expr(&args[0], ctx, body_ctx)?;
            let key_expr = emit_js_string_arg(&args[1], ctx, body_ctx)?;
            Ok(quote!(#name(&#map_expr, #key_expr)))
        }
        RuntimeOp::ArrayFromString => {
            let name = runtime_op_ident(op)?;
            let source_expr = emit_js_string_arg(&args[0], ctx, body_ctx)?;
            Ok(quote!(#name(#source_expr)))
        }
        RuntimeOp::HostConsoleLog | RuntimeOp::SymbolFor => {
            let name = runtime_op_ident(op)?;
            let arg = emit_js_string_arg(&args[0], ctx, body_ctx)?;
            Ok(quote!(#name(#arg)))
        }
        RuntimeOp::JsonParse | RuntimeOp::JsonStringify => {
            let name = runtime_op_ident(op)?;
            let target = target_ty.unwrap_or(ty);
            let ty_tokens = emit_type_id_with_ctx(target, ctx);
            if op == RuntimeOp::JsonParse {
                let arg = emit_js_string_arg(&args[0], ctx, body_ctx)?;
                Ok(quote!(#name::<#ty_tokens>(#arg)))
            } else {
                let arg = emit_expr(&args[0], ctx, body_ctx)?;
                Ok(quote!(#name::<#ty_tokens>(&#arg)))
            }
        }
        RuntimeOp::SymbolNew => {
            let name = runtime_op_ident(op)?;
            if args.is_empty() || matches!(&args[0], MirExpr::Unit) {
                Ok(quote!(#name()))
            } else {
                let desc_expr: TokenStream = match &args[0] {
                    MirExpr::Null { .. } => quote!(ts_aot_runtime::JsString::from("null")),
                    _ => emit_js_string_arg(&args[0], ctx, body_ctx)?,
                };
                Ok(quote!(__ts_aot_symbol_new_desc(&#desc_expr)))
            }
        }
        RuntimeOp::PromiseResolveStatic | RuntimeOp::PromiseRejectStatic => {
            let name = runtime_op_ident(op)?;
            let target = target_ty.unwrap_or(ty);
            let ty_tokens = emit_type_id_with_ctx(target, ctx);
            if matches!(op, RuntimeOp::PromiseRejectStatic) {
                let reason = match &args[0] {
                    MirExpr::String { id, .. } => {
                        let lit = Literal::string(id.as_str());
                        quote!(ts_aot_runtime::JsString::from(#lit).to_string_lossy()
                            .into_owned())
                    }
                    _ if matches!(
                        expr_base_ty(&args[0], body_ctx).and_then(|ty| ctx.types.resolve(ty)),
                        Some(Type::String)
                    ) =>
                    {
                        let js = emit_js_string_owned(&args[0], ctx, body_ctx)?;
                        quote!(#js.to_string_lossy().into_owned())
                    }
                    _ => {
                        let reason_ty =
                            expr_base_ty(&args[0], body_ctx).and_then(|ty| ctx.types.resolve(ty));
                        if !is_display_supported_reason(reason_ty) {
                            return Err(BackendError::NotImplemented);
                        }
                        let inner = emit_expr(&args[0], ctx, body_ctx)?;
                        quote!(#inner.to_string())
                    }
                };
                Ok(quote!(#name::<#ty_tokens>(#reason)))
            } else {
                let value = emit_expr(&args[0], ctx, body_ctx)?;
                Ok(quote!(#name::<#ty_tokens>(#value)))
            }
        }
        RuntimeOp::PromiseThenInstance
        | RuntimeOp::PromiseCatchInstance
        | RuntimeOp::PromiseFinallyInstance => {
            let name = runtime_op_ident(op)?;
            let promise = emit_expr(&args[0], ctx, body_ctx)?;
            let handler = emit_expr(&args[1], ctx, body_ctx)?;
            let input_ty_tokens = match target_ty {
                Some(t) => emit_type_id_with_ctx(t, ctx),
                None => emit_type_id_with_ctx(ty, ctx),
            };
            Ok(quote!(#name::<#input_ty_tokens, _>(&#promise, #handler)))
        }
        _ => {
            let name = runtime_op_ident(op)?;
            let args = emit_exprs(args, ctx, body_ctx)?;
            Ok(quote!(#name(#(#args),*)))
        }
    }
}

fn string_op_string_arg_indices(op: RuntimeOp) -> &'static [usize] {
    match op {
        RuntimeOp::StringLen | RuntimeOp::StringCharAt => &[0],
        RuntimeOp::StringConcat | RuntimeOp::StringEquals | RuntimeOp::StringIndexOf => &[0, 1],
        _ => &[],
    }
}

fn emit_array_op_with_element_type(
    op_name: &'static str,
    args: &[MirExpr],
    ty: TypeId,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let element_ty_id = match ctx.types.resolve(ty) {
        Some(Type::Array { element }) => *element,
        _ => ty,
    };
    let element_ty = emit_type_id_with_ctx(element_ty_id, ctx);
    let parts: Vec<TokenStream> = args
        .iter()
        .map(|a| emit_expr(a, ctx, body_ctx))
        .collect::<Result<_, _>>()?;
    let name = format_ident!("{op_name}");
    Ok(quote!(#name::<#element_ty>(vec![#(#parts),*])))
}

fn emit_js_string_arg(
    arg: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Borrowed, ctx, body_ctx)
}

fn emit_js_string_owned(
    arg: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    emit_js_string_expr(arg, StringOwnership::Owned, ctx, body_ctx)
}

#[derive(Clone, Copy)]
enum StringOwnership {
    Borrowed,
    Owned,
}

fn emit_js_string_expr(
    arg: &MirExpr,
    ownership: StringOwnership,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let core = match arg {
        MirExpr::String { id, .. } => {
            let lit = Literal::string(id.as_str());
            quote!(ts_aot_runtime::JsString::from(#lit))
        }
        MirExpr::Local(id) => body_ctx.local_ref(*id),
        _ => emit_expr(arg, ctx, body_ctx)?,
    };
    Ok(match ownership {
        StringOwnership::Borrowed => quote!(&#core),
        StringOwnership::Owned => quote!(#core.clone()),
    })
}

fn emit_switch(
    disc: &MirExpr,
    cases: &[ts_aot_ir_mir::SwitchCase],
    default: Option<&MirBlock>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let disc_expr = emit_expr(disc, ctx, body_ctx)?;
    let mut arms: Vec<TokenStream> = Vec::with_capacity(cases.len() + 1);
    for case in cases {
        let pat = match &case.value {
            ConstValue::Int(v) => {
                let lit = Literal::i128_unsuffixed(*v);
                quote!(#lit)
            }
            ConstValue::String(s) => {
                let lit = Literal::string(s.as_str());
                quote!(#lit)
            }
        };
        let body_stmts = emit_block_stmts(&case.body, ctx, body_ctx)?;
        arms.push(quote!(#pat => { #(#body_stmts)* }));
    }
    if let Some(def) = default {
        let body_stmts = emit_block_stmts(def, ctx, body_ctx)?;
        arms.push(quote!(_ => { #(#body_stmts)* }));
    } else {
        arms.push(quote!(_ => {}));
    }
    Ok(quote!(match #disc_expr { #(#arms),* }))
}

fn emit_try(
    body: &MirBlock,
    catch_param: Option<LocalId>,
    catch: Option<&MirBlock>,
    finally: Option<&MirBlock>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let try_id = body_ctx.alloc_try_id();
    let label = format_ident!("__try_{}", try_id);
    let slot_ident = format_ident!("__return_slot_{}", try_id);
    let prev_in_try = body_ctx.in_try();
    let prev_try_label = body_ctx.try_label();
    let prev_return_slot = body_ctx.return_slot();
    body_ctx.set_in_try(true);
    body_ctx.set_try_label(Some(label.clone()));
    body_ctx.set_return_slot(Some(slot_ident.clone()));
    let body_stmts = emit_block_stmts(body, ctx, body_ctx);
    body_ctx.set_try_label(prev_try_label);
    let body_stmts = body_stmts?;

    let catch_stmts = if let Some(catch_block) = catch {
        let prev = body_ctx.try_label();
        body_ctx.set_try_label(Some(label.clone()));
        let stmts = emit_block_stmts(catch_block, ctx, body_ctx);
        body_ctx.set_try_label(prev);
        Some(stmts?)
    } else {
        None
    };

    body_ctx.set_in_try(false);
    body_ctx.set_try_label(None);
    let finally_stmts = if let Some(fin) = finally {
        Some(emit_block_stmts(fin, ctx, body_ctx)?)
    } else {
        None
    };
    body_ctx.set_in_try(prev_in_try);
    body_ctx.set_return_slot(prev_return_slot);

    let catch_unwind = format_ident!("catch_unwind");
    let assert_unwind_safe = format_ident!("AssertUnwindSafe");
    let resume_unwind = format_ident!("resume_unwind");

    let inner_ret_ty = emit_type_id_with_ctx(body_ctx.return_type_id(), ctx);
    let slot_ty = if body_ctx.is_generator() {
        quote!(Option<Option<#inner_ret_ty>>)
    } else {
        quote!(Option<#inner_ret_ty>)
    };
    let slot_decl = quote!(let mut #slot_ident: #slot_ty = None;);
    let is_nested = body_ctx.return_slot().is_some();
    let outer_slot_ident = body_ctx.return_slot();
    let replay_return = if is_nested {
        let outer_slot = outer_slot_ident.expect("nested try must have an outer return slot");
        quote! {
            if let Some(__v) = #slot_ident {
                #outer_slot = Some(__v);
            }
        }
    } else {
        quote! {
            if let Some(__v) = #slot_ident {
                return __v;
            }
        }
    };

    let body_arm = if catch.is_some() {
        let catch_stmts = catch_stmts.expect("catch block present");
        if let Some(param) = catch_param {
            let param_ident = body_ctx.local_ident(param);
            let param_ty = body_ctx
                .local_ty(param)
                .map_or_else(|| quote!(()), |t| emit_type_id_with_ctx(t, ctx));
            quote! {
                let #param_ident: #param_ty = match __e.downcast::<#param_ty>() {
                    Ok(v) => *v,
                    Err(__e) => std::panic::#resume_unwind(__e),
                };
                let __catch_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #(#catch_stmts)*
                }));
                if let Err(__e2) = __catch_result {
                    __pending_throw = Some(__e2);
                }
            }
        } else {
            quote! {
                let __catch_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #(#catch_stmts)*
                }));
                if let Err(__e2) = __catch_result {
                    __pending_throw = Some(__e2);
                }
            }
        }
    } else if finally_stmts.is_some() {
        quote! {
            let __e = if let Ok(__sentinel) = __e.downcast::<TsAotThrowSentinel>() {
                __sentinel
            } else {
                std::panic::#resume_unwind(__e)
            };
            __pending_throw = Some(__e);
        }
    } else {
        quote! {}
    };

    let finally_block = if let Some(finally_stmts) = finally_stmts {
        quote! { #(#finally_stmts)* }
    } else {
        quote! {}
    };

    Ok(quote! {{
        enum __ReturnSignal { Continue, Break }
        #slot_decl
        let mut __pending_throw: Option<Box<dyn std::any::Any + Send>> = None;
        #label: loop {
            let __try_result: Result<__ReturnSignal, Box<dyn std::any::Any + Send>> =
                std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                    #(#body_stmts)*
                    Ok::<_, Box<dyn std::any::Any + Send>>(__ReturnSignal::Continue)
                }));
            match __try_result {
                Ok(__ReturnSignal::Break | __ReturnSignal::Continue) => break #label,
                Err(__e) => { #body_arm }
            }
        }
        #finally_block
        if let Some(__e) = __pending_throw {
            std::panic::#resume_unwind(__e);
        }
        #replay_return
    }})
}
