use std::collections::HashMap;

use ts_aot_core::{Atom, LocalId, Span, TypeId};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirStmt, ObjectLiteralField};

use crate::PassContext;

pub(super) fn rewrite_in_body(
    body: &mut [HirStmt],
    closure_names: &HashMap<LocalId, Atom>,
    ctx: &mut PassContext,
) {
    for stmt in body {
        rewrite_in_stmt(stmt, closure_names, ctx);
    }
}

pub(super) fn rewrite_in_stmt(
    stmt: &mut HirStmt,
    closure_names: &HashMap<LocalId, Atom>,
    ctx: &mut PassContext,
) {
    match stmt {
        HirStmt::Block(stmts) => rewrite_in_body(stmts, closure_names, ctx),
        HirStmt::Let { init: Some(e), .. } | HirStmt::Expr { expr: e } => {
            rewrite_in_expr(e, closure_names, ctx);
        }
        HirStmt::If {
            cond,
            then,
            otherwise,
        } => {
            rewrite_in_expr(cond, closure_names, ctx);
            rewrite_in_stmt(then, closure_names, ctx);
            if let Some(else_stmt) = otherwise {
                rewrite_in_stmt(else_stmt, closure_names, ctx);
            }
        }
        HirStmt::While { cond, body } | HirStmt::DoWhile { body, cond } => {
            rewrite_in_expr(cond, closure_names, ctx);
            rewrite_in_stmt(body, closure_names, ctx);
        }
        HirStmt::ForOf { iter, body, .. } | HirStmt::ForIn { iter, body, .. } => {
            rewrite_in_expr(iter, closure_names, ctx);
            rewrite_in_stmt(body, closure_names, ctx);
        }
        HirStmt::Switch { disc, cases } => {
            rewrite_in_expr(disc, closure_names, ctx);
            for case in cases {
                if let Some(test) = &mut case.test {
                    rewrite_in_expr(test, closure_names, ctx);
                }
                rewrite_in_body(&mut case.body, closure_names, ctx);
            }
        }
        HirStmt::Return { value: Some(e) } => rewrite_in_expr(e, closure_names, ctx),
        HirStmt::Return { value: None } => {}
        HirStmt::Throw { expr } => rewrite_in_expr(expr, closure_names, ctx),
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_in_stmt(body, closure_names, ctx);
            if let Some(c) = catch {
                rewrite_in_stmt(&mut c.body, closure_names, ctx);
            }
            if let Some(f) = finally {
                rewrite_in_stmt(f, closure_names, ctx);
            }
        }
        HirStmt::Decl(d) => rewrite_in_decl(d, closure_names, ctx),
        HirStmt::Break { .. } | HirStmt::Continue { .. } | HirStmt::Let { init: None, .. } => {}
    }
}

pub(super) fn rewrite_in_decl(
    decl: &mut HirDecl,
    closure_names: &HashMap<LocalId, Atom>,
    ctx: &mut PassContext,
) {
    match decl {
        HirDecl::Function(f) => rewrite_in_body(&mut f.body, closure_names, ctx),
        HirDecl::Class(c) => {
            for method in &mut c.methods {
                rewrite_in_body(&mut method.body, closure_names, ctx);
            }
        }
        HirDecl::Global { init: Some(e), .. } => {
            rewrite_in_global_init(e, closure_names, ctx);
        }
        HirDecl::Namespace { members, .. } => {
            for m in members {
                rewrite_in_decl(m, closure_names, ctx);
            }
        }
        HirDecl::Enum { .. }
        | HirDecl::TypeAlias { .. }
        | HirDecl::Interface { .. }
        | HirDecl::Global { init: None, .. } => {}
    }
}

pub(super) fn rewrite_in_global_init(
    expr: &mut HirExpr,
    closure_names: &HashMap<LocalId, Atom>,
    ctx: &mut PassContext,
) {
    rewrite_in_expr(expr, closure_names, ctx);
}

pub(super) fn rewrite_in_expr(
    expr: &mut HirExpr,
    closure_names: &HashMap<LocalId, Atom>,
    ctx: &mut PassContext,
) {
    if let HirExpr::Call { callee, .. } = expr
        && let HirCallee::Closure(id) = callee
    {
        if let Some(name) = closure_names.get(id) {
            *callee = HirCallee::Indirect(Box::new(HirExpr::Global {
                name: name.clone(),
                ty: TypeId::from_raw(0),
            }));
        } else {
            ctx.error(
                "P0005",
                "call to undeclared closure (lower_closures did not produce a fn for it)",
                Span::new(0, 0),
            );
        }
    }
    match expr {
        HirExpr::Closure { body, .. } => {
            rewrite_in_body(body, closure_names, ctx);
        }
        HirExpr::Call { callee, args, .. } => {
            if let HirCallee::Indirect(inner) = callee {
                rewrite_in_expr(inner, closure_names, ctx);
            }
            for a in args {
                rewrite_in_expr(a, closure_names, ctx);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_in_expr(lhs, closure_names, ctx);
            rewrite_in_expr(rhs, closure_names, ctx);
        }
        HirExpr::Unary { expr: e, .. } => rewrite_in_expr(e, closure_names, ctx),
        HirExpr::Field { owner, .. } => rewrite_in_expr(owner, closure_names, ctx),
        HirExpr::Index { owner, index, .. } => {
            rewrite_in_expr(owner, closure_names, ctx);
            rewrite_in_expr(index, closure_names, ctx);
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                rewrite_in_expr(e, closure_names, ctx);
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                rewrite_in_expr(e, closure_names, ctx);
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                let value = match f {
                    ObjectLiteralField::Property { value, .. } => value,
                    ObjectLiteralField::Spread(value) => value,
                };
                rewrite_in_expr(value, closure_names, ctx);
            }
        }
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_in_expr(cond, closure_names, ctx);
            rewrite_in_expr(then_branch, closure_names, ctx);
            rewrite_in_expr(else_branch, closure_names, ctx);
        }
        HirExpr::Sequence { exprs, .. } => {
            for e in exprs {
                rewrite_in_expr(e, closure_names, ctx);
            }
        }
        HirExpr::Await { expr: e, .. } | HirExpr::TypeAssertion { expr: e, .. } => {
            rewrite_in_expr(e, closure_names, ctx);
        }
        HirExpr::Yield { expr: Some(e), .. } => rewrite_in_expr(e, closure_names, ctx),
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag {
                rewrite_in_expr(t, closure_names, ctx);
            }
            for p in expressions {
                rewrite_in_expr(p, closure_names, ctx);
            }
        }
        HirExpr::New { callee, args, .. } => {
            rewrite_in_expr(callee, closure_names, ctx);
            for a in args {
                rewrite_in_expr(a, closure_names, ctx);
            }
        }
        HirExpr::OptionalChain { base, .. } => rewrite_in_expr(base, closure_names, ctx),
        HirExpr::Assignment { target, value, .. } => {
            rewrite_in_expr(target, closure_names, ctx);
            rewrite_in_expr(value, closure_names, ctx);
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            rewrite_in_expr(target, closure_names, ctx);
            rewrite_in_expr(rhs, closure_names, ctx);
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
        | HirExpr::RegExp { .. } => {}
    }
}
