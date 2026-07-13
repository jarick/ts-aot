use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirEnumVariant, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
};
use ts_aot_ir_mir::{MirDecl, MirExpr, MirGlobalDecl, MirStmt, RuntimeOp};
use ts_aot_passes::{
    PassContext, convert_program, lower_async, lower_closures, lower_enums, lower_result,
};

fn fixture() -> (TypeTable, PassContext) {
    let types = TypeTable::new();
    let ctx = PassContext::default();
    (types, ctx)
}

fn unit_ty(types: &mut TypeTable) -> ts_aot_core::TypeId {
    types.intern(&ts_aot_core::Type::Void)
}

fn build_enum_decl(name: &str, variants: Vec<(&str, Option<i64>)>) -> HirDecl {
    let variants = variants
        .into_iter()
        .map(|(n, v)| HirEnumVariant {
            name: Atom::new_inline(n),
            value: v.map(HirExpr::Int),
        })
        .collect();
    HirDecl::Enum {
        name: Atom::new_inline(name),
        variants,
    }
}

#[test]
fn convert_program_preserves_global_with_int_init() {
    let (mut types, mut ctx) = fixture();
    let name_sym = Atom::new_inline("ANSWER");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Global {
        name: name_sym.clone(),
        ty: types.intern(&ts_aot_core::Type::I64),
        init: Some(HirExpr::Int(42)),
    });

    let mir = convert_program(&hir, &mut ctx);

    assert_eq!(mir.declarations.len(), 1);
    let MirDecl::Global(g) = &mir.declarations[0] else {
        panic!("expected MirDecl::Global");
    };
    assert_eq!(g.name, name_sym);
    let typed_id = types.intern(&ts_aot_core::Type::I64);
    assert_eq!(g.ty, typed_id, "global.ty must be the i64 from HIR");
    let Some(init) = &g.init else {
        panic!("init must be preserved through HIR->MIR");
    };
    let MirExpr::Int { value, ty } = init else {
        panic!("expected Int init, got {init:?}");
    };
    assert_eq!(*value, 42);
    assert_eq!(*ty, g.ty, "init.ty must match global.ty, not TypeId(0)");
}

#[test]
fn lower_enums_then_convert_program_emits_globals_with_values() {
    let (mut types, mut ctx) = fixture();
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(build_enum_decl(
        "Color",
        vec![("Red", None), ("Green", Some(10)), ("Blue", None)],
    ));

    lower_enums(&mut hir, &mut types, &mut ctx);

    let mir = convert_program(&hir, &mut ctx);

    let globals: Vec<&MirGlobalDecl> = mir.globals().collect();
    assert_eq!(
        globals.len(),
        3,
        "enum with 3 variants must produce 3 MirDecl::Global"
    );

    let mut by_name: Vec<(String, i128)> = Vec::new();
    for g in globals {
        let raw = g.name.as_str().to_owned();
        let val = match &g.init {
            Some(MirExpr::Int { value, .. }) => *value,
            other => panic!("expected Int init for {raw}, got {other:?}"),
        };
        by_name.push((raw, val));
    }
    by_name.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(by_name[0].0, "Color.Blue");
    assert_eq!(by_name[0].1, 11);
    assert_eq!(by_name[1].0, "Color.Green");
    assert_eq!(by_name[1].1, 10);
    assert_eq!(by_name[2].0, "Color.Red");
    assert_eq!(by_name[2].1, 0);
}

#[test]
fn convert_function_with_throw_sets_throws() {
    let (mut types, mut ctx) = fixture();
    let name = Atom::new_inline("oops");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: unit_ty(&mut types),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Int(7),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut ctx);
    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(fns.len(), 1);
    let f = fns[0];
    assert!(
        f.throws.is_some(),
        "convert_function must populate throws when body has Throw"
    );
    assert!(f.effects.can_throw);
}

#[test]
fn convert_function_without_throw_leaves_throws_none() {
    let (mut types, mut ctx) = fixture();
    let name = Atom::new_inline("ok");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: unit_ty(&mut types),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Int(1),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut ctx);
    let f = mir.functions().next().expect("one function");
    assert!(f.throws.is_none());
    assert!(!f.effects.can_throw);
}

