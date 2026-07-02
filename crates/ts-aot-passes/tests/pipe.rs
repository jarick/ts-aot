use ts_aot_core::{Atom, ModuleId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirEnumVariant, HirExpr, HirFunction, HirProgram, HirStmt,
};
use ts_aot_ir_mir::{MirDecl, MirExpr, MirGlobalDecl, MirStmt};
use ts_aot_passes::{PassContext, convert_program, lower_async, lower_enums, lower_result};

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
    lower_result(&mut mir);

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
    let MirStmt::Return(Some(MirExpr::Await { .. })) = &f.body.block.stmts[0] else {
        panic!(
            "expected Return(Some(MirExpr::Await)) at stmts[0], got stmts: {:?}",
            f.body.block.stmts
        );
    };
}
