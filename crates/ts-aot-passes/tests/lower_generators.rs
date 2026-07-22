use ts_aot_core::{Atom, ModuleId, Severity, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCatchClause, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, HirSwitchCase,
};
use ts_aot_passes::{PassContext, lower_generators};

#[test]
fn lower_generators_creates_dispatch_function_for_simple_yield() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let yield_stmt = HirStmt::Expr {
        expr: HirExpr::Yield {
            expr: Some(Box::new(HirExpr::Int(1))),
            ty: i64_ty,
        },
    };
    let return_stmt = HirStmt::Return {
        value: Some(HirExpr::Int(2)),
    };
    push_generator(&mut hir, "gen", i64_ty, vec![yield_stmt, return_stmt]);

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert_eq!(
        hir.declarations.len(),
        2,
        "expected 2 decls after transform: original + dispatch"
    );
    let dispatch = find_dispatch(&hir);
    assert_eq!(dispatch.params.len(), 1, "dispatch takes 1 param: g");
    assert_eq!(dispatch.params[0].name, Atom::from("g"));
    assert!(!dispatch.is_generator, "dispatch is regular function");
    let original = hir
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("gen") => Some(f.clone()),
            _ => None,
        })
        .expect("original function must still be present");
    assert!(!original.is_generator, "is_generator flag must be cleared");
    assert_eq!(
        original.body.len(),
        1,
        "constructor body has 1 stmt (return)"
    );
}

#[test]
fn lower_generators_skips_non_generator_functions() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("regular"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(0)),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 0);
    assert_eq!(
        hir.declarations.len(),
        1,
        "no dispatch added for non-generator"
    );
}

#[test]
fn lower_generators_rejects_yield_inside_if_with_diagnostic() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let if_with_yield = HirStmt::If {
        cond: HirExpr::Bool(true),
        then: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1))),
                ty: i64_ty,
            },
        }),
        otherwise: None,
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![if_with_yield, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 0, "transform must be skipped");
    assert_eq!(stats.generators_rejected, 1, "exactly one rejection");
    assert!(ctx.has_errors(), "diagnostic must be reported");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic for yield-in-if must be present");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("`if`"),
        "message must name the wrapper: {}",
        diag.message
    );
    let has_dispatch = has_dispatch(&hir);
    assert!(
        !has_dispatch,
        "no dispatch function must be added when generator is rejected"
    );
}

#[test]
fn lower_generators_rejects_yield_inside_while_with_diagnostic() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let while_with_yield = HirStmt::While {
        cond: HirExpr::Bool(true),
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(7))),
                ty: i64_ty,
            },
        }),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![while_with_yield, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 0);
    assert_eq!(stats.generators_rejected, 1);
    assert!(ctx.has_errors());
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic for yield-in-while must be present");
    assert!(diag.message.contains("`while`"));
}

#[test]
fn lower_generators_rejects_yield_inside_dowhile() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1))),
                ty: i64_ty,
            },
        }),
        cond: HirExpr::Bool(true),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`do-while`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_forof() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::ForOf {
        binding: ts_aot_core::LocalId::from_raw(0),
        iter: HirExpr::Unit,
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1))),
                ty: i64_ty,
            },
        }),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`for-of`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_forin() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::ForIn {
        binding: ts_aot_core::LocalId::from_raw(0),
        iter: HirExpr::Unit,
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1))),
                ty: i64_ty,
            },
        }),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`for-in`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_switch_case() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let case = HirSwitchCase::new(
        Some(HirExpr::Int(1)),
        vec![HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(9))),
                ty: i64_ty,
            },
        }],
    );
    let stmt = HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![case],
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`switch`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_try_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(1))),
                ty: i64_ty,
            },
        }),
        catch: None,
        finally: None,
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`try`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_catch_clause() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(HirStmt::Return { value: None }),
        catch: Some(HirCatchClause::new(
            None,
            Box::new(HirStmt::Expr {
                expr: HirExpr::Yield {
                    expr: Some(Box::new(HirExpr::Int(2))),
                    ty: i64_ty,
                },
            }),
        )),
        finally: None,
    };
    push_generator(&mut hir, "gen", i64_ty, vec![stmt]);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`try`"))
    );
}