#[test]
fn end_to_end_lower_result_rewrites_throw_to_return_result_err() {
    let (mut types, mut ctx) = fixture();
    let name = Atom::new_inline("oops");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: unit_ty(&mut types),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Int(7),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut ctx);
    lower_result(&mut mir, &mut types);

    let f = mir.functions().next().expect("one function");
    assert!(f.throws.is_some());
    assert_eq!(f.body.block.stmts.len(), 1);
    assert!(
        matches!(f.body.block.stmts[0], MirStmt::ReturnResultErr { .. }),
        "Throw must be rewritten to ReturnResultErr by lower_result, got {:?}",
        f.body.block.stmts[0]
    );
}

#[test]
fn end_to_end_throwing_function_emits_result_in_rust_signature() {
    let (mut types, mut ctx) = fixture();
    let int_ty = types.intern(&Type::I32);
    let name = Atom::new_inline("boom");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: int_ty,
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Int(7),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut ctx);
    let pre_throws = mir.functions().next().expect("one function").throws;
    assert!(
        pre_throws.is_some(),
        "convert_program must set f.throws when body has Throw"
    );
    lower_result(&mut mir, &mut types);

    let f = mir.functions().next().expect("one function");
    let Some(Type::Result { ok, err: _ }) = types.resolve(f.ret) else {
        panic!(
            "lower_result must wrap f.ret in Type::Result when f.throws is set, got {:?}",
            types.resolve(f.ret)
        );
    };
    assert_eq!(
        *ok, int_ty,
        "Result.ok must be the original return type (i32)"
    );
    assert!(
        f.body
            .block
            .stmts
            .iter()
            .any(|s| matches!(s, MirStmt::ReturnResultErr { .. }))
    );

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 ,") || s.contains("-> Result<i32 ,"),
        "emitted Rust signature must show Result<i32, ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        !s.contains("-> ()"),
        "ret must not fall back to unit, got: {s}"
    );
}

#[test]
fn end_to_end_throwing_function_with_success_return_emits_ok() {
    let (mut types, mut ctx) = fixture();
    let int_ty = types.intern(&Type::I32);
    let name = Atom::new_inline("maybe_boom");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: int_ty,
        throws: None,
        body: vec![
            HirStmt::If {
                cond: HirExpr::Bool(true),
                then: Box::new(HirStmt::Throw {
                    expr: HirExpr::Int(7),
                }),
                otherwise: None,
            },
            HirStmt::Return {
                value: Some(HirExpr::Int(42)),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut ctx);
    lower_result(&mut mir, &mut types);

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 ,") || s.contains("-> Result<i32 ,"),
        "ret must be Result<i32, ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        s.contains("Ok (42)"),
        "Success Return(42) must be lowered to `Ok(42)`, got: {s}"
    );
    assert!(
        !s.contains("return 42 ;"),
        "Bare `return 42;` would not typecheck in Result-returning fn, got: {s}"
    );
}

#[test]
fn end_to_end_throwing_void_function_bare_return_emits_ok_unit() {
    let (mut types, mut ctx) = fixture();
    let unit_ty = types.intern(&Type::Void);
    let name = Atom::new_inline("void_boom");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![
            HirStmt::If {
                cond: HirExpr::Bool(true),
                then: Box::new(HirStmt::Throw {
                    expr: HirExpr::Int(7),
                }),
                otherwise: None,
            },
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut ctx);
    lower_result(&mut mir, &mut types);

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < () ,")
            || s.contains("-> Result<()> ,")
            || s.contains("Result < ()"),
        "ret must be Result<(), ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        s.contains("Ok (())"),
        "Bare return must be lowered to `Ok(())`, got: {s}"
    );
    assert!(
        !s.contains("return ;"),
        "Bare `return;` would not typecheck in Result-returning fn, got: {s}"
    );
}

