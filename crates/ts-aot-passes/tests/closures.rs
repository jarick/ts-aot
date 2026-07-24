use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Span, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirExpr, MirStmt};
use ts_aot_passes::{PassContext, convert_program, lower_closures};

#[test]
fn end_to_end_lower_closures_then_convert_program_resolves_indirect_global_callee() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);

    let outer_name = Atom::new_inline("outer");
    let cb_local = LocalId::from_raw(0);
    let outer = HirFunction {
        name: outer_name.clone(),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: cb_local,
                name: Atom::new_inline("add"),
                ty: i64_ty,
                init: Some(HirExpr::Closure {
                    span: Span::default(),
                    id: cb_local,
                    params: vec![HirParam {
                        name: Atom::new_inline("x"),
                        ty: i64_ty,
                    }],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            span: Span::default(),
                            id: LocalId::from_raw(1),
                            ty: i64_ty,
                        }),
                    }],
                    ty: i64_ty,
                }),
            },
            HirStmt::Expr {
                expr: HirExpr::Call {
                    span: Span::default(),
                    callee: HirCallee::Closure(cb_local),
                    args: vec![HirExpr::Int(7, Span::default())],
                    ty: i64_ty,
                },
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(outer));

    let result = lower_closures(&mut hir, &mut ctx);
    let stats = &result.stats;
    assert_eq!(stats.emitted_fns, 1);
    assert_eq!(stats.deferred_capturing, 0);
    assert!(
        !ctx.has_errors(),
        "lower_closures must not error on non-capturing closure"
    );

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not emit P0005 for rewritten closure call"
    );

    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(
        fns.len(),
        2,
        "expected outer + one hoisted closure fn, got {} fns",
        fns.len()
    );

    let hoisted = fns
        .iter()
        .find(|f| f.name.as_str().starts_with("__ts_aot_closure_"))
        .expect("expected hoisted __ts_aot_closure_N fn in MIR");

    let mut outer_mir: Option<&ts_aot_ir_mir::MirFunctionDecl> = None;
    let mut hoisted_mir: Option<&ts_aot_ir_mir::MirFunctionDecl> = None;
    for f in &fns {
        if f.id == hoisted.id {
            hoisted_mir = Some(f);
        } else {
            outer_mir = Some(f);
        }
    }
    let outer_mir = outer_mir.expect("outer fn present");
    let hoisted_id = hoisted_mir.unwrap().id;

    let MirStmt::Expr(MirExpr::Call {
        callee,
        args,
        ty: _,
    }) = &outer_mir.body.block.stmts[1]
    else {
        panic!(
            "outer fn second stmt must be Expr(Call) after lower_closures rewrite, got {:?}",
            outer_mir.body.block.stmts[1]
        );
    };
    assert_eq!(
        args.len(),
        1,
        "rewritten outer call must preserve the original single arg, got {args:?}"
    );
    let MirExpr::Int { value, ty: _ } = &args[0] else {
        panic!(
            "rewritten outer call's first arg must be the original Int(7) literal, got {:?}",
            args[0]
        );
    };
    assert_eq!(
        *value, 7,
        "rewritten outer call's first arg must preserve the original literal 7, got {value}"
    );
    assert_eq!(
        *callee, hoisted_id,
        "outer fn Call.callee must be the hoisted closure fn's FunctionId, got {callee:?}"
    );
}