#[test]
fn lower_generators_rejects_yield_inside_finally_clause() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(HirStmt::Return { value: None }),
        catch: None,
        finally: Some(Box::new(HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(3))),
                ty: i64_ty,
            },
        })),
    };
    push_generator(&mut hir, "gen", i64_ty, vec![stmt]);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_rejected, 1);
    assert!(
        ctx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "E0501" && d.message.contains("`try`"))
    );
}

#[test]
fn lower_generators_preserves_non_yield_if_inside_state_block() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::If {
        cond: HirExpr::Bool(true),
        then: Box::new(HirStmt::Expr {
            expr: HirExpr::Int(0),
        }),
        otherwise: Some(Box::new(HirStmt::Expr {
            expr: HirExpr::Int(1),
        })),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert_eq!(stats.generators_rejected, 0);
    assert!(
        !ctx.has_errors(),
        "non-yield if must not trigger diagnostic"
    );
    let dispatch = find_dispatch(&hir);
    let preserved_if = dispatch.body.iter().any(|s| match s {
        HirStmt::If { then, .. } => {
            if let HirStmt::Block(stmts) = then.as_ref() {
                stmts
                    .iter()
                    .any(|inner| matches!(inner, HirStmt::If { .. }))
            } else {
                false
            }
        }
        _ => false,
    });
    assert!(
        preserved_if,
        "non-yield if must be preserved inside the state block, dispatch:\n{:?}",
        dispatch.body
    );
}

#[test]
fn lower_generators_preserves_non_yield_while_inside_state_block() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let stmt = HirStmt::While {
        cond: HirExpr::Bool(false),
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Int(0),
        }),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert_eq!(stats.generators_rejected, 0);
    assert!(
        !ctx.has_errors(),
        "non-yield while must not trigger diagnostic"
    );
    let dispatch = find_dispatch(&hir);
    let has_while = dispatch.body.iter().any(|s| match s {
        HirStmt::If { then, .. } => {
            if let HirStmt::Block(stmts) = then.as_ref() {
                stmts
                    .iter()
                    .any(|inner| matches!(inner, HirStmt::While { .. }))
            } else {
                false
            }
        }
        _ => false,
    });
    assert!(
        has_while,
        "non-yield while must be preserved inside the state block, dispatch:\n{:?}",
        dispatch.body
    );
}

#[test]
fn lower_generators_finds_yield_inside_nested_block() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let nested_block = HirStmt::Block(vec![
        HirStmt::Expr {
            expr: HirExpr::Int(0),
        },
        HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(42))),
                ty: i64_ty,
            },
        },
    ]);
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![nested_block, HirStmt::Return { value: None }],
    );
    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let dispatch = find_dispatch(&hir);
    let if_count = dispatch
        .body
        .iter()
        .filter(|s| matches!(s, HirStmt::If { .. }))
        .count();
    assert_eq!(
        if_count, 2,
        "expected 2 state branches (yield in nested block + return), got {if_count}"
    );
}

#[test]
fn lower_generators_dispatch_body_has_state_branches() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![
            HirStmt::Expr {
                expr: HirExpr::Yield {
                    expr: Some(Box::new(HirExpr::Int(1))),
                    ty: i64_ty,
                },
            },
            HirStmt::Expr {
                expr: HirExpr::Yield {
                    expr: Some(Box::new(HirExpr::Int(2))),
                    ty: i64_ty,
                },
            },
            HirStmt::Return {
                value: Some(HirExpr::Int(3)),
            },
        ],
    );

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let dispatch = find_dispatch(&hir);
    let if_count = dispatch
        .body
        .iter()
        .filter(|s| matches!(s, HirStmt::If { .. }))
        .count();
    assert_eq!(if_count, 3, "3 yields/returns → 3 state branches");
}

fn push_generator(hir: &mut HirProgram, name: &str, i64_ty: TypeId, body: Vec<HirStmt>) {
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from(name),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body,
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
}

fn find_dispatch(hir: &HirProgram) -> HirFunction {
    hir.declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name.as_str().starts_with("__gen_dispatch_") => {
                Some(f.clone())
            }
            _ => None,
        })
        .expect("dispatch function must be added")
}

fn has_dispatch(hir: &HirProgram) -> bool {
    hir.declarations.iter().any(
        |d| matches!(d, HirDecl::Function(f) if f.name.as_str().starts_with("__gen_dispatch_")),
    )
}
