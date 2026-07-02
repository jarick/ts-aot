use std::collections::HashMap;

use ts_aot_core::Atom;
use ts_aot_ir_hir::{HirCallee, HirExpr};

pub(super) fn rewrite_expr(expr: &mut HirExpr, map: &HashMap<(Atom, Atom), Atom>) {
    match expr {
        HirExpr::Field {
            owner,
            field_name,
            ty,
            ..
        } => {
            let enum_name = match owner.as_ref() {
                HirExpr::Global { name, .. } => Some(name.clone()),
                _ => None,
            };
            if let Some(enum_name) = enum_name
                && let Some(namespaced) = map.get(&(enum_name, field_name.clone()))
            {
                *expr = HirExpr::Global {
                    name: namespaced.clone(),
                    ty: *ty,
                };
            } else {
                rewrite_expr(owner, map);
            }
        }
        HirExpr::Call { callee, args, .. } => {
            rewrite_callee(callee, map);
            for a in args {
                rewrite_expr(a, map);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            rewrite_expr(lhs, map);
            rewrite_expr(rhs, map);
        }
        HirExpr::Unary { expr: e, .. } => rewrite_expr(e, map),
        HirExpr::Index { owner, index, .. } => {
            rewrite_expr(owner, map);
            rewrite_expr(index, map);
        }
        HirExpr::Assignment { target, value, .. } => {
            rewrite_expr(target, map);
            rewrite_expr(value, map);
        }
        HirExpr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                rewrite_expr(e, map);
            }
        }
        HirExpr::ArrayLiteral { elements, .. } => {
            for e in elements {
                rewrite_expr(e, map);
            }
        }
        HirExpr::Closure { captures, .. } => {
            for c in captures {
                rewrite_expr(c, map);
            }
        }
        HirExpr::Await { expr, .. } => rewrite_expr(expr, map),
        HirExpr::Yield { expr, .. } => {
            if let Some(e) = expr.as_mut() {
                rewrite_expr(e, map);
            }
        }
        HirExpr::Template { tag, parts, .. } => {
            if let Some(t) = tag.as_mut() {
                rewrite_expr(t, map);
            }
            for p in parts {
                rewrite_expr(p, map);
            }
        }
        HirExpr::New { callee, args, .. } => {
            rewrite_expr(callee, map);
            for a in args {
                rewrite_expr(a, map);
            }
        }
        HirExpr::OptionalChain { base, .. } => rewrite_expr(base, map),
        HirExpr::TypeAssertion { expr, .. } => rewrite_expr(expr, map),
        HirExpr::Global { .. }
        | HirExpr::Local { .. }
        | HirExpr::Unit
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Null
        | HirExpr::Undefined => {}
    }
}

fn rewrite_callee(callee: &mut HirCallee, map: &HashMap<(Atom, Atom), Atom>) {
    match callee {
        HirCallee::Function(_) | HirCallee::Closure(_) | HirCallee::Runtime { .. } => {}
        HirCallee::Indirect(expr) => rewrite_expr(expr, map),
    }
}
