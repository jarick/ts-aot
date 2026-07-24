use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, Span, TypeId};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirStmt};

use super::LowerClosuresStats;
use crate::PassContext;

pub(super) fn fresh_closure_name(next_id: u32) -> Atom {
    Atom::from(format!("__ts_aot_closure_{next_id}"))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn next_unused_closure_name(next_id: &mut u32, taken: &HashSet<Atom>) -> Atom {
    let mut name = fresh_closure_name(*next_id);
    while taken.contains(&name) {
        *next_id += 1;
        name = fresh_closure_name(*next_id);
    }
    name
}

pub(super) fn build_closure_fn_decl(
    name: &Atom,
    params: &[HirParam],
    body: &[HirStmt],
    ret: TypeId,
) -> HirDecl {
    HirDecl::Function(HirFunction {
        name: name.clone(),
        params: params.to_vec(),
        ret,
        throws: None,
        body: body.to_vec(),
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lift_non_capturing_closure(
    id: LocalId,
    params: &[HirParam],
    body: &mut [HirStmt],
    ty: TypeId,
    span: Span,
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    walk_body: WalkBodyFn,
    ctx: &mut PassContext,
) -> HirExpr {
    walk_body(
        body,
        next_id,
        closure_names,
        new_decls,
        generated,
        taken,
        stats,
        ctx,
    );
    super::rewrite::rewrite_in_body(body, closure_names, ctx);

    let name = next_unused_closure_name(next_id, taken);
    *next_id += 1;
    taken.insert(name.clone());

    let fn_decl = build_closure_fn_decl(&name, params, body, ty);
    new_decls.push(fn_decl);
    closure_names.insert(id, name.clone());
    generated.push(name.clone());
    stats.emitted_fns += 1;

    HirExpr::Global { name, ty, span }
}

pub(super) type WalkBodyFn = fn(
    body: &mut [HirStmt],
    next_id: &mut u32,
    closure_names: &mut HashMap<LocalId, Atom>,
    new_decls: &mut Vec<HirDecl>,
    generated: &mut Vec<Atom>,
    taken: &mut HashSet<Atom>,
    stats: &mut LowerClosuresStats,
    ctx: &mut PassContext,
);

pub(super) fn closure_callee_ty(new_decls: &[HirDecl], name: &Atom) -> TypeId {
    for d in new_decls {
        if let HirDecl::Function(f) = d
            && f.name == *name
        {
            return f.ret;
        }
    }
    TypeId::from_raw(0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rewrite_closure_callee(
    callee: &mut HirCallee,
    span: Span,
    closure_names: &HashMap<LocalId, Atom>,
    new_decls: &[HirDecl],
    ctx: &mut PassContext,
) {
    if let HirCallee::Closure(id) = callee
        && let Some(name) = closure_names.get(id)
    {
        let ty = closure_callee_ty(new_decls, name);
        *callee = HirCallee::Indirect(Box::new(HirExpr::Global {
            name: name.clone(),
            ty,
            span,
        }));
        return;
    }
    ctx.error(
        "P0005",
        "call to undeclared closure (lower_closures did not produce a fn for it)",
        ts_aot_core::Span::new(0, 0),
    );
}
