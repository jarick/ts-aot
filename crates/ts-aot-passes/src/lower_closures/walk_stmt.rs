use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId};
use ts_aot_ir_hir::HirStmt;

use super::LowerClosuresStats;
use super::walk::walk_decl;
use super::walk_expr::walk_expr;
use crate::PassContext;
pub(super) fn walk_body(
    body: &mut [HirStmt],
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<ts_aot_ir_hir::HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    ctx: &mut PassContext,
) {
    for stmt in body.iter_mut() {
        walk_stmt(
            stmt,
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
pub(super) fn walk_stmt(
    stmt: &mut HirStmt,
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<ts_aot_ir_hir::HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    ctx: &mut PassContext,
) {
    match stmt {
        HirStmt::Block(stmts) => walk_body(
            stmts,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirStmt::Let { init: Some(e), .. } | HirStmt::Expr { expr: e } => walk_expr(
            e,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirStmt::Let { init: None, .. } => {}
        HirStmt::If {
            cond,
            then,
            otherwise,
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
            walk_stmt(
                then,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            if let Some(else_stmt) = otherwise {
                walk_stmt(
                    else_stmt,
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
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
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
            walk_stmt(
                body,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            walk_expr(
                iter,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            walk_stmt(
                body,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
        }
        HirStmt::Switch { disc, cases } => {
            walk_expr(
                disc,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            for case in cases {
                if let Some(test) = &mut case.test {
                    walk_expr(
                        test,
                        next_id,
                        closure_names,
                        new_decls,
                        generated,
                        taken,
                        stats,
                        ctx,
                    );
                }
                walk_body(
                    &mut case.body,
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
        HirStmt::Return { value: Some(e) } => walk_expr(
            e,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirStmt::Return { value: None } => {}
        HirStmt::Throw { expr } => walk_expr(
            expr,
            next_id,
            closure_names,
            new_decls,
            generated,
            taken,
            stats,
            ctx,
        ),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            walk_stmt(
                body,
                next_id,
                closure_names,
                new_decls,
                generated,
                taken,
                stats,
                ctx,
            );
            if let Some(c) = catch {
                walk_stmt(
                    &mut c.body,
                    next_id,
                    closure_names,
                    new_decls,
                    generated,
                    taken,
                    stats,
                    ctx,
                );
            }
            if let Some(f) = finally {
                walk_stmt(
                    f,
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
        HirStmt::Decl(d) => walk_decl(d, next_id, new_decls, generated, taken, stats, ctx),
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
    }
}
