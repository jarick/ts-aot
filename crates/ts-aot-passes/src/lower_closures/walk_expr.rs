use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, Span};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, ObjectLiteralField};

use super::LowerClosuresStats;
use super::walk_stmt::walk_body;
use crate::PassContext;

#[allow(clippy::too_many_arguments)]
pub(super) fn walk_expr(
    expr: &mut HirExpr,
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    ctx: &mut PassContext,
) {
    if let HirExpr::Closure {
        id,
        params,
        captures,
        body,
        ty,
    } = expr
    {
        if !captures.is_empty() {
            ctx.warning(
                "P0007",
                "capturing closures are not yet supported by lower_closures; \
                 closure is left intact and will fail downstream in HIR→MIR",
                Span::new(0, 0),
            );
            stats.deferred_capturing += 1;
            return;
        }

        *expr = super::closure_lift::lift_non_capturing_closure(
            *id,
            params,
            body,
            *ty,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            walk_body,
            ctx,
        );
        return;
    }

    match expr {
        HirExpr::Closure { body, .. } => walk_body(
            body,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Call { callee, args, .. } => {
            walk_callee(
                callee,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            for a in args {
                walk_expr(
                    a,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            walk_expr(
                lhs,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                rhs,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirExpr::Unary { expr: e, .. } => walk_expr(
            e,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Field { owner, .. } => walk_expr(
            owner,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Index { owner, index, .. } => {
            walk_expr(
                owner,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                index,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                walk_expr(
                    e,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                walk_expr(
                    e,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                let value = match f {
                    ObjectLiteralField::Property { value, .. } => value,
                    ObjectLiteralField::Spread(value) => value,
                };
                walk_expr(
                    value,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            walk_expr(
                cond,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                then_branch,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                else_branch,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                walk_expr(
                    e,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::Await { expr: e, .. } | HirExpr::TypeAssertion { expr: e, .. } => walk_expr(
            e,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Yield { expr: Some(e), .. } => walk_expr(
            e,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag {
                walk_expr(
                    t,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
            for p in expressions {
                walk_expr(
                    p,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::New { callee, args, .. } => {
            walk_expr(
                callee,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            for a in args {
                walk_expr(
                    a,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
        }
        HirExpr::OptionalChain { base, .. } => walk_expr(
            base,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirExpr::Assignment { target, value, .. } => {
            walk_expr(
                target,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                value,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            walk_expr(
                target,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_expr(
                rhs,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirExpr::Unit
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Null
        | HirExpr::Undefined
        | HirExpr::Local { .. }
        | HirExpr::Global { .. }
        | HirExpr::Yield { expr: None, .. }
        | HirExpr::RegExp { .. }
        | HirExpr::BigInt { .. } => {}
        HirExpr::Import { source, .. } => {
            walk_expr(
                source,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn walk_callee(
    callee: &mut HirCallee,
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    ctx: &mut PassContext,
) {
    if matches!(callee, HirCallee::Closure(_)) {
        super::closure_lift::rewrite_closure_callee(callee, closure_names, new_decls, ctx);
        return;
    }
    if let HirCallee::Indirect(inner) = callee {
        walk_expr(
            inner,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        );
    }
}
