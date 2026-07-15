use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};
use ts_aot_core::TypeId;

use ts_aot_ir_mir::{
    BinaryOp, MirBlock, MirExpr, MirFunctionDecl, MirPlace, MirPlaceBase, MirStmt, RuntimeOp,
    UnaryOp,
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
        MirStmt::Return(None) => Ok(quote!(return;)),
        MirStmt::Return(Some(expr)) => {
            let expr = emit_expr(expr, ctx, body_ctx)?;
            Ok(quote!(return #expr;))
        }
        MirStmt::ReturnResultErr { error, .. } | MirStmt::Throw { error, .. } => {
            let error = emit_expr(error, ctx, body_ctx)?;
            Ok(quote!(return Err(#error);))
        }
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond = emit_expr(cond, ctx, body_ctx)?;
            let then_stmts = emit_block_stmts(then_block, ctx, body_ctx)?;
            if let Some(else_block) = else_block {
                let else_stmts = emit_block_stmts(else_block, ctx, body_ctx)?;
                Ok(quote!(if #cond { #(#then_stmts)* } else { #(#else_stmts)* }))
            } else {
                Ok(quote!(if #cond { #(#then_stmts)* }))
            }
        }
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
        MirStmt::Continue => Ok(quote!(continue;)),
        MirStmt::Runtime { op, args, dest, ty } => {
            let call = emit_runtime_call(*op, args, ctx, body_ctx)?;
            if let Some(dest) = dest {
                let dest = body_ctx.local_ident(*dest);
                let ty = emit_type_id_with_ctx(*ty, ctx);
                Ok(quote!(let #dest: #ty = #call;))
            } else {
                Ok(quote!(#call;))
            }
        }
        MirStmt::Switch { .. } | MirStmt::Try { .. } => Err(BackendError::NotImplemented),
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

fn emit_expr(
    expr: &MirExpr,
    ctx: &EmitCtx<'_>,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Unit | MirExpr::Null { .. } => Ok(quote!(())),
        MirExpr::Bool(value) => Ok(quote!(#value)),
        MirExpr::Int { value, .. } => Ok(emit_whole_number_literal(*value)),
        MirExpr::Float { value, .. } if value.is_finite() => {
            let literal = Literal::f64_unsuffixed(*value);
            Ok(quote!(#literal))
        }
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
        MirExpr::Yield { .. } | MirExpr::Float { .. } => Err(BackendError::NotImplemented),
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
    if op == RuntimeOp::OpInstanceof {
        let value = emit_expr(&args[0], ctx, body_ctx)?;
        let target_type_id: u32 = match args.get(2) {
            Some(MirExpr::Int { value, .. }) => (*value).try_into().unwrap_or(0),
            _ => 0,
        };
        return Ok(quote!(__ts_aot_op_instanceof(&#value, #target_type_id)));
    }
    if op == RuntimeOp::TypeOf {
        return emit_typeof(&args[0], ctx, body_ctx);
    }
    let name = runtime_op_ident(op);
    let args = emit_exprs(args, ctx, body_ctx)?;
    Ok(quote!(#name(#(#args),*)))
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
        RuntimeOp::OpDelete => format_ident!("__ts_aot_op_delete"),
        RuntimeOp::OpIn => format_ident!("__ts_aot_op_in"),
        RuntimeOp::OpInstanceof => format_ident!("__ts_aot_op_instanceof"),
    }
}
