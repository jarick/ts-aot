use ts_aot_core::{Atom, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirProgram, HirStmt};

use crate::PassContext;

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

fn rewrite_decl(
    decl: &mut HirDecl,
    promise_sym: &Atom,
    resolve_sym: &Atom,
    can_rewrite_promise_resolve: bool,
    stats: &mut LowerAsyncStats,
) {
    match decl {
        HirDecl::Function(f) => {
            rewrite_body(
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
                rewrite_body(
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

fn rewrite_body(
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

fn rewrite_stmt(
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

fn rewrite_expr(
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
        HirExpr::Template { tag, parts, .. } => {
            if let Some(t) = tag.as_mut() {
                rewrite_expr(
                    t.as_mut(),
                    promise_sym,
                    resolve_sym,
                    can_rewrite_promise_resolve,
                    stats,
                );
            }
            for p in parts {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::{Atom, LocalId, ModuleId, TypeId};
    use ts_aot_ir_hir::{HirFunction, HirParam};

    fn fixture() -> (TypeTable, PassContext) {
        (TypeTable::new(), PassContext::new())
    }

    fn i64_type_id(types: &mut TypeTable) -> TypeId {
        types.intern(&ts_aot_core::Type::I64)
    }

    fn promise_resolve_call(arg: HirExpr, arg_ty: TypeId, types: &mut TypeTable) -> HirExpr {
        let promise_sym = Atom::new_inline("Promise");
        let resolve_sym = Atom::new_inline("resolve");
        let promise_ty = types.intern(&ts_aot_core::Type::Promise {
            ok: arg_ty,
            err: None,
        });
        HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: promise_sym,
                    ty: promise_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: resolve_sym,
                ty: promise_ty,
            })),
            args: vec![arg],
            ty: promise_ty,
        }
    }

    fn await_promise_resolve(arg: HirExpr, arg_ty: TypeId, types: &mut TypeTable) -> HirExpr {
        HirExpr::Await {
            expr: Box::new(promise_resolve_call(arg, arg_ty, types)),
            ty: arg_ty,
        }
    }

    fn body_returning(expr: HirExpr) -> HirFunction {
        HirFunction {
            name: Atom::new_inline("u32::MAX"),
            params: Vec::<HirParam>::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: vec![HirStmt::Return { value: Some(expr) }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }
    }

    fn last_return_expr(body: &[HirStmt]) -> &HirExpr {
        match body.last().expect("body is not empty") {
            HirStmt::Return { value: Some(expr) } => expr,
            other => panic!("expected Return(Some), got {other:?}"),
        }
    }

    fn build_program(decl: HirDecl) -> HirProgram {
        let mut p = HirProgram::new(ModuleId::from_raw(0));
        p.declarations.push(decl);
        p
    }

    #[test]
    fn rewrites_await_promise_resolve_literal_to_await_arg() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let f = body_returning(await_promise_resolve(
            HirExpr::Int(42),
            typed_id,
            &mut types,
        ));
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
        assert_eq!(stats.cleared_async_info, 0);
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        let HirExpr::Await { expr: inner, .. } = last_return_expr(&f.body) else {
            panic!(
                "Await wrapper must be preserved (Promise.resolve call is removed, but await stays), got {:?}",
                last_return_expr(&f.body)
            );
        };
        assert!(
            matches!(&**inner, HirExpr::Int(42)),
            "Await's inner expr must now be the bare Int(42), got {inner:?}"
        );
    }

    #[test]
    fn rewrites_await_promise_resolve_binary_expr_to_await_arg() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let inner = HirExpr::Binary {
            op: ts_aot_ir_hir::HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1)),
            rhs: Box::new(HirExpr::Int(2)),
            ty: typed_id,
        };
        let f = body_returning(await_promise_resolve(inner, typed_id, &mut types));
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        let HirExpr::Await { expr: inner, .. } = last_return_expr(&f.body) else {
            panic!("expected await wrapper preserved around the binary expression");
        };
        assert!(
            matches!(&**inner, HirExpr::Binary { .. }),
            "Await's inner expr must now be the bare Binary expression, got {inner:?}"
        );
    }

    #[test]
    fn does_not_inline_await_of_other_call() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let callee_sym = Atom::new_inline("otherFn");
        let callee = HirExpr::Global {
            name: callee_sym,
            ty: typed_id,
        };
        let non_promise_call = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(callee)),
            args: vec![HirExpr::Int(7)],
            ty: typed_id,
        };
        let await_other = HirExpr::Await {
            expr: Box::new(non_promise_call),
            ty: typed_id,
        };
        let f = body_returning(await_other);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        assert!(matches!(last_return_expr(&f.body), HirExpr::Await { .. }));
    }

    #[test]
    fn does_not_inline_promise_reject_or_then() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let promise_sym = Atom::new_inline("Promise");
        let reject_sym = Atom::new_inline("reject");
        let promise_ty = typed_id;
        let reject_call = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: promise_sym,
                    ty: promise_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: reject_sym,
                ty: promise_ty,
            })),
            args: vec![HirExpr::Int(0)],
            ty: promise_ty,
        };
        let await_reject = HirExpr::Await {
            expr: Box::new(reject_call),
            ty: typed_id,
        };
        let f = body_returning(await_reject);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
    }

    #[test]
    fn does_not_inline_await_promise_resolve_without_args() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let promise_sym = Atom::new_inline("Promise");
        let resolve_sym = Atom::new_inline("resolve");
        let promise_ty = typed_id;
        let zero_args = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: promise_sym,
                    ty: promise_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: resolve_sym,
                ty: promise_ty,
            })),
            args: Vec::new(),
            ty: promise_ty,
        };
        let await_zero_args = HirExpr::Await {
            expr: Box::new(zero_args),
            ty: typed_id,
        };
        let f = body_returning(await_zero_args);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
    }

    #[test]
    fn does_not_inline_await_promise_resolve_with_two_args() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let two_args_call = promise_resolve_call(HirExpr::Int(1), typed_id, &mut types);
        let extra_arg = HirExpr::Int(2);
        let augmented = match two_args_call {
            HirExpr::Call {
                callee,
                mut args,
                ty,
            } => {
                args.push(extra_arg);
                HirExpr::Call { callee, args, ty }
            }
            other => panic!("expected Call, got {other:?}"),
        };
        let await_two = HirExpr::Await {
            expr: Box::new(augmented),
            ty: typed_id,
        };
        let f = body_returning(await_two);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
    }

    #[test]
    fn pass_is_idempotent() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = build_program(HirDecl::Function(body_returning(await_promise_resolve(
            HirExpr::Int(99),
            typed_id,
            &mut types,
        ))));

        let stats_first = lower_async(&mut program, &mut types, &mut ctx);
        let stats_second = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats_first.inlined_promise_resolve, 1);
        assert_eq!(stats_second.inlined_promise_resolve, 0);
    }

    #[test]
    fn nested_await_promise_resolve_keeps_both_awaits() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let inner_await = HirExpr::Await {
            expr: Box::new(promise_resolve_call(HirExpr::Int(7), typed_id, &mut types)),
            ty: typed_id,
        };
        let outer = await_promise_resolve(inner_await, typed_id, &mut types);
        let f = body_returning(outer);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 2,
            "both inner and outer await Promise.resolve should be rewritten"
        );
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        let HirExpr::Await {
            expr: outer_inner, ..
        } = last_return_expr(&f.body)
        else {
            panic!("outer Await must be preserved");
        };
        let HirExpr::Await {
            expr: inner_inner, ..
        } = &**outer_inner
        else {
            panic!(
                "inner Await must be preserved (Promise.resolve call inside it was just rewritten to bare arg), got {outer_inner:?}"
            );
        };
        assert!(
            matches!(&**inner_inner, HirExpr::Int(7)),
            "innermost expression must now be Int(7) (the bare arg), got {inner_inner:?}"
        );
    }

    #[test]
    fn preserves_await_when_arg_is_promise_typed_local() {
        let (mut types, mut ctx) = fixture();
        let type_id = i64_type_id(&mut types);
        let promise_string_ty = types.intern(&ts_aot_core::Type::Promise {
            ok: type_id,
            err: None,
        });
        let local_id = LocalId::from_raw(0);
        let _ = Atom::new_inline("p");
        let p_local = HirExpr::Local {
            id: local_id,
            ty: promise_string_ty,
        };
        let f = HirFunction {
            name: Atom::new_inline("__test_fn__"),
            params: Vec::<HirParam>::new(),
            ret: type_id,
            throws: None,
            body: vec![HirStmt::Let {
                id: LocalId::from_raw(1),
                name: Atom::new_inline("x"),
                ty: type_id,
                init: Some(await_promise_resolve(p_local, type_id, &mut types)),
            }],
            is_async: true,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "Promise.resolve call must still be stripped even for thenable-typed args"
        );
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        let HirStmt::Let {
            init: Some(init), ..
        } = &f.body[0]
        else {
            panic!("expected Let with init");
        };
        let HirExpr::Await { expr: inner, .. } = init else {
            panic!(
                "P1 regression: Await wrapper must be PRESERVED when arg is Promise-typed; \
                 lowering `let x = await Promise.resolve(p)` to `let x = p` would change \
                 x's effective type from typed_id to Promise<typed_id>. got {init:?}"
            );
        };
        assert!(
            matches!(&**inner, HirExpr::Local { id, .. } if *id == local_id),
            "Await's inner expr must now be the bare Local reference (p), got {inner:?}"
        );
    }

    #[test]
    fn clears_async_info_on_function_with_async_info() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut f = body_returning(await_promise_resolve(HirExpr::Int(5), typed_id, &mut types));
        f.async_info = Some(ts_aot_ir_hir::HirAsyncInfo::Promise {
            ok_ty: typed_id,
            err_ty: None,
            promise_ty: typed_id,
        });
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.cleared_async_info, 1);
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        assert!(f.async_info.is_none());
    }

    #[test]
    fn clears_async_info_on_class_method() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut method = body_returning(await_promise_resolve(
            HirExpr::Int(11),
            typed_id,
            &mut types,
        ));
        method.async_info = Some(ts_aot_ir_hir::HirAsyncInfo::Promise {
            ok_ty: typed_id,
            err_ty: None,
            promise_ty: typed_id,
        });
        let class = ts_aot_ir_hir::HirClass {
            name: Atom::new_inline("C"),
            ty: types.intern(&ts_aot_core::Type::I64),
            fields: Vec::new(),
            methods: vec![method],
            extends: None,
            type_params: Vec::new(),
        };
        let mut program = build_program(HirDecl::Class(class));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
        assert_eq!(stats.cleared_async_info, 1);
        let HirDecl::Class(c) = &program.declarations[0] else {
            panic!("expected Class");
        };
        assert!(c.methods[0].async_info.is_none());
    }

    #[test]
    fn walks_let_init_expr() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let init = await_promise_resolve(HirExpr::Int(3), typed_id, &mut types);
        let f = HirFunction {
            name: Atom::new_inline("__test_fn__"),
            params: Vec::<HirParam>::new(),
            ret: typed_id,
            throws: None,
            body: vec![HirStmt::Let {
                id: LocalId::from_raw(0),
                name: Atom::new_inline("v"),
                ty: typed_id,
                init: Some(init),
            }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
    }

    #[test]
    fn walks_for_in_iter_expr() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let iter = await_promise_resolve(HirExpr::Int(1), typed_id, &mut types);
        let f = HirFunction {
            name: Atom::new_inline("__test_fn__"),
            params: Vec::<HirParam>::new(),
            ret: typed_id,
            throws: None,
            body: vec![HirStmt::ForIn {
                binding: LocalId::from_raw(0),
                iter,
                body: Box::new(HirStmt::Expr {
                    expr: HirExpr::Int(0),
                }),
            }],
            is_async: true,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "ForIn.iter must be rewritten even when iter is a Promise.resolve call, otherwise HIR->MIR still emits MirExpr::Await (no state-machine lowering)"
        );
        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected Function");
        };
        let HirStmt::ForIn { iter, .. } = &f.body[0] else {
            panic!("expected ForIn");
        };
        let HirExpr::Await { expr: inner, .. } = iter else {
            panic!(
                "ForIn.iter Await wrapper must be PRESERVED (just the Promise.resolve call inside is rewritten). \
                 Lowering `for (k in await Promise.resolve(x))` to `for (k in x)` would lose \
                 the await microtask hop. got {iter:?}"
            );
        };
        assert!(
            matches!(&**inner, HirExpr::Int(1)),
            "ForIn.iter's await's inner expr must now be the bare Int(1), got {inner:?}"
        );
    }

    #[test]
    fn walks_global_init_expr() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let init = await_promise_resolve(HirExpr::Int(13), typed_id, &mut types);
        let global = HirDecl::Global {
            name: Atom::new_inline("G"),
            ty: typed_id,
            init: Some(init),
        };
        let mut program = build_program(global);

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
    }

    #[test]
    fn walks_into_namespace_members() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let inner_init = await_promise_resolve(HirExpr::Int(17), typed_id, &mut types);
        let inner_fn = HirDecl::Function(HirFunction {
            name: Atom::new_inline("inner"),
            params: Vec::new(),
            ret: typed_id,
            throws: None,
            body: vec![HirStmt::Return {
                value: Some(inner_init),
            }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        });
        let ns = HirDecl::Namespace {
            name: Atom::new_inline("ns"),
            members: vec![inner_fn],
        };
        let mut program = build_program(ns);

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 1);
    }

    #[test]
    fn does_not_inline_when_owner_is_not_promise_global() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let other_global_sym = Atom::new_inline("MaybePromise");
        let resolve_sym = Atom::new_inline("resolve");
        let promise_ty = typed_id;
        let not_promise_call = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: other_global_sym,
                    ty: promise_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: resolve_sym,
                ty: promise_ty,
            })),
            args: vec![HirExpr::Int(0)],
            ty: promise_ty,
        };
        let await_other = HirExpr::Await {
            expr: Box::new(not_promise_call),
            ty: typed_id,
        };
        let f = body_returning(await_other);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
    }

    #[test]
    fn does_not_inline_when_field_name_is_other_method() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let promise_sym = Atom::new_inline("Promise");
        let then_sym = Atom::new_inline("then");
        let promise_ty = typed_id;
        let call = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: promise_sym,
                    ty: promise_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: then_sym,
                ty: promise_ty,
            })),
            args: vec![HirExpr::Int(0)],
            ty: promise_ty,
        };
        let await_then = HirExpr::Await {
            expr: Box::new(call),
            ty: typed_id,
        };
        let f = body_returning(await_then);
        let mut program = build_program(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(stats.inlined_promise_resolve, 0);
    }

    #[test]
    fn skips_when_user_declares_top_level_var_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Global {
            name: Atom::new_inline("Promise"),
            ty: typed_id,
            init: Some(HirExpr::Int(99)),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(1),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "user-declared `var Promise` shadows builtin; pass must skip rewriting"
        );
        let HirDecl::Function(f) = &program.declarations[1] else {
            panic!("expected Function at index 1");
        };
        assert!(
            matches!(last_return_expr(&f.body), HirExpr::Await { .. }),
            "await Promise.resolve(x) must be left intact when user shadows Promise"
        );
    }

    #[test]
    fn unrelated_top_level_var_does_not_block_rewrite() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Global {
            name: Atom::new_inline("Counter"),
            ty: typed_id,
            init: Some(HirExpr::Int(0)),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(13),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "unrelated `var Counter = ...` must not block Promise.resolve rewrite"
        );
    }

    #[test]
    fn skips_when_user_imports_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.imports.push(ts_aot_ir_hir::HirImport {
            module: Atom::new_inline("my-promise-lib"),
            name: Atom::new_inline("Promise"),
            alias: None,
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(11),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`import {{ Promise }} from ...` shadows builtin; pass must skip rewriting"
        );
    }

    #[test]
    fn skips_when_user_imports_promise_via_alias() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.imports.push(ts_aot_ir_hir::HirImport {
            module: Atom::new_inline("my-promise-lib"),
            name: Atom::new_inline("Promise"),
            alias: Some(Atom::new_inline("P")),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(HirExpr::Await {
                expr: Box::new(HirExpr::Call {
                    callee: ts_aot_ir_hir::HirCallee::Indirect(Box::new(HirExpr::Field {
                        owner: Box::new(HirExpr::Global {
                            name: Atom::new_inline("P"),
                            ty: typed_id,
                        }),
                        field: ts_aot_core::FieldId::from_raw(0),
                        field_name: Atom::new_inline("resolve"),
                        ty: typed_id,
                    })),
                    args: vec![HirExpr::Int(7)],
                    ty: typed_id,
                }),
                ty: typed_id,
            })));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`import {{ Promise as P }} from ...` shadows builtin via alias P; pass must skip rewriting"
        );
    }

    #[test]
    fn still_clears_async_info_when_promise_globally_shadowed() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut f = body_returning(await_promise_resolve(HirExpr::Int(5), typed_id, &mut types));
        f.is_async = true;
        f.async_info = Some(ts_aot_ir_hir::HirAsyncInfo::Promise {
            ok_ty: typed_id,
            err_ty: None,
            promise_ty: typed_id,
        });
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Global {
            name: Atom::new_inline("Promise"),
            ty: typed_id,
            init: Some(HirExpr::Int(7)),
        });
        program.declarations.push(HirDecl::Function(f));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "user-declared `var Promise` must still skip Promise.resolve rewrite"
        );
        assert_eq!(
            stats.cleared_async_info, 1,
            "shadowing Promise must NOT skip async_info clearing on async functions — \
             early-return would leave async_info uncleared (P2 regression guard)"
        );
        let HirDecl::Function(f) = &program.declarations[1] else {
            panic!("expected Function at index 1");
        };
        assert!(
            f.async_info.is_none(),
            "async_info must be cleared even when Promise.resolve rewrite is skipped"
        );
    }

    #[test]
    fn skips_when_user_declares_top_level_function_named_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Function(HirFunction {
            name: Atom::new_inline("Promise"),
            params: Vec::new(),
            ret: typed_id,
            throws: None,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }));
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(5),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`function Promise() {{}}` creates a value binding at module scope; must shadow builtin"
        );
    }

    #[test]
    fn skips_when_user_declares_top_level_class_named_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program
            .declarations
            .push(HirDecl::Class(ts_aot_ir_hir::HirClass {
                name: Atom::new_inline("Promise"),
                ty: typed_id,
                fields: Vec::new(),
                methods: Vec::new(),
                extends: None,
                type_params: Vec::new(),
            }));
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(7),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`class Promise {{}}` creates a constructor binding at module scope; must shadow builtin"
        );
    }

    #[test]
    fn skips_when_user_declares_top_level_namespace_named_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Namespace {
            name: Atom::new_inline("Promise"),
            members: Vec::new(),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(11),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`namespace Promise {{}}` creates a module-scope binding; must shadow builtin"
        );
    }

    #[test]
    fn skips_when_user_declares_top_level_enum_named_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Enum {
            name: Atom::new_inline("Promise"),
            variants: Vec::new(),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(3),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "`enum Promise {{}}` creates a value namespace; must shadow builtin"
        );
    }

    #[test]
    fn does_not_skip_for_top_level_type_alias_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::TypeAlias {
            name: Atom::new_inline("Promise"),
            target: typed_id,
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(13),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "`type Promise = ...` is type-only, does not create a runtime value; builtin Promise.resolve must still be rewritten"
        );
    }

    #[test]
    fn does_not_skip_for_top_level_interface_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Interface {
            name: Atom::new_inline("Promise"),
        });
        program
            .declarations
            .push(HirDecl::Function(body_returning(await_promise_resolve(
                HirExpr::Int(17),
                typed_id,
                &mut types,
            ))));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "`interface Promise {{}}` is type-only, does not create a runtime value; builtin Promise.resolve must still be rewritten"
        );
    }

    #[test]
    fn skips_when_nested_function_declares_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let promise_sym_inner = Atom::new_inline("Promise");
        let outer = HirFunction {
            name: Atom::new_inline("__test_fn_outer__"),
            params: Vec::<HirParam>::new(),
            ret: typed_id,
            throws: None,
            body: vec![HirStmt::Decl(HirDecl::Function(HirFunction {
                name: promise_sym_inner,
                params: Vec::<HirParam>::new(),
                ret: typed_id,
                throws: None,
                body: vec![HirStmt::Return {
                    value: Some(await_promise_resolve(HirExpr::Int(7), typed_id, &mut types)),
                }],
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            }))],
            is_async: true,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Function(outer));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        let HirDecl::Function(f) = &program.declarations[0] else {
            panic!("expected outer Function");
        };
        assert_eq!(
            stats.inlined_promise_resolve, 0,
            "nested function named Promise shadows builtin; inner body must not be rewritten"
        );
        assert!(
            matches!(f.body[0], HirStmt::Decl(_)),
            "outer body should still contain the nested function decl"
        );
    }

    #[test]
    fn does_not_skip_when_inner_function_does_not_shadow_promise() {
        let (mut types, mut ctx) = fixture();
        let typed_id = i64_type_id(&mut types);
        let outer = HirFunction {
            name: Atom::new_inline("__test_fn_outer__"),
            params: Vec::<HirParam>::new(),
            ret: typed_id,
            throws: None,
            body: vec![HirStmt::Decl(HirDecl::Function(HirFunction {
                name: Atom::new_inline("__test_fn_helper__"),
                params: Vec::<HirParam>::new(),
                ret: typed_id,
                throws: None,
                body: vec![HirStmt::Return {
                    value: Some(await_promise_resolve(
                        HirExpr::Int(11),
                        typed_id,
                        &mut types,
                    )),
                }],
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            }))],
            is_async: true,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        };
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations.push(HirDecl::Function(outer));

        let stats = lower_async(&mut program, &mut types, &mut ctx);

        assert_eq!(
            stats.inlined_promise_resolve, 1,
            "nested function with non-Promise name must NOT block Promise.resolve rewrite in its body"
        );
    }
}
