use ts_aot_core::Atom;
use ts_aot_ir_hir::{HirCallee, HirExpr, ObjectLiteralField};

use super::LowerAsyncStats;

pub(super) fn rewrite_expr(
    expr: &mut HirExpr,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    loop {
        if try_inline_promise_resolve(
            expr,
            promise_sym,
            resolve_sym,
            can_rewrite_promise_resolve,
            stats,
        ) {
            continue;
        }
        recurse_subexprs(
            expr,
            promise_sym,
            resolve_sym,
            can_rewrite_promise_resolve,
            stats,
        );
        return;
    }
}

fn try_inline_promise_resolve(
    expr: &mut HirExpr,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) -> bool {
    if !can_rewrite_promise_resolve {
        return false;
    }

    let HirExpr::Await { expr: inner, .. } = expr else {
        return false;
    };

    let HirExpr::Call { callee, args, .. } = inner.as_mut() else {
        return false;
    };

    if args.len() != 1 {
        return false;
    }

    let HirCallee::Indirect(field_expr) = callee else {
        return false;
    };

    let HirExpr::Field {
        owner, field_name, ..
    } = field_expr.as_mut()
    else {
        return false;
    };

    if *field_name != resolve_sym {
        return false;
    }

    let HirExpr::Global { name, .. } = owner.as_mut() else {
        return false;
    };

    if *name != promise_sym {
        return false;
    }

    let arg = args.pop().expect("validated args.len() == 1");
    **inner = arg;
    stats.inlined_promise_resolve += 1;
    true
}

fn recurse_subexprs(
    expr: &mut HirExpr,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    match expr {
        HirExpr::Await { expr: inner, .. } => {
            rewrite_expr(
                inner.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Field { owner, .. } => {
            rewrite_expr(
                owner.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Index { owner, index, .. } => {
            rewrite_expr(
                owner.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_expr(
                index.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Call { callee, args, .. } => {
            match callee {
                HirCallee::Indirect(e) => {
                    rewrite_expr(
                        e.as_mut(),
                        promise_sym,
                        resolve_sym,
                        can_rewrite_promise_resolve,
                        stats,
                    );
                }
                HirCallee::Function(_) | HirCallee::Closure(_) | HirCallee::Runtime { .. } => {}
            }
            for arg in args {
                rewrite_expr(
                    arg,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_expr(
                lhs.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_expr(
                rhs.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Unary { expr: e, .. } => {
            rewrite_expr(
                e.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                rewrite_expr(
                    e,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                rewrite_expr(
                    e,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::ObjectLiteral { fields, .. } => {
            for f in fields {
                let value = match f {
                    ObjectLiteralField::Property { value, .. } => value,
                    ObjectLiteralField::Spread(value) => value,
                };
                rewrite_expr(
                    value,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::Closure { captures, .. } => {
            for c in captures {
                rewrite_expr(
                    c,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::Yield { expr: Some(e), .. } => {
            rewrite_expr(
                e.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Yield { expr: None, .. } => {}
        HirExpr::Template {
            tag, expressions, ..
        } => {
            if let Some(t) = tag.as_mut() {
                rewrite_expr(
                    t.as_mut(),
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
            for p in expressions {
                rewrite_expr(
                    p,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::New { callee, args, .. } => {
            rewrite_expr(
                callee.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            for a in args {
                rewrite_expr(
                    a,
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
        }
        HirExpr::OptionalChain { base, .. } => {
            rewrite_expr(
                base.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::TypeAssertion { expr: e, .. } => {
            rewrite_expr(
                e.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::Assignment { target, value, .. } => {
            rewrite_expr(
                target.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_expr(
                value.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
        }
        HirExpr::CompoundUpdate { target, rhs, .. } => {
            rewrite_expr(
                target.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
            );
            rewrite_expr(
                rhs.as_mut(),
                promise_sym,
                resolve_sym,
                can_rewrite_promise_resolve,
                stats,
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
        | HirExpr::Global { .. } => {}
    }
}