#[test]
fn end_to_end_two_distinct_closures_both_lifted_and_resolvable() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);

    let outer_name = Atom::new_inline("outer");
    let add_local = LocalId::from_raw(0);
    let sub_local = LocalId::from_raw(1);
    let outer = HirFunction {
        name: outer_name.clone(),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: add_local,
                name: Atom::new_inline("add"),
                ty: i64_ty,
                init: Some(HirExpr::Closure {
                    span: Span::default(),
                    id: add_local,
                    params: vec![HirParam {
                        name: Atom::new_inline("x"),
                        ty: i64_ty,
                    }],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            span: Span::default(),
                            id: LocalId::from_raw(1),
                            ty: i64_ty,
                        }),
                    }],
                    ty: i64_ty,
                }),
            },
            HirStmt::Let {
                id: sub_local,
                name: Atom::new_inline("sub"),
                ty: i64_ty,
                init: Some(HirExpr::Closure {
                    span: Span::default(),
                    id: sub_local,
                    params: vec![HirParam {
                        name: Atom::new_inline("y"),
                        ty: i64_ty,
                    }],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            span: Span::default(),
                            id: LocalId::from_raw(2),
                            ty: i64_ty,
                        }),
                    }],
                    ty: i64_ty,
                }),
            },
            HirStmt::Expr {
                expr: HirExpr::Call {
                    span: Span::default(),
                    callee: HirCallee::Closure(add_local),
                    args: vec![HirExpr::Int(3, Span::default())],
                    ty: i64_ty,
                },
            },
            HirStmt::Expr {
                expr: HirExpr::Call {
                    span: Span::default(),
                    callee: HirCallee::Closure(sub_local),
                    args: vec![HirExpr::Int(2, Span::default())],
                    ty: i64_ty,
                },
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(outer));

    let result = lower_closures(&mut hir, &mut ctx);
    assert_eq!(
        result.stats.emitted_fns, 2,
        "two distinct closures must be lifted"
    );

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error when two distinct closures are both lifted"
    );

    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(fns.len(), 3, "outer + 2 hoisted closures");
    let mut hoisted_ids: Vec<FunctionId> = fns
        .iter()
        .filter(|f| f.name.as_str().starts_with("__ts_aot_closure_"))
        .map(|f| f.id)
        .collect();
    hoisted_ids.sort();
    hoisted_ids.dedup();
    assert_eq!(
        hoisted_ids.len(),
        2,
        "two distinct hoisted closures must have distinct FunctionIds, got {hoisted_ids:?}"
    );

    let outer_mir = fns
        .iter()
        .find(|f| !f.name.as_str().starts_with("__ts_aot_closure_"))
        .expect("outer fn present");
    let MirStmt::Expr(MirExpr::Call {
        callee: callee_add,
        args: args_add,
        ty: _,
    }) = &outer_mir.body.block.stmts[2]
    else {
        panic!(
            "outer fn third stmt must be Expr(Call) to first hoisted closure, got {:?}",
            outer_mir.body.block.stmts[2]
        );
    };
    let MirStmt::Expr(MirExpr::Call {
        callee: callee_sub,
        args: args_sub,
        ty: _,
    }) = &outer_mir.body.block.stmts[3]
    else {
        panic!(
            "outer fn fourth stmt must be Expr(Call) to second hoisted closure, got {:?}",
            outer_mir.body.block.stmts[3]
        );
    };
    assert_eq!(
        args_add.len(),
        1,
        "first hoisted-closure call must preserve the original single arg, got {args_add:?}"
    );
    let MirExpr::Int {
        value: value_add,
        ty: _,
    } = &args_add[0]
    else {
        panic!(
            "first hoisted-closure call's first arg must be the original Int(3) literal, got {:?}",
            args_add[0]
        );
    };
    assert_eq!(
        *value_add, 3,
        "first hoisted-closure call's first arg must preserve the original literal 3, got {value_add}"
    );
    assert_eq!(
        args_sub.len(),
        1,
        "second hoisted-closure call must preserve the original single arg, got {args_sub:?}"
    );
    let MirExpr::Int {
        value: value_sub,
        ty: _,
    } = &args_sub[0]
    else {
        panic!(
            "second hoisted-closure call's first arg must be the original Int(2) literal, got {:?}",
            args_sub[0]
        );
    };
    assert_eq!(
        *value_sub, 2,
        "second hoisted-closure call's first arg must preserve the original literal 2, got {value_sub}"
    );
    assert_ne!(
        callee_add, callee_sub,
        "the two outer calls must reach distinct hoisted closures, both got {callee_add:?}"
    );
    assert!(
        hoisted_ids.contains(callee_add),
        "outer first Call.callee must be a hoisted closure id, got {callee_add:?}, hoisted_ids={hoisted_ids:?}"
    );
    assert!(
        hoisted_ids.contains(callee_sub),
        "outer second Call.callee must be a hoisted closure id, got {callee_sub:?}, hoisted_ids={hoisted_ids:?}"
    );
}