#[test]
fn end_to_end_enum_through_hir_to_mir_dump_includes_values() {
    let (mut types, mut ctx) = fixture();
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations
        .push(build_enum_decl("E", vec![("A", None), ("B", None)]));

    lower_enums(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut ctx);
    let text = mir.dump_text();
    assert!(text.contains("global"), "expected global in dump:\n{text}");

    let globals: Vec<_> = mir.globals().collect();
    assert_eq!(globals.len(), 2);
    let mut by_name: Vec<(String, i128)> = globals
        .into_iter()
        .map(|g| {
            let raw = g.name.as_str().to_owned();
            let val = match &g.init {
                Some(MirExpr::Int { value, .. }) => *value,
                other => panic!("expected Int init for {raw}, got {other:?}"),
            };
            (raw, val)
        })
        .collect();
    by_name.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(by_name[0].0, "E.A");
    assert_eq!(by_name[0].1, 0);
    assert_eq!(by_name[1].0, "E.B");
    assert_eq!(by_name[1].1, 1);
    assert!(
        text.contains("= 0(:0)"),
        "dump must render init=0 explicitly for E.A:\n{text}"
    );
    assert!(
        text.contains("= 1(:0)"),
        "dump must render init=1 explicitly for E.B:\n{text}"
    );
}

fn build_enum_decl_returning_sym(
    name: &str,
    variants: Vec<(&str, Option<i64>)>,
) -> (HirDecl, ts_aot_core::Atom) {
    let enum_name = Atom::new_inline(name);
    let variants = variants
        .into_iter()
        .map(|(n, v)| HirEnumVariant {
            name: Atom::new_inline(n),
            value: v.map(HirExpr::Int),
        })
        .collect();
    (
        HirDecl::Enum {
            name: enum_name.clone(),
            variants,
        },
        enum_name,
    )
}

#[test]
fn enum_member_use_in_function_body_is_rewritten_to_namespaced_global() {
    let (mut types, mut ctx) = fixture();
    let mut hir = HirProgram::new(ModuleId::from_raw(0));

    let (enum_decl, color_sym) =
        build_enum_decl_returning_sym("Color", vec![("Red", None), ("Green", Some(10))]);
    hir.declarations.push(enum_decl);

    let typed_id = types.intern(&ts_aot_core::Type::I64);
    let green_name = Atom::new_inline("Green");
    let fn_name = Atom::new_inline("pick");

    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name.clone(),
        params: Vec::new(),
        ret: typed_id,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: color_sym.clone(),
                    ty: typed_id,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: green_name.clone(),
                ty: typed_id,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    lower_enums(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut ctx);

    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(fns.len(), 1);
    let f = fns[0];
    assert_eq!(f.name, fn_name.clone());

    let MirStmt::Return(Some(ret_expr)) = &f.body.block.stmts[0] else {
        panic!(
            "expected Return(Some(expr)), got {:?}",
            f.body.block.stmts[0]
        );
    };
    let MirExpr::Global(resolved) = ret_expr else {
        panic!(
            "Color.Green use must be rewritten to MirExpr::Global, got {:?}",
            ret_expr
        );
    };
    let expected = Atom::new_inline("Color.Green");
    assert_eq!(
        *resolved, expected,
        "Field(Global(Color), Green) must rewrite to Global(Color.Green)"
    );

    let text = mir.dump_text();
    assert!(
        text.contains("Color.Green"),
        "dump must show the namespaced global:\n{text}"
    );
}

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
    let (mut types, mut ctx) = fixture();
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

    let mir = convert_program(&hir, &mut ctx);
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
    let (mut types, mut ctx) = fixture();
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

    let mir = convert_program(&hir, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::Await { .. })) = &f.body.block.stmts[1] else {
        panic!(
            "expected Return(Some(MirExpr::Await)) at stmts[1] (after CallIndirect Runtime at stmts[0]), got stmts: {:?}",
            f.body.block.stmts
        );
    };
}

