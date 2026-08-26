use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirStmt};

use super::LowerClosuresStats;
use super::walk_expr::walk_expr;
use super::walk_stmt::walk_body;
use crate::PassContext;

pub(super) fn walk_decl(
    decl: &mut HirDecl,
    next_id: &mut u32,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) {
    match decl {
        HirDecl::Function(f) => process_scope(
            &mut f.body,
            next_id,
            new_decls,
            generated,
            taken,
            stats,
            types,
            ctx,
        ),
        HirDecl::Class(c) => {
            for method in &mut c.methods {
                process_scope(
                    &mut method.body,
                    next_id,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    types,
                    ctx,
                );
            }
        }
        HirDecl::Global { init: Some(e), .. } => {
            process_global_init(e, next_id, new_decls, generated, taken, stats, types, ctx);
        }
        HirDecl::Namespace { members, .. } => {
            for m in members {
                walk_decl(m, next_id, new_decls, generated, taken, stats, types, ctx);
            }
        }
        HirDecl::Global { init: None, .. }
        | HirDecl::Enum { .. }
        | HirDecl::TypeAlias { .. }
        | HirDecl::Interface { .. } => {}
    }
}

fn process_scope(
    body: &mut [HirStmt],
    next_id: &mut u32,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) {
    let mut closure_names: HashMap<LocalId, Atom> = HashMap::new();
    walk_body(
        body,
        next_id,
        &mut closure_names,
        new_decls,
        generated,
        taken,
        stats,
        types,
        ctx,
    );
    if !closure_names.is_empty() {
        super::rewrite::rewrite_in_body(body, &closure_names, ctx);
    }
}

fn process_global_init(
    expr: &mut HirExpr,
    next_id: &mut u32,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) {
    let mut closure_names: HashMap<LocalId, Atom> = HashMap::new();
    walk_expr(
        expr,
        next_id,
        &mut closure_names,
        new_decls,
        generated,
        taken,
        stats,
        types,
        ctx,
    );
    if !closure_names.is_empty() {
        super::rewrite::rewrite_in_global_init(expr, &closure_names, ctx);
    }
}