#[test]
fn end_to_end_closure_passed_as_call_argument_is_lifted() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);

    let apply_name = Atom::new_inline("apply");
    let apply_fn = HirFunction {
        name: apply_name.clone(),
        params: vec![HirParam {
            name: Atom::new_inline("f"),
            ty: i64_ty,
        }],
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Local {
                span: Span::default(),
                id: LocalId::from_raw(1),
                ty: i64_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let outer_name = Atom::new_inline("outer");
    let cb_local = LocalId::from_raw(0);
    let outer = HirFunction {
        name: outer_name.clone(),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                span: Span::default(),
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: vec![HirExpr::Closure {
                    span: Span::default(),
                    id: cb_local,
                    params: vec![HirParam {
                        name: Atom::new_inline("x"),
                        ty: i64_ty,
                    }],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            span: Span::default(),
                            id: LocalId::from_raw(1),
                            ty: i64_ty,
                        }),
                    }],
                    ty: i64_ty,
                }],
                ty: i64_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(apply_fn));
    hir.declarations.push(HirDecl::Function(outer));

    let result = lower_closures(&mut hir, &mut ctx);
    assert_eq!(
        result.stats.emitted_fns, 1,
        "exactly one closure lifted (inlined as call argument)"
    );
    assert_eq!(result.stats.deferred_capturing, 0);

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error when closure is inlined as call argument"
    );

    let fns: Vec<_> = mir.functions().collect();
    let hoisted_count = fns
        .iter()
        .filter(|f| f.name.as_str().starts_with("__ts_aot_closure_"))
        .count();
    assert_eq!(
        hoisted_count, 1,
        "inlined closure must be hoisted exactly once, got {hoisted_count}"
    );

    let outer_mir = fns
        .iter()
        .find(|f| f.name.as_str() == "outer")
        .expect("outer fn present");
    let hoisted_name = fns
        .iter()
        .find(|f| f.name.as_str().starts_with("__ts_aot_closure_"))
        .expect("hoisted closure fn present")
        .name
        .clone();
    let MirStmt::Return(Some(MirExpr::Call { args, .. })) = &outer_mir.body.block.stmts[0] else {
        panic!(
            "outer fn first stmt must be Return(Some(Call)) carrying the hoisted closure as its first arg, got {:?}",
            outer_mir.body.block.stmts[0]
        );
    };
    let MirExpr::Global(referenced) = &args[0] else {
        panic!(
            "outer Call's first arg must be MirExpr::Global pointing at the hoisted closure, got {:?}",
            args[0]
        );
    };
    assert_eq!(
        *referenced, hoisted_name,
        "outer Call's first arg must reference the hoisted closure by name, got {referenced:?}, expected {hoisted_name:?}"
    );
}

#[test]
fn end_to_end_closure_in_global_init_is_preserved_as_function_reference() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let closure_local = LocalId::from_raw(0);
    let closure_name = Atom::new_inline("f");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Global {
        name: closure_name.clone(),
        ty: i64_ty,
        init: Some(HirExpr::Closure {
            span: Span::default(),
            id: closure_local,
            params: vec![HirParam {
                name: Atom::from("x"),
                ty: i64_ty,
            }],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Local {
                    span: Span::default(),
                    id: LocalId::from_raw(1),
                    ty: i64_ty,
                }),
            }],
            ty: i64_ty,
        }),
    });

    let result = lower_closures(&mut hir, &mut ctx);
    assert_eq!(result.stats.emitted_fns, 1);
    assert!(
        !ctx.has_errors(),
        "lower_closures must not error on global-init non-capturing closure"
    );

    let HirDecl::Global {
        init: Some(init), ..
    } = &hir.declarations[0]
    else {
        panic!(
            "global decl must retain Some(init) after lower_closures hoist; got {:?}",
            hir.declarations[0]
        );
    };
    let HirExpr::Global { name, .. } = init else {
        panic!(
            "global init must be rewritten to HirExpr::Global pointing at hoisted fn, got {init:?}"
        );
    };
    assert!(
        name.as_str().starts_with("__ts_aot_closure_"),
        "rewrite must point at __ts_aot_closure_N, got {name:?}"
    );

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not emit P0006 for HirExpr::Global global init; diagnostics: {:?}",
        ctx.diagnostics()
    );
    assert!(
        !ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0006"),
        "global init that resolves to a function reference must NOT emit P0006"
    );

    let globals: Vec<_> = mir.globals().collect();
    assert_eq!(
        globals.len(),
        1,
        "global f must survive lower_closures + convert_program, got {} globals",
        globals.len()
    );
    let g = globals[0];
    assert_eq!(g.name, closure_name);
    let Some(mir_init) = &g.init else {
        panic!(
            "global f.init must be preserved as function reference, not dropped to None (this is the bug the regression test guards against)"
        );
    };
    let MirExpr::Global(referenced) = mir_init else {
        panic!(
            "global f.init must be MirExpr::Global pointing at hoisted closure fn, got {mir_init:?}"
        );
    };
    assert!(
        referenced.as_str().starts_with("__ts_aot_closure_"),
        "MirExpr::Global.name must point at hoisted closure fn, got {referenced:?}"
    );
}