#[test]
fn end_to_end_lower_closures_then_convert_program_resolves_indirect_global_callee() {
    let (mut types, mut ctx) = fixture();
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
                    id: cb_local,
                    params: vec![HirParam {
                        name: Atom::new_inline("x"),
                        ty: i64_ty,
                    }],
                    captures: Vec::new(),
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr::Local {
                            id: LocalId::from_raw(1),
                            ty: i64_ty,
                        }),
                    }],
                    ty: i64_ty,
                }),
            },
            HirStmt::Expr {
                expr: HirExpr::Call {
                    callee: HirCallee::Closure(cb_local),
                    args: vec![HirExpr::Int(7)],
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

    let mir = convert_program(&hir, &mut ctx);
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

    let MirStmt::Expr(MirExpr::Call { callee, .. }) = &outer_mir.body.block.stmts[1] else {
        panic!(
            "outer fn second stmt must be Expr(Call) after lower_closures rewrite, got {:?}",
            outer_mir.body.block.stmts[1]
        );
    };
    assert_eq!(
        *callee, hoisted_id,
        "outer fn Call.callee must be the hoisted closure fn's FunctionId, got {callee:?}"
    );
}

#[test]
fn convert_program_unresolved_global_name_falls_through_to_placeholder() {
    let (mut types, mut ctx) = fixture();
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

    let mir = convert_program(&hir, &mut ctx);
    let f = mir.functions().next().expect("one fn");

    let MirStmt::Runtime {
        op: RuntimeOp::CallIndirect,
        args,
        dest,
        ..
    } = &f.body.block.stmts[0]
    else {
        panic!(
            "expected MirStmt::Runtime::CallIndirect at stmts[0] (PR 1.2 runtime fallback), got {:?}",
            f.body.block.stmts[0]
        );
    };
    let dest = dest.expect("CallIndirect must allocate a dest local");
    let MirExpr::Global(callee_name) = &args[0] else {
        panic!(
            "first arg of CallIndirect must be the callee value, got {:?}",
            args[0]
        );
    };
    assert_eq!(
        callee_name.as_str(),
        user_global_name.as_str(),
        "callee value must be the unresolved global name; got {callee_name:?}"
    );
    assert_eq!(
        args.len(),
        2,
        "CallIndirect args must be [callee, ...original_args]; got {} args",
        args.len()
    );
    let _ = dest;
    assert!(
        !ctx.has_errors(),
        "PR 1.2: P0005 for unresolved indirect callee is downgraded to warning (runtime fallback handles it), so has_errors() must be false"
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
fn end_to_end_closure_in_global_init_is_preserved_as_function_reference() {
    let (mut types, mut ctx) = fixture();
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let closure_local = LocalId::from_raw(0);
    let closure_name = Atom::new_inline("f");

    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Global {
        name: closure_name.clone(),
        ty: i64_ty,
        init: Some(HirExpr::Closure {
            id: closure_local,
            params: vec![HirParam {
                name: Atom::from("x"),
                ty: i64_ty,
            }],
            captures: Vec::new(),
            body: vec![HirStmt::Return {
                value: Some(HirExpr::Local {
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

    let mir = convert_program(&hir, &mut ctx);
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

#[test]
fn indirect_call_to_resolved_global_uses_direct_call_not_runtime_fallback() {
    let (mut types, mut ctx) = fixture();
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

    let mir = convert_program(&hir, &mut ctx);
    let f = mir
        .functions()
        .find(|f| f.name == fn_name)
        .expect("caller must exist");
    let MirStmt::Expr(MirExpr::Call { callee, args, .. }) = &f.body.block.stmts[0] else {
        panic!(
            "resolved Indirect(Global) must still produce MirExpr::Call (PR 1.1 path), got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert_eq!(
        *callee,
        ts_aot_core::FunctionId::from_raw(0),
        "Indirect(Global(\"add\")) must resolve to FunctionId(0); got {callee:?}"
    );
    assert_eq!(args.len(), 1, "direct call must preserve original args");
    assert!(
        !ctx.has_errors(),
        "resolved Indirect(Global) must not emit P0005; got {:?}",
        ctx.diagnostics()
    );
}

#[test]
fn indirect_call_to_non_global_callee_emits_p0005_warning_for_runtime_dispatch_fallback() {
    let (mut types, mut ctx) = fixture();
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

    let _ = convert_program(&hir, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "PR 1.2 must keep compilation alive (warning, not error) for non-Global callee; got {:?}",
        ctx.diagnostics()
    );
    let fallback_warnings: Vec<_> = ctx
        .diagnostics()
        .iter()
        .filter(|d| {
            d.code.as_str() == "P0005" && d.message.contains("will fail during Rust code emission")
        })
        .collect();
    assert_eq!(
        fallback_warnings.len(),
        1,
        "non-Global indirect callee must emit exactly one PR 1.2 fallback warning, got {fallback_warnings:?}"
    );
}
