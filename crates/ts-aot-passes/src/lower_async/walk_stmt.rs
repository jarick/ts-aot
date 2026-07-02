use ts_aot_core::Atom;
use ts_aot_ir_hir::HirStmt;

use super::LowerAsyncStats;
use super::expr::rewrite_expr;
use super::rewrite_decl;

pub(super) fn rewrite_body(
    body: &mut [HirStmt],
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    for stmt in body {
        rewrite_stmt(
            stmt,
            promise_sym,
            resolve_sym,
            can_rewrite_promise_resolve,
            stats,
        );
    }
}

pub(super) fn rewrite_stmt(
    stmt: &mut HirStmt,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    match stmt {
        HirStmt::Block(stmts) => rewrite_body(
            stmts,
            promise_sym,
            resolve_sym,
            can_rewrite_promise_resolve,
            stats,
        ),
        HirStmt::Let { init, .. } => {
            if let Some(expr) = init.as_mut() {
                rewrite_expr(
                    expr,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirStmt::Expr { expr } => {
            rewrite_expr(
                expr,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            rewrite_expr(
                cond,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_stmt(
                then.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            if let Some(else_stmt) = otherwise.as_mut() {
                rewrite_stmt(
                    else_stmt.as_mut(),
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirStmt::While { cond, body } => {
            rewrite_expr(
                cond,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_stmt(
                body.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::DoWhile { body, cond } => {
            rewrite_stmt(
                body.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_expr(
                cond,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::ForOf { iter, body, .. } => {
            rewrite_expr(
                iter,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_stmt(
                body.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::ForIn { iter, body, .. } => {
            rewrite_expr(
                iter,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_stmt(
                body.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::Switch { disc, cases } => {
            rewrite_expr(
                disc,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            for case in cases {
                if let Some(test) = case.test.as_mut() {
                    rewrite_expr(
                        test,
                        promise_sym,
                        resolve_sym,
                        can_rewrite_promise_resolve,
                        stats,
                    );
                }
                rewrite_body(
                    &mut case.body,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirStmt::Return { value: Some(expr) } => {
            rewrite_expr(
                expr,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::Return { value: None } => {}
        HirStmt::Throw { expr } => {
            rewrite_expr(
                expr,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmt(
                body.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            if let Some(c) = catch.as_mut() {
                rewrite_stmt(
                    c.body.as_mut(),
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
            if let Some(f) = finally.as_mut() {
                rewrite_stmt(
                    f.as_mut(),
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirStmt::Decl(decl) => rewrite_decl(
            decl,
            promise_sym,
            resolve_sym,
            can_rewrite_promise_resolve,
            stats,
        ),
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
    }
}
