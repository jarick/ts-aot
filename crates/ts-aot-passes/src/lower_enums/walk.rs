use std::collections::HashMap;

use ts_aot_core::Atom;
use ts_aot_ir_hir::{HirDecl, HirStmt};

use super::expr::rewrite_expr;

pub(super) fn rewrite_decl(decl: &mut HirDecl, map: &HashMap<(Atom, Atom), Atom>) {
    match decl {
        HirDecl::Function(f) => rewrite_body(&mut f.body, map),
        HirDecl::Class(c) => {
            for m in &mut c.methods {
                rewrite_body(&mut m.body, map);
            }
        }
        HirDecl::Global { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirDecl::TypeAlias { .. }
        | HirDecl::Enum { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Namespace { .. } => {}
    }
}

pub(super) fn rewrite_body(body: &mut [HirStmt], map: &HashMap<(Atom, Atom), Atom>) {
    for stmt in body.iter_mut() {
        rewrite_stmt(stmt, map);
    }
}

fn rewrite_stmt(stmt: &mut HirStmt, map: &HashMap<(Atom, Atom), Atom>) {
    match stmt {
        HirStmt::Block(stmts) => rewrite_body(stmts, map),
        HirStmt::Let { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirStmt::Expr { expr } => rewrite_expr(expr, map),
        HirStmt::Return { value } => {
            if let Some(e) = value.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirStmt::Throw { expr } => rewrite_expr(expr, map),
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            rewrite_expr(cond, map);
            rewrite_stmt(then, map);
            if let Some(o) = otherwise.as_mut() {
                rewrite_stmt(o, map);
            }
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            rewrite_expr(cond, map);
            rewrite_stmt(body, map);
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            rewrite_expr(iter, map);
            rewrite_stmt(body, map);
        }
        HirStmt::Switch { disc, cases } => {
            rewrite_expr(disc, map);
            for case in cases {
                if let Some(test) = case.test.as_mut() {
                    rewrite_expr(test, map);
                }
                rewrite_body(&mut case.body, map);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmt(body, map);
            if let Some(c) = catch.as_mut() {
                rewrite_stmt(&mut c.body, map);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_stmt(f, map);
            }
        }
        HirStmt::Decl(decl) => rewrite_decl(decl, map),
        HirStmt::Break { .. } | HirStmt::Continue { .. } => {}
    }
}
