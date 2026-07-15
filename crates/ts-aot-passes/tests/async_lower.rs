use ts_aot_core::{Atom, ModuleId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirExpr, MirStmt};
use ts_aot_passes::{PassContext, convert_program, lower_async};

fn await_promise_resolve_call(
    arg: HirExpr,
    arg_ty: ts_aot_core::TypeId,
    types: &mut TypeTable,
) -> HirExpr {
    let promise_sym = Atom::new_inline("Promise");
    let resolve_sym = Atom::new_inline("resolve");
    let promise_ty = types.intern(&ts_aot_core::Type::I64);
    HirExpr::Await {
        expr: Box::new(HirExpr::Call {
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
        }),
        ty: arg_ty,
    }
}

#[test]
fn end_to_end_lower_async_strips_promise_resolve_but_keeps_mir_await() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let typed_id = types.intern(&ts_aot_core::Type::I64);
    let fn_name = Atom::new_inline("greet");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: typed_id,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(await_promise_resolve_call(
                HirExpr::Int(42),
                typed_id,
                &mut types,
            )),
        }],
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: Some(ts_aot_ir_hir::HirAsyncInfo::Promise {
            ok_ty: typed_id,
            err_ty: None,
            promise_ty: typed_id,
        }),
    }));

    let stats = lower_async(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.inlined_promise_resolve, 1);
    assert_eq!(stats.cleared_async_info, 1);

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "lower_async + convert_program must not error for async_lower.rs:65, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::Await { expr: promise, .. })) = &f.body.block.stmts[0] else {
        panic!(
            "expected Return(Some(MirExpr::Await)) at stmts[0], got stmts: {:?}",
            f.body.block.stmts
        );
    };
    let MirExpr::Int { value, .. } = promise.as_ref() else {
        panic!(
            "MirExpr::Await.expr must now be the bare Int(42) (Promise.resolve call was stripped), got {promise:?}"
        );
    };
    assert_eq!(*value, 42, "bare arg must be preserved through HIR -> MIR");
}

#[test]
fn end_to_end_lower_async_keeps_non_promise_resolve_await_as_mir_state() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let typed_id = types.intern(&ts_aot_core::Type::I64);
    let fn_name = Atom::new_inline("waitFor");
    let callee_sym = Atom::new_inline("realPromise");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: typed_id,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Await {
                expr: Box::new(HirExpr::Call {
                    callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                        name: callee_sym,
                        ty: typed_id,
                    })),
                    args: Vec::new(),
                    ty: typed_id,
                }),
                ty: typed_id,
            }),
        }],
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: Some(ts_aot_ir_hir::HirAsyncInfo::Promise {
            ok_ty: typed_id,
            err_ty: None,
            promise_ty: typed_id,
        }),
    }));

    let stats = lower_async(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.inlined_promise_resolve, 0);
    assert_eq!(
        stats.cleared_async_info, 1,
        "still clears async_info on the function"
    );

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "lower_async + convert_program must not error for async_lower.rs:124, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::Await {
        expr: call_expr, ..
    })) = &f.body.block.stmts[0]
    else {
        panic!(
            "expected Return(Some(MirExpr::Await)) at stmts[0], got stmts: {:?}",
            f.body.block.stmts
        );
    };
    let MirExpr::IndirectCall { callee, args, .. } = call_expr.as_ref() else {
        panic!("Await.expr must now be MirExpr::IndirectCall, got {call_expr:?}");
    };
    let MirExpr::Global(callee_sym) = callee.as_ref() else {
        panic!("IndirectCall.callee must be MirExpr::Global, got {callee:?}");
    };
    assert_eq!(callee_sym.as_str(), "realPromise");
    assert!(args.is_empty());
}
