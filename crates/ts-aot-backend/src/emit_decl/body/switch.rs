use super::super::ctx::{BodyCtx, EmitCtx, EmitEnv};
use super::super::types::emit_type_id_with_ctx;
use super::expr::emit_expr;
use super::helpers::{is_dynamic_expr, is_float_type, loop_label, value_ty};
use super::stmt::{emit_stmt, let_init_tokens};
use crate::error::BackendError;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashSet;
use ts_aot_core::{LocalId, Type, TypeId};
use ts_aot_ir_mir::{ConstValue, MirBlock, MirExpr, MirStmt};

pub(super) fn emit_switch(
    disc: &MirExpr,
    cases: &[ts_aot_ir_mir::SwitchCase],
    default: Option<&MirBlock>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<TokenStream, BackendError> {
    let disc_expr = emit_expr(disc, ctx, emit_env, body_ctx)?;
    let switch_ident = format_ident!("__switch_{}", body_ctx.alloc_block_id());
    let prev_break = body_ctx.break_label();
    body_ctx.set_break_label(Some(switch_ident.clone()));
    body_ctx.enter_switch();
    let mut arm_blocks: Vec<&MirBlock> = cases.iter().map(|case| &case.body).collect();
    if let Some(def) = default {
        arm_blocks.push(def);
    }
    let hoisted = switch_hoisted_locals(&arm_blocks, ctx, emit_env, body_ctx)?;
    let hoisted_ids: HashSet<LocalId> = hoisted.iter().map(|(local, _)| *local).collect();
    let mut decls: Vec<TokenStream> = Vec::with_capacity(hoisted.len());
    for (local, ty) in &hoisted {
        let name = body_ctx.local_ident(*local);
        let ty_tokens = emit_type_id_with_ctx(*ty, emit_env, ctx);
        let mut budget = super::super::CONST_DEFAULT_BUDGET;
        let default_tokens = super::super::emit_const_default(
            *ty,
            ctx,
            emit_env,
            0,
            &mut budget,
            super::super::types::LifetimeMode::Full,
        )?;
        decls.push(quote!(let mut #name: #ty_tokens = #default_tokens;));
    }
    let mut case_bodies: Vec<(&ConstValue, Vec<TokenStream>)> = Vec::with_capacity(cases.len());
    for case in cases {
        let body_stmts = emit_switch_arm(&case.body, &hoisted_ids, ctx, emit_env, body_ctx)?;
        case_bodies.push((&case.value, body_stmts));
    }
    let default_body = match default {
        Some(def) => Some(emit_switch_arm(def, &hoisted_ids, ctx, emit_env, body_ctx)?),
        None => None,
    };
    body_ctx.exit_switch();
    body_ctx.set_break_label(prev_break);
    let lt = loop_label(&switch_ident);
    if let Some(kind) = switch_chain_kind(disc, ctx, emit_env, body_ctx) {
        let disc_ident =
            body_ctx.alloc_hoist_ident(&format!("__switch_disc_{}", body_ctx.alloc_block_id()));
        let mut chain = if let Some(stmts) = &default_body {
            quote!({ #(#stmts)* })
        } else {
            quote!({})
        };
        for (value, body_stmts) in case_bodies.iter().rev() {
            let test = switch_case_equality(kind, &disc_ident, value);
            chain = quote!(if #test { #(#body_stmts)* } else #chain);
        }
        return Ok(quote!(
            #lt: {
                let #disc_ident = #disc_expr;
                #(#decls)*
                #chain
            }
        ));
    }
    let mut arms: Vec<TokenStream> = Vec::with_capacity(cases.len() + 1);
    for (value, body_stmts) in &case_bodies {
        let pat = match value {
            ConstValue::Int(v) => {
                let lit = Literal::i128_unsuffixed(*v);
                quote!(#lit)
            }
            ConstValue::String(s) => {
                let lit = Literal::string(s.as_str());
                quote!(#lit)
            }
        };
        arms.push(quote!(#pat => { #(#body_stmts)* }));
    }
    match &default_body {
        Some(body_stmts) => arms.push(quote!(_ => { #(#body_stmts)* })),
        None => arms.push(quote!(_ => {})),
    }
    if decls.is_empty() {
        Ok(quote!(#lt: { match #disc_expr { #(#arms),* } }))
    } else {
        let disc_ident =
            body_ctx.alloc_hoist_ident(&format!("__switch_disc_{}", body_ctx.alloc_block_id()));
        Ok(quote!(
            #lt: {
                let #disc_ident = #disc_expr;
                #(#decls)*
                match #disc_ident { #(#arms),* }
            }
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SwitchChainKind {
    Float,
    String,
}

fn switch_chain_kind(
    disc: &MirExpr,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Option<SwitchChainKind> {
    match emit_env
        .types
        .resolve(value_ty(disc, ctx, emit_env, body_ctx)?)?
    {
        Type::String => Some(SwitchChainKind::String),
        ty if is_float_type(ty) => Some(SwitchChainKind::Float),
        _ => None,
    }
}

fn switch_case_equality(kind: SwitchChainKind, disc: &Ident, value: &ConstValue) -> TokenStream {
    match (kind, value) {
        (SwitchChainKind::Float, ConstValue::Int(v)) => {
            let lit = Literal::f64_unsuffixed(*v as f64);
            quote!(#disc == #lit)
        }
        (SwitchChainKind::String, ConstValue::String(s)) => {
            let lit = Literal::string(s.as_str());
            quote!(#disc == ts_aot_runtime::JsString::from(#lit))
        }
        (SwitchChainKind::Float, ConstValue::String(_))
        | (SwitchChainKind::String, ConstValue::Int(_)) => quote!(false),
    }
}

fn switch_hoisted_locals(
    arms: &[&MirBlock],
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<(LocalId, TypeId)>, BackendError> {
    let mut arm_uses: Vec<HashSet<LocalId>> = Vec::with_capacity(arms.len());
    let candidates = arms
        .iter()
        .flat_map(|arm| arm.stmts.iter())
        .filter_map(|stmt| {
            if let MirStmt::Let { local, .. } = stmt {
                Some(*local)
            } else {
                None
            }
        })
        .collect();
    for arm in arms {
        let mut counts: std::collections::HashMap<LocalId, (u32, bool)> =
            std::collections::HashMap::new();
        super::super::ctx::count_uses_block(arm, false, &mut counts);
        let mut uses: HashSet<_> = counts.into_keys().collect();
        uses.extend(super::super::ctx::collect_assigned_locals(
            &arm.stmts,
            emit_env.types,
            &candidates,
        ));
        arm_uses.push(uses);
    }
    let mut hoisted: Vec<(LocalId, TypeId)> = Vec::new();
    for (index, arm) in arms.iter().enumerate() {
        for stmt in &arm.stmts {
            let MirStmt::Let {
                local, ty, init, ..
            } = stmt
            else {
                continue;
            };
            if hoisted.iter().any(|(other, _)| other == local) {
                continue;
            }
            if body_ctx.shared_access_for_local(*local).is_some() {
                continue;
            }
            if matches!(
                emit_env.types.resolve(*ty),
                None | Some(Type::Error | Type::Void)
            ) {
                continue;
            }
            if let Some(init) = init
                && is_dynamic_expr(init, body_ctx)
            {
                continue;
            }
            let read_elsewhere = arm_uses
                .iter()
                .enumerate()
                .any(|(other, ids)| other != index && ids.contains(local));
            if !read_elsewhere {
                continue;
            }
            let mut budget = super::super::CONST_DEFAULT_BUDGET;
            if super::super::emit_const_default(
                *ty,
                ctx,
                emit_env,
                0,
                &mut budget,
                super::super::types::LifetimeMode::Full,
            )
            .is_err()
            {
                return Err(BackendError::NotImplemented(
                    "a `switch` case declares a local that another case reads, and this local's \
                     type has no default value to hoist the declaration with",
                ));
            }
            hoisted.push((*local, *ty));
        }
    }
    Ok(hoisted)
}

fn emit_switch_arm(
    block: &MirBlock,
    hoisted: &HashSet<LocalId>,
    ctx: &EmitCtx,
    emit_env: &EmitEnv,
    body_ctx: &BodyCtx,
) -> Result<Vec<TokenStream>, BackendError> {
    let mut out = Vec::with_capacity(block.stmts.len());
    for stmt in &block.stmts {
        match stmt {
            MirStmt::Let {
                local, ty, init, ..
            } if hoisted.contains(local) => {
                let name = body_ctx.local_ident(*local);
                if let Some(init) = init {
                    let value = let_init_tokens(init, *ty, ctx, emit_env, body_ctx)?;
                    out.push(quote!(#name = #value;));
                }
            }
            other => out.push(emit_stmt(other, ctx, emit_env, body_ctx)?),
        }
    }
    Ok(out)
}
