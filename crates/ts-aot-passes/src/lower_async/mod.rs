use ts_aot_core::{Atom, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirProgram};

mod expr;
#[cfg(test)]
mod tests;
mod walk_stmt;

use crate::PassContext;
use expr::rewrite_expr;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerAsyncStats {
    pub inlined_promise_resolve: usize,
    pub cleared_async_info: usize,
}

pub fn lower_async(
    program: &mut HirProgram,
    _types: &mut TypeTable,
    _ctx: &mut PassContext,
) -> LowerAsyncStats {
    let promise_sym = Atom::from("Promise");
    let resolve_sym = Atom::from("resolve");

    let decl_shadows_promise = |d: &HirDecl| -> bool {
        match d {
            HirDecl::Function(f) => f.name == promise_sym,
            HirDecl::Class(c) => c.name == promise_sym,
            HirDecl::Enum { name, .. }
            | HirDecl::Namespace { name, .. }
            | HirDecl::Global { name, .. } => *name == promise_sym,
            HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } => false,
        }
    };

    let user_shadows_promise_builtin = program.declarations.iter().any(decl_shadows_promise)
        || program
            .imports
            .iter()
            .any(|imp| imp.alias.clone().unwrap_or_else(|| imp.name.clone()) == promise_sym);
    let can_rewrite_promise_resolve = !user_shadows_promise_builtin;

    let mut stats = LowerAsyncStats::default();

    for decl in &mut program.declarations {
        rewrite_decl(
            decl,
            &promise_sym,
            &resolve_sym,
            can_rewrite_promise_resolve,
            &mut stats,
        );
    }

    stats
}

pub(super) fn rewrite_decl(
    decl: &mut HirDecl,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    match decl {
        HirDecl::Function(f) => {
            walk_stmt::rewrite_body(
                &mut f.body,
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve && f.name != promise_sym,
                stats,
            );
            if f.async_info.take().is_some() {
                stats.cleared_async_info += 1;
            }
        }
        HirDecl::Class(c) => {
            let can_rewrite_in_class = can_rewrite_promise_resolve && c.name != promise_sym;
            for method in &mut c.methods {
                walk_stmt::rewrite_body(
                    &mut method.body,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_in_class,
                    stats,
                );
                if method.async_info.take().is_some() {
                    stats.cleared_async_info += 1;
                }
            }
        }
        HirDecl::Global { init, .. } => {
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
        HirDecl::Namespace { name, members, .. } => {
            let can_rewrite_in_ns = can_rewrite_promise_resolve && *name != promise_sym;
            for m in members {
                rewrite_decl(m, promise_sym, resolve_sym, can_rewrite_in_ns, stats);
            }
        }
        HirDecl::Enum { .. } | HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } => {}
    }
}
