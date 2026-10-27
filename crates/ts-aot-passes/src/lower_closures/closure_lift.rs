use std::collections::{HashMap, HashSet};

use ts_aot_core::{Atom, LocalId, Span, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirStmt};

use super::ClosureLowerer;
use crate::PassContext;

pub(super) fn fresh_closure_name(next_id: u32) -> Atom {
    Atom::from(format!("__ts_aot_closure_{next_id}"))
}
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

impl ClosureLowerer<'_> {
    pub(super) fn lift_non_capturing_closure(
        &mut self,
        id: LocalId,
        params: &[HirParam],
        body: &mut [HirStmt],
        ty: TypeId,
        span: Span,
    ) -> HirExpr {
        self.walk_body(body);
        super::rewrite::rewrite_in_body(body, &self.closure_names, self.ctx);

        let name = next_unused_closure_name(self.next_id, self.taken);
        *self.next_id += 1;
        self.taken.insert(name.clone());

        let fn_decl =
            build_closure_fn_decl(&name, params, body, ret_ty_from_fn_type(ty, self.types));
        self.new_decls.push(fn_decl);
        self.closure_names.insert(id, name.clone());
        self.generated.push(name.clone());
        self.stats.emitted_fns += 1;

        HirExpr::Global { name, ty, span }
    }
}

fn ret_ty_from_fn_type(ty: TypeId, types: &TypeTable) -> TypeId {
    if let Some(Type::Fn { ret, .. }) = types.resolve(ty) {
        return *ret;
    }
    ty
}

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
    ctx.warning(
        "P0005",
        "call to undeclared closure (lower_closures did not produce a fn for it)",
        ts_aot_core::Span::new(0, 0),
    );
}
