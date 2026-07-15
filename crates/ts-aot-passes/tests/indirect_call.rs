use ts_aot_core::{Atom, FunctionId, ModuleId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirExpr, MirStmt};
use ts_aot_passes::{PassContext, convert_program};

#[test]
fn convert_program_unresolved_global_name_falls_through_to_placeholder() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let fn_name = Atom::new_inline("caller");
    let user_global_name = Atom::new_inline("realFn");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name.clone(),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: user_global_name.clone(),
                    ty: i64_ty,
                })),
                args: vec![HirExpr::Int(1)],
                ty: i64_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one fn");

    let MirStmt::Expr(MirExpr::IndirectCall { callee, args, .. }) = &f.body.block.stmts[0] else {
        panic!(
            "expected MirStmt::Expr(MirExpr::IndirectCall) at stmts[0], got {:?}",
            f.body.block.stmts[0]
        );
    };
    let MirExpr::Global(callee_name) = callee.as_ref() else {
        panic!(
            "IndirectCall.callee must be the MirExpr::Global carrying the unresolved name, got {callee:?}"
        );
    };
    assert_eq!(
        callee_name.as_str(),
        user_global_name.as_str(),
        "callee value must be the unresolved global name; got {callee_name:?}"
    );
    assert_eq!(
        args.len(),
        1,
        "IndirectCall.args must be the original call args; got {} args",
        args.len()
    );
    assert!(
        !ctx.has_errors(),
        "PR 1.4: P0005 for unresolved indirect callee is downgraded to warning (IndirectCall emit handles it), so has_errors() must be false"
    );
    let p0005_count = ctx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .count();
    assert_eq!(
        p0005_count, 1,
        "P0005 must still be emitted as a warning, got {p0005_count} diags"
    );
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005");
    assert!(
        diag.message.contains("indirect"),
        "P0005 message should mention 'indirect', got: {}",
        diag.message
    );
}

#[test]
fn indirect_call_to_resolved_global_uses_direct_call_not_runtime_fallback() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let fn_name = Atom::new_inline("caller");
    let resolved_name = Atom::new_inline("add");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: resolved_name,
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: i64_ty,
        }],
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name.clone(),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline("add"),
                    ty: i64_ty,
                })),
                args: vec![HirExpr::Int(1)],
                ty: i64_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir
        .functions()
        .find(|f| f.name == fn_name)
        .expect("caller must exist");
    let MirStmt::Expr(MirExpr::Call { callee, args, .. }) = &f.body.block.stmts[0] else {
        panic!(
            "resolved Indirect(Global) must still produce MirExpr::Call, got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert_eq!(
        *callee,
        FunctionId::from_raw(0),
        "Indirect(Global(\"add\")) must resolve to FunctionId(0); got {callee:?}"
    );
    assert_eq!(args.len(), 1, "direct call must preserve original args");
    assert!(
        !ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0005"),
        "resolved Indirect(Global) must not emit P0005; got {:?}",
        ctx.diagnostics()
    );
    assert!(
        !ctx.has_errors(),
        "resolved Indirect(Global) must not produce error diagnostics; got {:?}",
        ctx.diagnostics()
    );
}

#[test]
fn indirect_call_to_non_global_callee_emits_p0005_warning_for_runtime_dispatch_fallback() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let fn_name = Atom::new_inline("caller");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Binary {
                    op: ts_aot_ir_hir::HirBinaryOp::Add,
                    lhs: Box::new(HirExpr::Global {
                        name: Atom::new_inline("x"),
                        ty: i64_ty,
                    }),
                    rhs: Box::new(HirExpr::Int(1)),
                    ty: i64_ty,
                })),
                args: vec![HirExpr::Int(2)],
                ty: i64_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let _ = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "must keep compilation alive (warning, not error) for non-Global callee; got {:?}",
        ctx.diagnostics()
    );
    let fallback_warnings: Vec<_> = ctx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005" && d.message.contains("MirExpr::IndirectCall"))
        .collect();
    assert_eq!(
        fallback_warnings.len(),
        1,
        "non-Global indirect callee must emit exactly one P0005 warning pointing at MirExpr::IndirectCall, got {fallback_warnings:?}"
    );
}
