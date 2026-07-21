use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::{LocalId, TypeId};

use ts_aot_ir_mir::{
    BinaryOp, ConstValue, MirBlock, MirExpr, MirFunctionDecl, MirPlace, MirPlaceBase, MirStmt,
    RuntimeOp, UnaryOp,
};

use super::ctx::{BodyCtx, EmitCtx};
use super::ident::ident_from;
use super::literals::emit_whole_number_literal;
use super::types::emit_type_id_with_ctx;
use crate::error::BackendError;

pub(super) fn emit_body(
    f: &MirFunctionDecl,
    ctx: &EmitCtx<'_>,
) -> Result<TokenStream, BackendError> {
    if f.body.block.is_empty() {
        return Ok(quote!({ unimplemented!() }));
    }
    let body_ctx = BodyCtx::new(f);
    let stmts = emit_block_stmts(&f.body.block, ctx, &body_ctx)?;
    Ok(quote!({ #(#stmts)* }))
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
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let call = emit_runtime_call(op, args, ctx, body_ctx)?;
    if matches!(op, RuntimeOp::OpObjectSet | RuntimeOp::OpObjectDelete) {
        let _ = dest;
        return Ok(quote!(#call;));
    }
    if let Some(dest) = dest {
        let dest = body_ctx.local_ident(dest);
        let ty = emit_type_id_with_ctx(ty, ctx);
        let mutability = if matches!(
            op,
            RuntimeOp::OpObjectUnwrap | RuntimeOp::OpObjectNew | RuntimeOp::DynVecNew
        ) {
            quote!(mut)
        } else {
            quote!()
        };
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
    if let Some(label) = body_ctx.try_label() {
        if let Some(expr) = slot {
            let expr = emit_expr(expr, ctx, body_ctx)?;
            body_ctx.set_pending_return(Some(quote!(#expr)));
        } else {
            body_ctx.set_pending_return(Some(quote!(())));
        }
        Ok(quote!(break #label;))
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
            let mutability = if *mutable { quote!(mut) } else { quote!() };
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
            body,
        } => {
            let item = body_ctx.local_ident(*item);
            let iterable = emit_expr(iterable, ctx, body_ctx)?;
            let body_stmts = emit_block_stmts(body, ctx, body_ctx)?;
            Ok(quote!(for #item in #iterable { #(#body_stmts)* }))
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
        MirStmt::Runtime { op, args, dest, ty } => {
            emit_runtime_stmt(*op, args, *dest, *ty, ctx, body_ctx)
        }
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

#[allow(clippy::too_many_lines)]
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
            Ok(quote!(String::from(#literal)))
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
        MirExpr::Await { expr, .. } => {
            let expr = emit_expr(expr, ctx, body_ctx)?;
            Ok(quote!(#expr.await))
        }
        MirExpr::OptionalChain { base, .. } => emit_expr(base, ctx, body_ctx),
        MirExpr::TypeOf { expr, .. } => emit_typeof(expr, ctx, body_ctx),
        MirExpr::DynamicFrom { value, .. } => emit_dynamic_from(value, ctx, body_ctx),
        MirExpr::TemplateStringsArray { cooked, raw, .. } => {
            let cooked_lits: Vec<TokenStream> = cooked
                .iter()
                .map(|p| {
                    let lit = Literal::string(p.as_str());
                    quote!(String::from(#lit))
                })
                .collect();
            let raw_lits: Vec<TokenStream> = raw
                .iter()
                .map(|p| {
                    let lit = Literal::string(p.as_str());
                    quote!(String::from(#lit))
                })
                .collect();
            Ok(quote!(ts_aot_runtime::TemplateStringsArray::new(
                vec![#(#cooked_lits),*],
                vec![#(#raw_lits),*]
            )))
        }
        MirExpr::RegExp { pattern, flags, .. } => {
            let pattern_lit = Literal::string(pattern);
            let flags_lit = Literal::string(flags);
            Ok(quote!(ts_aot_runtime::__ts_aot_regex_new(#pattern_lit, #flags_lit)))
        }
        MirExpr::Yield { expr, .. } => match expr {
            Some(inner) => emit_expr(inner, ctx, body_ctx),
            None => Ok(quote!(())),
        },
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

fn emit_dynamic_from(
    value: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match value {
        MirExpr::Unit => Ok(quote!(DynamicValue::Undefined)),
        MirExpr::Null { .. } => Ok(quote!(DynamicValue::Null)),
        _ => {
            let inner = emit_expr(value, ctx, body_ctx)?;
            Ok(quote!(DynamicValue::from(#inner)))
        }
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

fn emit_runtime_call(
    op: RuntimeOp,
    args: &[MirExpr],
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match op {
        RuntimeOp::OpInstanceof => {
            let value = emit_expr(&args[0], ctx, body_ctx)?;
            let target_type_id: u32 = match args.get(2) {
                Some(MirExpr::Int { value, .. }) => (*value).try_into().unwrap_or(0),
                _ => 0,
            };
            Ok(quote!(__ts_aot_op_instanceof(&#value, #target_type_id)))
        }
        RuntimeOp::TypeOf => emit_typeof(&args[0], ctx, body_ctx),
        RuntimeOp::OpObjectGet => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let field_name = extract_string_arg(&args[1])?;
            Ok(quote!(__ts_aot_dynamic_get(&#obj, #field_name)))
        }
        RuntimeOp::OpObjectUnwrap => {
            let opt = emit_expr(&args[0], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_dynamic_unwrap(#opt)))
        }
        RuntimeOp::OpObjectNew => Ok(quote!(__ts_aot_object_new())),
        RuntimeOp::DynVecAppend => {
            let vec = emit_expr(&args[0], ctx, body_ctx)?;
            let value = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_dyn_vec_append(&mut #vec, #value)))
        }
        RuntimeOp::OpObjectSet => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let field_name = extract_string_arg(&args[1])?;
            let value = emit_expr(&args[2], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_dynamic_set(&mut #obj, #field_name, #value)))
        }
        RuntimeOp::OpObjectHas => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let key = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_dynamic_has(&#obj, &__ts_aot_dynamic_key(#key.as_str()))))
        }
        RuntimeOp::OpObjectDelete => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let field_name = extract_string_arg(&args[1])?;
            Ok(quote!(__ts_aot_dynamic_delete(&mut #obj, #field_name)))
        }
        RuntimeOp::OpObjectProtoGet => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_object_proto_get(&#obj)))
        }
        RuntimeOp::OpObjectProtoSet => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let proto = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_object_proto_set(&#obj, #proto)))
        }
        RuntimeOp::OpObjectSetPrototypeOf => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            let proto = emit_expr(&args[1], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_object_set_prototype_of(&#obj, #proto)))
        }
        RuntimeOp::OpObjectKeys => {
            let obj = emit_expr(&args[0], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_object_keys(&#obj)))
        }
        RuntimeOp::OpDynamicBinary => {
            let op_id = match args.first() {
                Some(MirExpr::Int { value, .. }) => u8::try_from(*value).unwrap_or(0),
                _ => 0,
            };
            let left = emit_expr(&args[1], ctx, body_ctx)?;
            let right = emit_expr(&args[2], ctx, body_ctx)?;
            Ok(quote!(__ts_aot_dynamic_op(#op_id, &#left, &#right)))
        }
        _ => {
            let name = runtime_op_ident(op);
            let args = emit_exprs(args, ctx, body_ctx)?;
            Ok(quote!(#name(#(#args),*)))
        }
    }
}

fn extract_string_arg(expr: &MirExpr) -> Result<TokenStream, BackendError> {
    let MirExpr::String { id, .. } = expr else {
        return Err(BackendError::NotImplemented);
    };
    let literal = Literal::string(id.as_str());
    Ok(quote!(#literal))
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

#[allow(clippy::too_many_lines)]
fn emit_try(
    body: &MirBlock,
    catch_param: Option<LocalId>,
    catch: Option<&MirBlock>,
    finally: Option<&MirBlock>,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let label = format_ident!("__try_{}", body.stmts.len());
    let prev_in_try = body_ctx.in_try();
    let prev_try_label = body_ctx.try_label();
    let prev_pending_return = body_ctx.take_pending_return();
    body_ctx.set_in_try(true);
    body_ctx.set_try_label(Some(label.clone()));
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
    let pending_return_after_try = body_ctx.take_pending_return();
    body_ctx.set_pending_return(prev_pending_return);

    let catch_unwind = format_ident!("catch_unwind");
    let assert_unwind_safe = format_ident!("AssertUnwindSafe");
    let resume_unwind = format_ident!("resume_unwind");

    let replay_return = if let Some(return_expr) = &pending_return_after_try {
        quote! {
            return #return_expr;
        }
    } else {
        quote! {}
    };

    let body_arm = if catch.is_some() {
        let catch_stmts = catch_stmts.expect("catch block present");
        if let Some(param) = catch_param {
            let param_ident = body_ctx.local_ident(param);
            let param_ty = body_ctx
                .local_ty(param)
                .map_or_else(|| quote!(()), |t| emit_type_id_with_ctx(t, ctx));
            quote! {
                if let Err(__e) = __try_result {
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
            }
        } else {
            quote! {
                if let Err(__e) = __try_result {
                    let __catch_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                        #(#catch_stmts)*
                    }));
                    if let Err(__e2) = __catch_result {
                        __pending_throw = Some(__e2);
                    }
                }
            }
        }
    } else if finally_stmts.is_some() {
        quote! {
            if let Err(__e) = __try_result {
                let __e = if let Ok(__sentinel) = __e.downcast::<TsAotThrowSentinel>() {
                    __sentinel
                } else {
                    std::panic::#resume_unwind(__e)
                };
                __pending_throw = Some(__e);
            }
        }
    } else {
        quote! {
            let _ = __try_result;
        }
    };

    let finally_block = if let Some(finally_stmts) = finally_stmts {
        quote! { #(#finally_stmts)* }
    } else {
        quote! {}
    };

    Ok(quote! {{
        let mut __pending_throw: Option<Box<dyn std::any::Any + Send>> = None;
        #label: loop {
            let __try_result = std::panic::#catch_unwind(std::panic::#assert_unwind_safe(|| {
                #(#body_stmts)*
            }));
            #body_arm
            break #label;
        }
        #finally_block
        if let Some(__e) = __pending_throw {
            std::panic::#resume_unwind(__e);
        }
        #replay_return
    }})
}

fn runtime_op_ident(op: RuntimeOp) -> Ident {
    match op {
        RuntimeOp::StringConcat => format_ident!("__ts_aot_string_concat"),
        RuntimeOp::StringEquals => format_ident!("__ts_aot_string_equals"),
        RuntimeOp::StringLen => format_ident!("__ts_aot_string_len"),
        RuntimeOp::ArrayCreate => format_ident!("__ts_aot_array_create"),
        RuntimeOp::ArrayGet => format_ident!("__ts_aot_array_get"),
        RuntimeOp::ArraySet => format_ident!("__ts_aot_array_set"),
        RuntimeOp::ArrayLen => format_ident!("__ts_aot_array_len"),
        RuntimeOp::MapGet => format_ident!("__ts_aot_map_get"),
        RuntimeOp::MapSet => format_ident!("__ts_aot_map_set"),
        RuntimeOp::ResultOk => format_ident!("__ts_aot_result_ok"),
        RuntimeOp::ResultErr => format_ident!("__ts_aot_result_err"),
        RuntimeOp::ResultUnwrapOk => format_ident!("__ts_aot_result_unwrap_ok"),
        RuntimeOp::PromiseCreate => format_ident!("__ts_aot_promise_create"),
        RuntimeOp::PromiseResolve => format_ident!("__ts_aot_promise_resolve"),
        RuntimeOp::HostConsoleLog => format_ident!("__ts_aot_host_console_log"),
        RuntimeOp::MathSqrt => format_ident!("__ts_aot_math_sqrt"),
        RuntimeOp::TypeOf => unreachable!("TypeOf is handled by emit_typeof, not runtime_op_ident"),
        RuntimeOp::OpIn => format_ident!("__ts_aot_op_in"),
        RuntimeOp::OpInstanceof => format_ident!("__ts_aot_op_instanceof"),
        RuntimeOp::OpObjectGet => format_ident!("__ts_aot_dynamic_get"),
        RuntimeOp::OpObjectSet => format_ident!("__ts_aot_dynamic_set"),
        RuntimeOp::OpObjectHas => format_ident!("__ts_aot_dynamic_has"),
        RuntimeOp::OpObjectDelete => format_ident!("__ts_aot_dynamic_delete"),
        RuntimeOp::OpObjectUnwrap => format_ident!("__ts_aot_dynamic_unwrap"),
        RuntimeOp::OpObjectNew => format_ident!("__ts_aot_object_new"),
        RuntimeOp::OpObjectProtoGet => format_ident!("__ts_aot_object_proto_get"),
        RuntimeOp::OpObjectProtoSet => format_ident!("__ts_aot_object_proto_set"),
        RuntimeOp::OpObjectSetPrototypeOf => format_ident!("__ts_aot_object_set_prototype_of"),
        RuntimeOp::OpObjectKeys => format_ident!("__ts_aot_object_keys"),
        RuntimeOp::OpDynamicBinary => format_ident!("__ts_aot_dynamic_op"),
        RuntimeOp::DynVecNew => format_ident!("__ts_aot_dyn_vec_new"),
        RuntimeOp::DynVecAppend => format_ident!("__ts_aot_dyn_vec_append"),
    }
}
