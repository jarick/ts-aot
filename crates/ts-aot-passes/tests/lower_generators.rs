mod common;

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, Severity, Span, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirCallee, HirCatchClause, HirClass, HirDecl, HirExpr, HirFunction, HirParam, HirProgram,
    HirStmt, HirSwitchCase,
};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirStmt, RuntimeOp};
use ts_aot_passes::{PassContext, convert_program, lower_generators};

use common::{convert, count_runtime_ops, find_mir_function, has_errors};

fn yield_stmt(value: i64, i64_ty: TypeId) -> HirStmt {
    HirStmt::Expr {
        expr: HirExpr::Yield {
            expr: Some(Box::new(HirExpr::Int(value, Span::default()))),
            ty: i64_ty,
            span: Span::default(),
        },
    }
}

fn collect_call_callees_in_block(b: &MirBlock, out: &mut Vec<FunctionId>) {
    for s in &b.stmts {
        match s {
            MirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_in_expr(e, out);
                }
            }
            MirStmt::Assign { value, .. } => collect_in_expr(value, out),
            MirStmt::Expr(expr) => collect_in_expr(expr, out),
            MirStmt::Return(Some(expr)) => collect_in_expr(expr, out),
            MirStmt::Return(None) => {}
            MirStmt::ReturnResultErr { error, .. } => collect_in_expr(error, out),
            MirStmt::Throw { error, .. } => collect_in_expr(error, out),
            MirStmt::Runtime { args, .. } => {
                for a in args {
                    collect_in_expr(a, out);
                }
            }
            MirStmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                collect_in_expr(cond, out);
                collect_call_callees_in_block(then_block, out);
                if let Some(eb) = else_block {
                    collect_call_callees_in_block(eb, out);
                }
            }
            MirStmt::While { cond, body } => {
                collect_in_expr(cond, out);
                collect_call_callees_in_block(body, out);
            }
            MirStmt::DoWhile { body, cond } => {
                collect_in_expr(cond, out);
                collect_call_callees_in_block(body, out);
            }
            MirStmt::ForOf { iterable, body, .. } | MirStmt::ForAwaitOf { iterable, body, .. } => {
                collect_in_expr(iterable, out);
                collect_call_callees_in_block(body, out);
            }
            MirStmt::ForIn { object, body, .. } => {
                collect_in_expr(object, out);
                collect_call_callees_in_block(body, out);
            }
            MirStmt::Switch {
                disc,
                cases,
                default,
            } => {
                collect_in_expr(disc, out);
                for case in cases {
                    collect_call_callees_in_block(&case.body, out);
                }
                if let Some(def) = default {
                    collect_call_callees_in_block(def, out);
                }
            }
            MirStmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                collect_call_callees_in_block(body, out);
                if let Some(catch_block) = catch {
                    collect_call_callees_in_block(catch_block, out);
                }
                if let Some(fin) = finally {
                    collect_call_callees_in_block(fin, out);
                }
            }
            MirStmt::Break | MirStmt::Continue => {}
        }
    }
}

fn collect_in_expr(e: &MirExpr, out: &mut Vec<FunctionId>) {
    match e {
        MirExpr::Unit
        | MirExpr::Bool(_)
        | MirExpr::Int { .. }
        | MirExpr::Float { .. }
        | MirExpr::String { .. }
        | MirExpr::Null { .. }
        | MirExpr::Local(_)
        | MirExpr::Global(_)
        | MirExpr::TemplateStringsArray { .. }
        | MirExpr::RegExp { .. }
        | MirExpr::BigInt { .. } => {}
        MirExpr::Field { base, .. } => collect_in_expr(base, out),
        MirExpr::Index { base, index, .. } => {
            collect_in_expr(base, out);
            collect_in_expr(index, out);
        }
        MirExpr::Call { callee, args, .. } => {
            out.push(*callee);
            for a in args {
                collect_in_expr(a, out);
            }
        }
        MirExpr::IndirectCall { callee, args, .. } => {
            collect_in_expr(callee, out);
            for a in args {
                collect_in_expr(a, out);
            }
        }
        MirExpr::StructLiteral { fields, .. } => {
            for (_, fv) in fields {
                collect_in_expr(fv, out);
            }
        }
        MirExpr::ResultOk { value, .. } => collect_in_expr(value, out),
        MirExpr::ResultErr { error, .. } => collect_in_expr(error, out),
        MirExpr::Binary { left, right, .. } => {
            collect_in_expr(left, out);
            collect_in_expr(right, out);
        }
        MirExpr::Unary { expr, .. } => collect_in_expr(expr, out),
        MirExpr::Await { expr, .. } => collect_in_expr(expr, out),
        MirExpr::Yield { expr, .. } => {
            if let Some(inner) = expr {
                collect_in_expr(inner, out);
            }
        }
        MirExpr::OptionalChain { base, .. } => collect_in_expr(base, out),
        MirExpr::TypeOf { expr, .. } => collect_in_expr(expr, out),
        MirExpr::Cast { expr, .. } => collect_in_expr(expr, out),
        MirExpr::Import { source, .. } => collect_in_expr(source, out),
        MirExpr::Closure { body, .. } => {
            collect_call_callees_in_block(body, out);
        }
    }
}

#[test]
fn lower_generators_rewrites_ret_and_keeps_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(2, Span::default())),
            },
        ],
    );

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert_eq!(
        hir.declarations.len(),
        1,
        "no dispatch function must be added anymore"
    );
    let original = find_fn(&hir, "gen");
    assert_eq!(
        original.ret, gen_ty,
        "ret must be rewritten to Generator<i64>"
    );
    assert!(
        original.is_generator,
        "is_generator flag must be kept for MIR lowering"
    );
    assert_eq!(
        original.body.len(),
        2,
        "body must keep the yield and the return untouched"
    );
    assert!(
        matches!(
            &original.body[0],
            HirStmt::Expr {
                expr: HirExpr::Yield { .. }
            }
        ),
        "yield must survive lowering, got: {:?}",
        original.body[0]
    );
    assert!(!ctx.has_errors(), "no diagnostics expected");
}

#[test]
fn lower_generators_appends_fallthrough_return() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    push_generator(&mut hir, "gen", i64_ty, vec![yield_stmt(1, i64_ty)]);

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    let original = find_fn(&hir, "gen");
    assert_eq!(
        original.body.len(),
        2,
        "fallthrough return must be appended after the trailing yield"
    );
    assert!(
        matches!(original.body[1], HirStmt::Return { value: None }),
        "appended stmt must be `return None` (bare return), got: {:?}",
        original.body[1]
    );
}

#[test]
fn lower_generators_does_not_append_return_when_body_is_terminal() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(2, Span::default())),
            },
        ],
    );

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let original = find_fn(&hir, "gen");
    assert_eq!(
        original.body.len(),
        2,
        "no extra return must be appended when the body already returns"
    );
}

#[test]
fn lower_generators_skips_non_generator_functions() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("regular"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(0, Span::default())),
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
        "no decls added for non-generator"
    );
}

#[test]
fn lower_generators_supports_yield_inside_if() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let if_with_yield = HirStmt::If {
        cond: HirExpr::Bool(true, Span::default()),
        then: Box::new(yield_stmt(1, i64_ty)),
        otherwise: None,
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![if_with_yield, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 1,
        "yield inside if must be supported"
    );
    assert_eq!(stats.generators_rejected, 0);
    assert!(!ctx.has_errors(), "got: {:?}", ctx.diagnostics());
    assert_eq!(
        hir.declarations.len(),
        1,
        "no __gen_dispatch_* function must be added by lower_generators"
    );
}

#[test]
fn lower_generators_supports_yield_inside_while() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let while_with_yield = HirStmt::While {
        cond: HirExpr::Bool(true, Span::default()),
        body: Box::new(yield_stmt(7, i64_ty)),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![while_with_yield, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_supports_yield_inside_dowhile() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::DoWhile {
        body: Box::new(yield_stmt(1, i64_ty)),
        cond: HirExpr::Bool(true, Span::default()),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_supports_yield_inside_forof() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::ForOf {
        binding: ts_aot_core::LocalId::from_raw(0),
        iter: HirExpr::Unit(Span::default()),
        body: Box::new(yield_stmt(1, i64_ty)),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_supports_yield_inside_forin() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::ForIn {
        binding: ts_aot_core::LocalId::from_raw(0),
        iter: HirExpr::Unit(Span::default()),
        body: Box::new(yield_stmt(1, i64_ty)),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_supports_yield_inside_switch_case() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let case = HirSwitchCase::new(
        Some(HirExpr::Int(1, Span::default())),
        vec![yield_stmt(9, i64_ty)],
    );
    let stmt = HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![case],
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![stmt, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_rejects_yield_inside_try_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(yield_stmt(1, i64_ty)),
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
    assert_eq!(
        stats.generators_transformed, 0,
        "yield in try body must not transform the generator"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "yield in try body must be rejected"
    );
    assert!(ctx.has_errors(), "diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for yield in try body");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message
            .contains("yield inside a try body or catch clause"),
        "message must mention try body or catch clause rejection, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_yield_inside_catch_clause() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(HirStmt::Return { value: None }),
        catch: Some(HirCatchClause::new(None, Box::new(yield_stmt(2, i64_ty)))),
        finally: None,
    };
    push_generator(&mut hir, "gen", i64_ty, vec![stmt]);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 0,
        "yield in catch clause must not transform the generator"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "yield in catch clause must be rejected"
    );
    assert!(ctx.has_errors(), "diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for yield in catch clause");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message
            .contains("yield inside a try body or catch clause"),
        "message must mention try body or catch clause rejection, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_supports_yield_inside_finally_clause() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::Try {
        body: Box::new(HirStmt::Return { value: None }),
        catch: None,
        finally: Some(Box::new(yield_stmt(3, i64_ty))),
    };
    push_generator(&mut hir, "gen", i64_ty, vec![stmt]);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_preserves_non_yield_if() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::If {
        cond: HirExpr::Bool(true, Span::default()),
        then: Box::new(HirStmt::Expr {
            expr: HirExpr::Int(0, Span::default()),
        }),
        otherwise: Some(Box::new(HirStmt::Expr {
            expr: HirExpr::Int(1, Span::default()),
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
    assert!(!ctx.has_errors());
    let original = find_fn(&hir, "gen");
    assert!(
        matches!(&original.body[0], HirStmt::If { .. }),
        "non-yield if must be preserved, got: {:?}",
        original.body[0]
    );
}

#[test]
fn lower_generators_preserves_non_yield_while() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let stmt = HirStmt::While {
        cond: HirExpr::Bool(false, Span::default()),
        body: Box::new(HirStmt::Expr {
            expr: HirExpr::Int(0, Span::default()),
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
    assert!(!ctx.has_errors());
}

#[test]
fn lower_generators_supports_yield_inside_nested_block() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let nested_block = HirStmt::Block(vec![
        HirStmt::Expr {
            expr: HirExpr::Int(0, Span::default()),
        },
        yield_stmt(42, i64_ty),
    ]);
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![nested_block, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors());
    assert_eq!(
        hir.declarations.len(),
        1,
        "no __gen_dispatch_* function must be added by lower_generators"
    );
}

#[test]
fn lower_generators_rejects_expression_position_yield() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let call = HirStmt::Expr {
        expr: HirExpr::Binary {
            op: ts_aot_ir_hir::HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1, Span::default())),
            rhs: Box::new(HirExpr::Yield {
                expr: Some(Box::new(HirExpr::Int(2, Span::default()))),
                ty: i64_ty,
                span: Span::default(),
            }),
            ty: i64_ty,
            span: Span::default(),
        },
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![call, HirStmt::Return { value: None }],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 0, "transform must be skipped");
    assert_eq!(stats.generators_rejected, 1);
    assert!(ctx.has_errors());
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present");
    assert_eq!(diag.severity, Severity::Error);
    assert!(diag.message.contains("expression position"));
}

#[test]
fn lower_generators_rejects_await_inside_generator() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let body: Vec<HirStmt> = vec![
        yield_stmt(1, i64_ty),
        HirStmt::Expr {
            expr: HirExpr::Await {
                expr: Box::new(HirExpr::Unit(Span::new(50, 51))),
                ty: unit_ty,
                span: Span::new(50, 51),
            },
        },
        HirStmt::Return { value: None },
    ];
    push_generator(&mut hir, "gen", i64_ty, body);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 0,
        "generator with await must not transform"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "generator with await must be rejected"
    );
    assert!(ctx.has_errors(), "diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for await inside generator");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("await"),
        "message must mention await, got: {:?}",
        diag.message
    );
    assert_eq!(
        diag.span,
        Span::new(50, 51),
        "diagnostic span must point at the await expression"
    );
}

#[test]
fn lower_generators_rejects_throw_inside_generator() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![
            yield_stmt(1, i64_ty),
            HirStmt::Throw {
                expr: HirExpr::Unit(Span::default()),
            },
        ],
    );
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 0);
    assert_eq!(stats.generators_rejected, 1);
    assert!(ctx.has_errors());
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present");
    assert!(diag.message.contains("throw"));
}

#[test]
fn lower_generators_supports_params() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let f = HirFunction {
        name: Atom::from("gen"),
        params: vec![ts_aot_ir_hir::HirParam {
            name: Atom::from("n"),
            ty: i64_ty,
        }],
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Expr {
                expr: HirExpr::Yield {
                    expr: Some(Box::new(HirExpr::Local {
                        id: ts_aot_core::LocalId::from_raw(0),
                        ty: i64_ty,
                        span: Span::default(),
                    })),
                    ty: i64_ty,
                    span: Span::default(),
                },
            },
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    hir.declarations.push(HirDecl::Function(f));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(!ctx.has_errors(), "got: {:?}", ctx.diagnostics());
}

#[test]
fn lower_generators_accepts_throw_inside_nested_closure() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let throwing_closure = HirExpr::Closure {
        id: ts_aot_core::LocalId::from_raw(99),
        params: vec![HirParam {
            name: Atom::from("__p"),
            ty: i64_ty,
        }],
        captures: Vec::new(),
        body: vec![HirStmt::Throw {
            expr: HirExpr::Unit(Span::default()),
        }],
        ty: unit_ty,
        span: Span::default(),
    };
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![
            HirStmt::Expr {
                expr: throwing_closure,
            },
            yield_stmt(1, i64_ty),
            HirStmt::Return { value: None },
        ],
    );

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 1);
    assert!(
        !ctx.has_errors(),
        "throw inside a nested closure must not be attributed to the enclosing generator, got: {:?}",
        ctx.diagnostics()
    );
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

fn find_fn(hir: &HirProgram, name: &str) -> HirFunction {
    hir.declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from(name) => Some(f.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("function '{name}' must be present"))
}

#[test]
fn lower_generators_rejects_void_return_with_valued_yield() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let void_ty = types.intern(&Type::Void);
    push_generator(&mut hir, "gen", void_ty, vec![yield_stmt(1, void_ty)]);
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 0,
        "void-returning generator with valued yield must not transform"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "void-returning generator with valued yield must be rejected"
    );
    assert!(ctx.has_errors(), "diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for void + valued yield");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("non-void"),
        "message must explain the non-void requirement, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_generator_method_inside_class() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let class_ty = TypeId::from_raw(42);
    let method = HirFunction {
        name: Atom::from("iter"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    hir.declarations.push(HirDecl::Class(HirClass {
        name: Atom::from("Klass"),
        ty: class_ty,
        fields: Vec::new(),
        methods: vec![method],
        extends: None,
        type_params: Vec::new(),
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 0,
        "class generator method must not be transformed"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "class generator method must be rejected"
    );
    assert!(ctx.has_errors(), "E0501 diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for class generator method");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("iter()"),
        "message must name the rejected method, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_generator_nested_inside_closure_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let inner_gen = HirFunction {
        name: Atom::from("inner"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let closure = HirExpr::Closure {
        id: ts_aot_core::LocalId::from_raw(99),
        params: Vec::new(),
        captures: Vec::new(),
        body: vec![HirStmt::Decl(HirDecl::Function(inner_gen))],
        ty: unit_ty,
        span: Span::default(),
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("outer"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Expr { expr: closure },
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "generator nested inside a closure body must be rejected"
    );
    assert!(ctx.has_errors(), "E0501 diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for nested generator in closure");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("inner"),
        "message must name the rejected nested generator, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_deeply_nested_generators_via_namespace_member_recursion() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let innermost_gen = HirFunction {
        name: Atom::from("innermost"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let outer_gen = HirFunction {
        name: Atom::from("outer_gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Decl(HirDecl::Function(innermost_gen)),
            yield_stmt(2, i64_ty),
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let inner_ns = HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(outer_gen)],
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("host"),
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![HirStmt::Decl(inner_ns), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 2,
        "outer_gen (ns1 member) + innermost (inside outer_gen body) must both be rejected; \
         outer_gen goes through report_nested_generator via reject_nested_decl_list namespace \
         recursion, innermost goes through the visitor's visit_stmt path which also calls the helper"
    );
    let rejected_names: Vec<String> = ctx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0501")
        .filter_map(|d| {
            if d.message.contains("outer_gen") {
                Some("outer_gen".to_owned())
            } else if d.message.contains("innermost") {
                Some("innermost".to_owned())
            } else {
                None
            }
        })
        .collect();
    assert!(
        rejected_names.contains(&"outer_gen".to_owned()),
        "outer_gen (ns1 namespace member) must be rejected via report_nested_generator, got: {:?}",
        rejected_names
    );
    assert!(
        rejected_names.contains(&"innermost".to_owned()),
        "innermost (declared inside outer_gen body, depth 2) must be rejected, got: {:?}",
        rejected_names
    );
}

#[test]
fn lower_generators_rejects_generator_nested_inside_deeply_nested_namespace() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let deep_gen = HirFunction {
        name: Atom::from("deep_gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let inner_ns = HirDecl::Namespace {
        name: Atom::from("ns_inner"),
        members: vec![HirDecl::Function(deep_gen)],
    };
    let middle_ns = HirDecl::Namespace {
        name: Atom::from("ns_middle"),
        members: vec![inner_ns],
    };
    let outer_ns = HirDecl::Namespace {
        name: Atom::from("ns_outer"),
        members: vec![middle_ns],
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("host"),
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![HirStmt::Decl(outer_ns), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "deeply nested generator (ns_outer::ns_middle::ns_inner::deep_gen) must be rejected"
    );
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 must be present for deeply nested generator");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("deep_gen"),
        "message must name the deeply nested generator, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_class_generator_method_inside_function_body_namespace() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let class_with_gen_method = HirClass {
        name: Atom::from("Counter"),
        ty: unit_ty,
        type_params: Vec::new(),
        extends: None,
        fields: Vec::new(),
        methods: vec![HirFunction {
            name: Atom::from("stream"),
            params: Vec::new(),
            ret: i64_ty,
            throws: None,
            body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
            is_async: false,
            is_generator: true,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }],
    };
    let inner_ns = HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Class(class_with_gen_method)],
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("host"),
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![HirStmt::Decl(inner_ns), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "class generator method (Counter::stream) inside function-body namespace must be rejected"
    );
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 must be present for nested class generator method");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("stream"),
        "message must name the nested generator method, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_generator_nested_inside_namespace_in_function_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let inner_gen = HirFunction {
        name: Atom::from("inner"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let inner_ns = HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(inner_gen)],
    };
    let outer_ns_stmt = HirStmt::Decl(inner_ns);
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("outer"),
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![outer_ns_stmt, HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "generator nested inside a namespace inside a function body must be rejected"
    );
    assert!(ctx.has_errors(), "E0501 diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for nested generator in namespace-in-function");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("inner"),
        "message must name the rejected nested generator, got: {:?}",
        diag.message
    );
}

fn call_to_global(name: &'static str, ty: TypeId) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from(name),
            ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        ty,
        type_args: Vec::new(),
        span: Span::default(),
    }
}

#[test]
fn lower_generators_propagates_generator_type_through_yield_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let pre_call_ty = types.intern(&Type::Error);
    let inner = call_to_global("gen", pre_call_ty);
    let yield_expr = HirExpr::Yield {
        expr: Some(Box::new(inner)),
        ty: i64_ty,
        span: Span::default(),
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("caller"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Expr { expr: yield_expr },
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(stats.generators_transformed, 2, "both gens must transform");
    assert_eq!(stats.generators_rejected, 0);
    assert!(
        !ctx.has_errors(),
        "yield gen() must not error, got: {:?}",
        ctx.diagnostics()
    );
    let caller = find_fn(&hir, "caller");
    let HirStmt::Expr { expr: caller_yield } = &caller.body[0] else {
        panic!(
            "caller.body[0] must be the yield expression, got: {:?}",
            caller.body[0]
        );
    };
    let HirExpr::Yield {
        expr: Some(inner_after),
        ..
    } = caller_yield
    else {
        panic!("caller.body[0] expr must be Yield with inner, got: {caller_yield:?}");
    };
    let HirExpr::Call { ty: inner_ty, .. } = inner_after.as_ref() else {
        panic!("inner must be a Call, got: {inner_after:?}");
    };
    assert_eq!(
        *inner_ty,
        gen_ty,
        "yield gen() must propagate the generator type to the inner Call (got TypeId({}))",
        inner_ty.raw()
    );
}

#[test]
fn propagate_generator_types_dedups_global_span_diagnostic() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    push_generator(
        &mut hir,
        "gen",
        i64_ty,
        vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
    );
    let use_span = Span::new(42, 45);
    let gen_ref = HirStmt::Expr {
        expr: HirExpr::Global {
            name: Atom::from("gen"),
            ty: gen_ty,
            span: use_span,
        },
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("use_gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![gen_ref, HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let e0501_count = ctx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0501")
        .count();
    assert_eq!(
        e0501_count,
        1,
        "single `gen` reference must emit exactly one E0501 diagnostic \
         (not one per fixpoint iteration), got {}: {:?}",
        e0501_count,
        ctx.diagnostics()
    );
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present");
    assert_eq!(
        diag.span, use_span,
        "diagnostic must point at the gen reference span"
    );
}

#[test]
fn propagate_generator_types_reaches_nested_namespace_functions() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let nested_gen = HirFunction {
        name: Atom::from("nested_gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("outer"),
        members: vec![HirDecl::Namespace {
            name: Atom::from("inner"),
            members: vec![HirDecl::Function(nested_gen)],
        }],
    });

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 1,
        "generator inside nested namespace must be transformed, got stats: {:?}",
        stats
    );
    assert_eq!(stats.generators_rejected, 0);
    assert!(!ctx.has_errors(), "got: {:?}", ctx.diagnostics());

    fn find_nested_gen(decls: &[HirDecl]) -> Option<&HirFunction> {
        for d in decls {
            match d {
                HirDecl::Function(f) => return Some(f),
                HirDecl::Namespace { members, .. } => {
                    if let Some(f) = find_nested_gen(members) {
                        return Some(f);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let transformed = find_nested_gen(&hir.declarations).expect("nested_gen must be present");
    assert_eq!(
        transformed.ret, gen_ty,
        "nested namespace generator must have its ret rewritten to Generator<i64>, got {:?}",
        transformed.ret
    );
}

#[test]
fn lower_generators_propagates_type_through_while_cond() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
            yield 2;
            return 0;
        }
        function main(): i64 {
            const g = gen();
            while (g.next()) {
                const x = 1;
            }
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "no errors expected, got: {diags:?}");
    let _main = find_mir_function(&mir, "main").expect("main must be present");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    let mir_dump = mir.dump_text();
    assert_eq!(
        count, 1,
        "g.next() inside `while (g.next())` must lower to exactly one \
         RuntimeOp::GeneratorNext (cond must be walked exactly once so generator type propagates), \
         got count={count}, diags={diags:?}, mir:\n{mir_dump}"
    );
}

#[test]
fn lower_generators_propagates_type_through_if_cond() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
            return 0;
        }
        function main(): i64 {
            const g = gen();
            if (g.next()) {
                const x = 1;
            }
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "no errors expected, got: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "g.next() inside `if (g.next())` must lower to exactly one \
         RuntimeOp::GeneratorNext (cond must be walked so generator type propagates), \
         got count={count}, diags={diags:?}"
    );
}

#[test]
fn lower_generators_propagates_type_through_dowhile_cond() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
            return 0;
        }
        function main(): i64 {
            const g = gen();
            do {
                const x = 1;
            } while (g.next());
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "no errors expected, got: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "g.next() inside `do {{ ... }} while (g.next())` must lower to exactly one \
         RuntimeOp::GeneratorNext (cond must be walked so generator type propagates), \
         got count={count}, diags={diags:?}"
    );
}

#[test]
fn propagate_generator_types_keeps_separate_generators_per_namespace() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let make_gen = || HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(make_gen())],
    });
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns2"),
        members: vec![HirDecl::Function(make_gen())],
    });

    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_transformed, 2,
        "both namespace-scoped generators must be transformed, got stats: {:?}",
        stats
    );
    assert_eq!(stats.generators_rejected, 0);
    assert!(!ctx.has_errors(), "got: {:?}", ctx.diagnostics());

    fn find_named_gen<'a>(decls: &'a [HirDecl], ns: &str) -> Option<&'a HirFunction> {
        for d in decls {
            if let HirDecl::Namespace { name, members } = d
                && name.as_str() == ns
            {
                for m in members {
                    if let HirDecl::Function(f) = m {
                        return Some(f);
                    }
                }
            }
        }
        None
    }

    let ns1_gen =
        find_named_gen(&hir.declarations, "ns1").expect("ns1::gen must be present after lowering");
    let ns2_gen =
        find_named_gen(&hir.declarations, "ns2").expect("ns2::gen must be present after lowering");

    assert_eq!(
        ns1_gen.ret, gen_ty,
        "ns1::gen ret must be rewritten to Generator<i64>, got {:?}",
        ns1_gen.ret
    );
    assert_eq!(
        ns2_gen.ret, gen_ty,
        "ns2::gen ret must be rewritten to Generator<i64>, got {:?}",
        ns2_gen.ret
    );
    assert!(
        ns1_gen.is_generator && ns2_gen.is_generator,
        "is_generator flag must be kept on both namespaced generators"
    );

    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let mir_ns1 = find_mir_function(&mir, "ns1::gen")
        .expect("MIR must contain ns1::gen (namespace-qualified identity preserved)");
    let mir_ns2 = find_mir_function(&mir, "ns2::gen")
        .expect("MIR must contain ns2::gen (namespace-qualified identity preserved)");

    assert_eq!(
        mir_ns1.ret, gen_ty,
        "MIR ns1::gen must have ret = Generator<i64>, got {:?}",
        mir_ns1.ret
    );
    assert_eq!(
        mir_ns2.ret, gen_ty,
        "MIR ns2::gen must have ret = Generator<i64>, got {:?}",
        mir_ns2.ret
    );
    assert_ne!(
        mir_ns1.id, mir_ns2.id,
        "namespace-scoped generators must be lowered to distinct MIR function ids, \
         both ended up at {:?}",
        mir_ns1.id
    );
}

#[test]
fn propagate_generator_types_resolves_namespace_scoped_next() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let gen_fn = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);
    let g_name = Atom::from("g");

    let constructor_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("ns1::gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let next_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: g_local_id,
                ty: i64_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::from("next"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: g_name,
                ty: i64_ty,
                init: Some(constructor_call),
            },
            HirStmt::Expr { expr: next_call },
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(gen_fn), HirDecl::Function(main_fn)],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let mir_gen = find_mir_function(&mir, "ns1::gen")
        .expect("MIR must contain ns1::gen (namespace-qualified identity preserved)");
    let mir_main = find_mir_function(&mir, "ns1::main")
        .expect("MIR must contain ns1::main (namespace-qualified identity preserved)");

    assert_eq!(
        mir_gen.ret, gen_ty,
        "MIR ns1::gen must have ret = Generator<i64>, got {:?}",
        mir_gen.ret
    );

    let runtime_next_count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        runtime_next_count,
        1,
        "namespace-scoped `g.next()` must lower to exactly one RuntimeOp::GeneratorNext, \
         got count={runtime_next_count}, diags={:?}",
        ctx2.diagnostics()
    );

    let constructor_callees = {
        let mut out = Vec::new();
        collect_call_callees_in_block(&mir_main.body.block, &mut out);
        out
    };
    assert_eq!(
        constructor_callees.len(),
        1,
        "main must call exactly one constructor function (ns1.gen), got {:?}",
        constructor_callees
    );
    let ctor_callee = constructor_callees[0];
    assert_eq!(
        ctor_callee,
        mir_gen.id,
        "the constructor call inside `main` must resolve to `ns1::gen` (FunctionId {}), \
         not a bare name; got FunctionId {}",
        mir_gen.id.raw(),
        ctor_callee.raw()
    );
}

#[test]
fn propagate_generator_types_resolves_bare_name_global_call_in_namespace() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let gen_fn = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);

    let bare_constructor_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let next_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: g_local_id,
                ty: i64_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::from("next"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: Atom::from("g"),
                ty: i64_ty,
                init: Some(bare_constructor_call),
            },
            HirStmt::Expr { expr: next_call },
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(gen_fn), HirDecl::Function(main_fn)],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let runtime_next_count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        runtime_next_count,
        1,
        "bare-name `g` inside a namespace must lower `g.next()` to exactly one \
         RuntimeOp::GeneratorNext, got count={runtime_next_count}, diags={:?}",
        ctx2.diagnostics()
    );

    let mir_main = find_mir_function(&mir, "ns1::main")
        .expect("MIR must contain ns1::main (namespace-qualified identity preserved)");
    let mir_gen = find_mir_function(&mir, "ns1::gen")
        .expect("MIR must contain ns1::gen (namespace-qualified identity preserved)");

    assert_eq!(
        mir_gen.ret, gen_ty,
        "MIR ns1::gen must have ret = Generator<i64>, got {:?}",
        mir_gen.ret
    );

    let constructor_callees = {
        let mut out = Vec::new();
        collect_call_callees_in_block(&mir_main.body.block, &mut out);
        out
    };
    assert_eq!(
        constructor_callees.len(),
        1,
        "ns1::main must call exactly one constructor function, got {:?}",
        constructor_callees
    );
    let ctor_callee = constructor_callees[0];
    assert_eq!(
        ctor_callee,
        mir_gen.id,
        "the constructor call inside `ns1::main` must resolve to `ns1::gen` (FunctionId {}), \
         not a bare name; got FunctionId {}",
        mir_gen.id.raw(),
        ctor_callee.raw()
    );
}

#[test]
fn propagate_generator_types_resolves_chained_call_next() {
    let src = r"
        function* gen(): i64 {
            yield 1;
            return 2;
        }
        function main(): i64 {
            gen().next();
            return 0;
        }
        ";
    let (mir, _types, diags, _hir) = convert(src);
    assert!(
        !has_errors(&diags),
        "chained `gen().next()` must compile without errors, got: {diags:?}"
    );
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "chained `gen().next()` must lower to exactly one RuntimeOp::GeneratorNext, \
         got count={count}, diags={diags:?}"
    );
}

#[test]
fn propagate_generator_types_records_local_on_assignment() {
    let src = r"
        function* gen(): i64 {
            yield 1;
            return 0;
        }
        function main(): i64 {
            let g: i64 = 0;
            g = gen();
            return g.next();
        }
        ";
    let (mir, _types, diags, _hir) = convert(src);
    assert!(
        !has_errors(&diags),
        "local reassigned to a generator then `.next()` must compile without errors, \
         got: {diags:?}"
    );
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "`g = gen(); g.next();` must lower to exactly one RuntimeOp::GeneratorNext \
         (a local reassigned to a generator must be recorded in generator_locals so \
         the subsequent `.next()` call resolves the owner type to `Generator<...>`), \
         got count={count}, diags={diags:?}"
    );
}

#[test]
fn propagate_generator_types_does_not_shadow_root_generator_with_namespace_plain_fn() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let root_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let ns1_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(1, Span::default())),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);

    let bare_gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: Atom::from("g"),
                ty: i64_ty,
                init: Some(bare_gen_call),
            },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: g_local_id,
                    ty: i64_ty,
                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(root_gen));
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(ns1_gen), HirDecl::Function(main_fn)],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program must not error: {:?}",
        ctx2.diagnostics()
    );

    let mir_main =
        find_mir_function(&mir, "ns1::main").expect("MIR must contain `ns1::main` after lowering");
    let mir_root_gen =
        find_mir_function(&mir, "gen").expect("MIR must contain root-level `gen` after lowering");
    let mir_ns1_gen =
        find_mir_function(&mir, "ns1::gen").expect("MIR must contain `ns1::gen` after lowering");

    assert_eq!(
        mir_main.ret,
        i64_ty,
        "ns1::main ret must remain i64 (not Generator<i64>): the bare-name `gen` lookup inside \
         ns1::main must NOT fall back to the root-level `gen` generator name when propagating \
         Generator type info. A fall-through would have rewritten main's return type to \
         Generator<i64> via the root-level name match. got ret TypeId({})",
        mir_main.ret.raw()
    );
    assert_eq!(
        mir_root_gen.ret,
        gen_ty,
        "root-level `gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_root_gen.ret.raw()
    );
    assert_eq!(
        mir_ns1_gen.ret,
        i64_ty,
        "namespace `ns1::gen` is a plain function, so its MIR ret must remain i64, got TypeId({})",
        mir_ns1_gen.ret.raw()
    );

    let g_let = mir_main
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            ts_aot_ir_mir::MirStmt::Let { local, ty, .. } if *local == g_local_id => Some(*ty),
            _ => None,
        })
        .expect("ns1::main body must contain the `let g` binding");
    assert_eq!(
        g_let,
        i64_ty,
        "the `let g` binding in ns1::main must keep i64 type (the bare `gen()` call resolves to \
         the plain `ns1::gen`, NOT to the root-level `gen` generator). If g had been misclassified \
         as Generator<i64>, the Let ty would be gen_ty (TypeId({})). got g_let TypeId({})",
        gen_ty.raw(),
        g_let.raw()
    );

    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count,
        0,
        "no GeneratorNext dispatch may be emitted: the bare `gen` call inside ns1::main must \
         resolve to the plain `ns1::gen` (i64), NOT the root-level generator `gen`. A fall-through \
         to the root generator would have caused a GeneratorNext dispatch to be inserted. \
         got count={count}, diags={:?}",
        ctx2.diagnostics()
    );
}

#[test]
fn lower_generators_rejects_nested_generator_decl_in_function_body() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let nested = HirFunction {
        name: Atom::from("nested"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let outer = HirFunction {
        name: Atom::from("outer"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Decl(HirDecl::Function(nested)),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    hir.declarations.push(HirDecl::Function(outer));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert!(
        ctx.has_errors(),
        "nested generator function must trigger an E0501 diagnostic, got: {:?}",
        ctx.diagnostics()
    );
    assert_eq!(
        stats.generators_transformed, 0,
        "the outer function is not a generator, so generators_transformed must be 0"
    );
    assert_eq!(
        stats.generators_rejected, 1,
        "the nested generator decl must be rejected (one E0501 diagnostic), got stats: {:?}",
        stats
    );
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for nested generator decl");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("nested"),
        "message must mention nested, got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("nested") && diag.message.contains("hoist"),
        "message must explain the hoist-to-module-scope remediation, got: {:?}",
        diag.message
    );
}

#[test]
fn propagate_generator_types_qualifier_does_not_shadow_root_generator_with_namespace_plain_fn() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let root_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let ns1_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(1, Span::default())),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);

    let qualified_gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("ns1::gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: Atom::from("g"),
                ty: i64_ty,
                init: Some(qualified_gen_call),
            },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: g_local_id,
                    ty: i64_ty,
                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(root_gen));
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(ns1_gen), HirDecl::Function(main_fn)],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program must not error: {:?}",
        ctx2.diagnostics()
    );

    let mir_main =
        find_mir_function(&mir, "ns1::main").expect("MIR must contain `ns1::main` after lowering");
    let mir_root_gen =
        find_mir_function(&mir, "gen").expect("MIR must contain root-level `gen` after lowering");
    let mir_ns1_gen =
        find_mir_function(&mir, "ns1::gen").expect("MIR must contain `ns1::gen` after lowering");

    assert_eq!(
        mir_main.ret,
        i64_ty,
        "ns1::main ret must remain i64 (not Generator<i64>): the explicit qualifier `ns1::gen` \
         must NOT be classified as the root-level `gen` generator. Explicit qualifiers are \
         restricted to their exact generator entry, so the plain `ns1::gen` function must be \
         selected. got ret TypeId({})",
        mir_main.ret.raw()
    );
    assert_eq!(
        mir_root_gen.ret,
        gen_ty,
        "root-level `gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_root_gen.ret.raw()
    );
    assert_eq!(
        mir_ns1_gen.ret,
        i64_ty,
        "namespace `ns1::gen` is a plain function, so its MIR ret must remain i64, got TypeId({})",
        mir_ns1_gen.ret.raw()
    );

    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count,
        0,
        "no GeneratorNext dispatch may be emitted: the explicit `ns1::gen` call must resolve to \
         the plain `ns1::gen` (i64), NOT the root-level generator `gen`. got count={count}, diags={:?}",
        ctx2.diagnostics()
    );
}

#[test]
fn propagate_generator_types_resolves_root_generator_bare_from_namespace_without_shadowing() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let root_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let ns1_other = HirFunction {
        name: Atom::from("other"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(7, Span::default())),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);

    let bare_gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let next_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: g_local_id,
                ty: i64_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::from("next"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: Atom::from("g"),
                ty: i64_ty,
                init: Some(bare_gen_call),
            },
            HirStmt::Expr { expr: next_call },
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(root_gen));
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![HirDecl::Function(ns1_other), HirDecl::Function(main_fn)],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let mir_main =
        find_mir_function(&mir, "ns1::main").expect("MIR must contain `ns1::main` after lowering");
    let mir_root_gen =
        find_mir_function(&mir, "gen").expect("MIR must contain root-level `gen` after lowering");
    let mir_ns1_other = find_mir_function(&mir, "ns1::other")
        .expect("MIR must contain `ns1::other` after lowering");

    assert_eq!(
        mir_root_gen.ret,
        gen_ty,
        "root-level `gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_root_gen.ret.raw()
    );
    assert_eq!(
        mir_ns1_other.ret,
        i64_ty,
        "`ns1::other` is a plain function, so its MIR ret must remain i64, got TypeId({})",
        mir_ns1_other.ret.raw()
    );

    let g_let_ty = mir_main
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            ts_aot_ir_mir::MirStmt::Let { local, ty, .. } if *local == g_local_id => Some(*ty),
            _ => None,
        })
        .expect("ns1::main body must contain the `let g` binding");
    assert_eq!(
        g_let_ty,
        gen_ty,
        "the `let g` binding in `ns1::main` must be classified as Generator<i64> because the bare \
         `gen()` call must fall back to the root-level generator `gen` (no `ns1::gen` shadows it). \
         If lookup incorrectly returned None (the pre-fix bug), `g` would keep i64. \
         got g_let TypeId({}), expected TypeId({})",
        g_let_ty.raw(),
        gen_ty.raw()
    );

    let constructor_callees = {
        let mut out = Vec::new();
        collect_call_callees_in_block(&mir_main.body.block, &mut out);
        out
    };
    let ctor_callee = constructor_callees
        .iter()
        .find(|&&c| c == mir_root_gen.id)
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "the bare `gen()` constructor call inside `ns1::main` must resolve to the root \
                 `gen` (FunctionId {}), but no call to it was emitted. All callees: {:?}. \
                 Without the root-fallback fix, the bare lookup inside `ns1` returns None because \
                 the namespace loop fails and the old `if self.namespace_path.is_empty()` guard \
                 rejects the depth-0 fallthrough.",
                mir_root_gen.id.raw(),
                constructor_callees
            )
        });
    assert_eq!(
        ctor_callee,
        mir_root_gen.id,
        "the constructor call inside `ns1::main` must resolve to root `gen` (FunctionId {}), got \
         FunctionId {}",
        mir_root_gen.id.raw(),
        ctor_callee.raw()
    );

    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count,
        1,
        "bare-name `gen()` inside `ns1::main` must lower `g.next()` to exactly one \
         RuntimeOp::GeneratorNext because the bare lookup falls back to the root-level generator \
         `gen` (no `ns1::gen` shadow). got count={count}, diags={:?}",
        ctx2.diagnostics()
    );
}

#[test]
fn propagate_generator_types_skips_root_generator_shadowed_by_namespaced_global() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let root_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let x_local_id = LocalId::from_raw(0);

    let bare_gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: x_local_id,
                name: Atom::from("x"),
                ty: i64_ty,
                init: Some(bare_gen_call),
            },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: x_local_id,
                    ty: i64_ty,
                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(root_gen));
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("ns1"),
        members: vec![
            HirDecl::Global {
                name: Atom::from("gen"),
                ty: i64_ty,
                init: Some(HirExpr::Int(5, Span::default())),
            },
            HirDecl::Function(main_fn),
        ],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let mir_main =
        find_mir_function(&mir, "ns1::main").expect("MIR must contain `ns1::main` after lowering");
    let mir_root_gen =
        find_mir_function(&mir, "gen").expect("MIR must contain root-level `gen` after lowering");

    assert_eq!(
        mir_main.ret,
        i64_ty,
        "`ns1::main` ret must remain i64 (not Generator<i64>): the bare-name `gen` lookup inside \
         `ns1::main` must NOT fall back to the root-level `gen` generator because the namespace \
         declares its own same-named `global gen: i64 = 5`, which shadows the root. got ret \
         TypeId({})",
        mir_main.ret.raw()
    );
    assert_eq!(
        mir_root_gen.ret,
        gen_ty,
        "root-level `gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_root_gen.ret.raw()
    );

    let x_let_ty = mir_main
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            ts_aot_ir_mir::MirStmt::Let { local, ty, .. } if *local == x_local_id => Some(*ty),
            _ => None,
        })
        .expect("ns1::main body must contain the `let x` binding");
    assert_eq!(
        x_let_ty,
        i64_ty,
        "the `let x` binding in `ns1::main` must keep i64 type because the bare `gen()` call \
         resolves to the namespaced `global gen` (which shadows the root-level `gen` generator). If \
         the non_generator_names shadowing check missed `HirDecl::Global`, x would be misclassified \
         as Generator<i64> (TypeId({})). got x_let TypeId({})",
        gen_ty.raw(),
        x_let_ty.raw()
    );

    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count,
        0,
        "no GeneratorNext dispatch may be emitted: the bare `gen` call inside `ns1::main` must \
         resolve to the namespaced `global gen` (i64), NOT the root-level generator `gen`. Without \
         the non_generator_names shadowing check for `HirDecl::Global`, the root fallback would \
         fire and a GeneratorNext dispatch would be inserted. got count={count}, diags={:?}",
        ctx2.diagnostics()
    );
}

#[test]
fn propagate_generator_types_skips_outer_generator_shadowed_by_inner_namespace_non_generator() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });

    let root_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let a_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(2, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let b_gen = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(0, Span::default())),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let g_local_id = LocalId::from_raw(0);

    let bare_gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Let {
                id: g_local_id,
                name: Atom::from("g"),
                ty: i64_ty,
                init: Some(bare_gen_call),
            },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: g_local_id,
                    ty: i64_ty,
                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(root_gen));
    hir.declarations.push(HirDecl::Namespace {
        name: Atom::from("a"),
        members: vec![
            HirDecl::Function(a_gen),
            HirDecl::Namespace {
                name: Atom::from("b"),
                members: vec![HirDecl::Function(b_gen), HirDecl::Function(main_fn)],
            },
        ],
    });

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let mut ctx2 = PassContext::new();
    let mir = convert_program(&hir, &mut types, &mut ctx2);
    assert!(
        !ctx2.has_errors(),
        "convert_program errors: {:?}",
        ctx2.diagnostics()
    );

    let mir_root_gen =
        find_mir_function(&mir, "gen").expect("MIR must contain root-level `gen` after lowering");
    let mir_a_gen =
        find_mir_function(&mir, "a::gen").expect("MIR must contain `a::gen` after lowering");
    let mir_b_gen =
        find_mir_function(&mir, "a::b::gen").expect("MIR must contain `a::b::gen` after lowering");
    let mir_main = find_mir_function(&mir, "a::b::main")
        .expect("MIR must contain `a::b::main` after lowering");

    assert_eq!(
        mir_root_gen.ret,
        gen_ty,
        "root-level `gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_root_gen.ret.raw()
    );
    assert_eq!(
        mir_a_gen.ret,
        gen_ty,
        "`a::gen` is a generator, so its MIR ret must be Generator<i64>, got TypeId({})",
        mir_a_gen.ret.raw()
    );
    assert_eq!(
        mir_b_gen.ret,
        i64_ty,
        "`a::b::gen` is a plain function, so its MIR ret must remain i64, got TypeId({})",
        mir_b_gen.ret.raw()
    );
    assert_eq!(
        mir_main.ret,
        i64_ty,
        "`a::b::main` ret must remain i64 (not Generator<i64>): the bare `gen` lookup inside \
         `a::b::main` must NOT resolve to the outer `a::gen` generator because the inner namespace \
         `a::b` declares its own plain `gen` function that shadows it. The single-pass prefix \
         lookup must stop at `a::b::gen` (non-generator) and return None instead of falling \
         through to `a::gen`. got ret TypeId({})",
        mir_main.ret.raw()
    );

    let g_let_ty = mir_main
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            ts_aot_ir_mir::MirStmt::Let { local, ty, .. } if *local == g_local_id => Some(*ty),
            _ => None,
        })
        .expect("`a::b::main` body must contain the `let g` binding");
    assert_eq!(
        g_let_ty,
        i64_ty,
        "the `let g` binding in `a::b::main` must keep i64 type because the bare `gen()` call \
         resolves to the inner non-generator `a::b::gen`, NOT to the outer generator `a::gen`. If \
         the single-pass lookup incorrectly skipped over `a::b::gen` and matched `a::gen`, the let \
         binding would be misclassified as Generator<i64> (TypeId({})). got g_let TypeId({})",
        gen_ty.raw(),
        g_let_ty.raw()
    );

    let constructor_callees = {
        let mut out = Vec::new();
        collect_call_callees_in_block(&mir_main.body.block, &mut out);
        out
    };
    let gen_callee = constructor_callees
        .iter()
        .find(|&&c| c == mir_b_gen.id)
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "the bare `gen()` call inside `a::b::main` must resolve to the inner non-generator \
                 `a::b::gen` (FunctionId {}), but no call to it was emitted. All callees: {:?}. \
                 If the lookup incorrectly returned `a::gen` (FunctionId {}), the call would have \
                 been classified as a generator constructor call instead.",
                mir_b_gen.id.raw(),
                constructor_callees,
                mir_a_gen.id.raw()
            )
        });
    assert_eq!(
        gen_callee,
        mir_b_gen.id,
        "the bare `gen()` call inside `a::b::main` must resolve to `a::b::gen` (FunctionId {}), \
         got FunctionId {}",
        mir_b_gen.id.raw(),
        gen_callee.raw()
    );

    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count,
        0,
        "no GeneratorNext dispatch may be emitted: the bare `gen` call inside `a::b::main` must \
         resolve to the inner plain `a::b::gen` (i64), NOT the outer generator `a::gen`. Without \
         the unified single-pass prefix lookup, the old two-pass algorithm would have skipped \
         `a::b::gen` and matched `a::gen`, causing a GeneratorNext dispatch to be inserted. \
         got count={count}, diags={:?}",
        ctx2.diagnostics()
    );
}

#[test]
fn propagate_generator_types_does_not_reassign_closure_local_sharing_outer_local_id() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let unit_ty = types.intern(&Type::Void);

    let shared_local_id = LocalId::from_raw(0);

    let gen_fn = HirFunction {
        name: Atom::from("gen"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            yield_stmt(1, i64_ty),
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    let gen_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::from("gen"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty: i64_ty,
        span: Span::default(),
    };

    let outer_g_let = HirStmt::Let {
        id: shared_local_id,
        name: Atom::from("g"),
        ty: i64_ty,
        init: Some(gen_call),
    };

    let closure_body_g_let = HirStmt::Let {
        id: shared_local_id,
        name: Atom::from("g"),
        ty: i64_ty,
        init: Some(HirExpr::Int(42, Span::default())),
    };

    let closure_body_g_return = HirStmt::Return {
        value: Some(HirExpr::Local {
            id: shared_local_id,
            ty: i64_ty,
            span: Span::default(),
        }),
    };

    let closure = HirExpr::Closure {
        id: LocalId::from_raw(99),
        params: Vec::new(),
        captures: Vec::new(),
        body: vec![closure_body_g_let, closure_body_g_return],
        ty: unit_ty,
        span: Span::default(),
    };

    let main_fn = HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            outer_g_let,
            HirStmt::Expr { expr: closure },
            HirStmt::Return {
                value: Some(HirExpr::Local {
                    id: shared_local_id,
                    ty: i64_ty,
                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };

    hir.declarations.push(HirDecl::Function(gen_fn));
    hir.declarations.push(HirDecl::Function(main_fn));

    let _ = lower_generators(&mut hir, &mut types, &mut ctx);

    let main = hir
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Function(f) if f.name == Atom::from("main") => Some(f),
            _ => None,
        })
        .expect("main must be present");

    let outer_g_let_ty = match &main.body[0] {
        HirStmt::Let { ty, .. } => *ty,
        other => panic!("expected outer `let g` at body[0], got {other:?}"),
    };
    assert_eq!(
        outer_g_let_ty, gen_ty,
        "outer `let g = gen();` must be retyped to gen_ty after propagation"
    );

    let outer_g_return_ty = match &main.body[2] {
        HirStmt::Return {
            value: Some(HirExpr::Local { ty, .. }),
        } => *ty,
        other => panic!("expected outer `return g;` at body[2], got {other:?}"),
    };
    assert_eq!(
        outer_g_return_ty, gen_ty,
        "outer `return g;` Local must be retyped to gen_ty after propagation"
    );

    let closure_expr = match &main.body[1] {
        HirStmt::Expr { expr } => expr,
        other => panic!("expected closure Expr at body[1], got {other:?}"),
    };
    let closure_body = match closure_expr {
        HirExpr::Closure { body, .. } => body,
        other => panic!("expected HirExpr::Closure at body[1].expr, got {other:?}"),
    };

    let closure_g_let_ty = match &closure_body[0] {
        HirStmt::Let { ty, .. } => *ty,
        other => panic!("expected closure `let g = 42;` at closure[0], got {other:?}"),
    };
    assert_eq!(
        closure_g_let_ty,
        i64_ty,
        "closure body `let g = 42;` must keep i64 type \
         (its init is Int(42), not a generator, so the walker should not touch it), \
         got TypeId({})",
        closure_g_let_ty.raw()
    );

    let closure_g_return_ty = match &closure_body[1] {
        HirStmt::Return {
            value: Some(HirExpr::Local { ty, .. }),
        } => *ty,
        other => panic!("expected closure `return g;` at closure[1], got {other:?}"),
    };
    assert_eq!(
        closure_g_return_ty,
        i64_ty,
        "closure body `return g;` must keep i64 type \
         (closure-body LocalIds must not be matched against the outer-scope generator_locals; \
         otherwise the Local's type gets mis-reassigned to gen_ty because it shares \
         LocalId::from_raw(0) with the outer generator local). got TypeId({})",
        closure_g_return_ty.raw()
    );
}

#[test]
fn lower_generators_rejects_generator_nested_inside_closure_capture() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let nested_in_capture = HirFunction {
        name: Atom::from("nested"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let captured_closure = HirExpr::Closure {
        id: LocalId::from_raw(50),
        params: Vec::new(),
        captures: Vec::new(),
        body: vec![HirStmt::Decl(HirDecl::Function(nested_in_capture))],
        ty: unit_ty,
        span: Span::default(),
    };
    let outer_closure = HirExpr::Closure {
        id: LocalId::from_raw(99),
        params: Vec::new(),
        captures: vec![captured_closure],
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(0, Span::default())),
        }],
        ty: unit_ty,
        span: Span::default(),
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("outer"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![
            HirStmt::Expr {
                expr: outer_closure,
            },
            HirStmt::Return {
                value: Some(HirExpr::Int(0, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "generator nested inside a closure capture must be rejected, got stats: {:?}",
        stats
    );
    assert!(ctx.has_errors(), "E0501 diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for nested generator in closure capture");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("nested"),
        "message must name the rejected nested generator, got: {:?}",
        diag.message
    );
}

#[test]
fn lower_generators_rejects_generator_nested_inside_global_closure_init() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::new());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let i64_ty = types.intern(&Type::I64);
    let unit_ty = types.intern(&Type::Void);
    let nested_in_closure = HirFunction {
        name: Atom::from("nested"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![yield_stmt(1, i64_ty), HirStmt::Return { value: None }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let global_closure = HirExpr::Closure {
        id: LocalId::from_raw(99),
        params: Vec::new(),
        captures: Vec::new(),
        body: vec![HirStmt::Decl(HirDecl::Function(nested_in_closure))],
        ty: unit_ty,
        span: Span::default(),
    };
    hir.declarations.push(HirDecl::Global {
        name: Atom::from("f"),
        ty: unit_ty,
        init: Some(global_closure),
    });
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::from("main"),
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Int(0, Span::default())),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let stats = lower_generators(&mut hir, &mut types, &mut ctx);
    assert_eq!(
        stats.generators_rejected, 1,
        "generator nested inside a global closure initializer must be rejected, got stats: {:?}",
        stats
    );
    assert!(ctx.has_errors(), "E0501 diagnostic must be emitted");
    let diag = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0501")
        .expect("E0501 diagnostic must be present for nested generator in global init");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("nested"),
        "message must name the rejected nested generator, got: {:?}",
        diag.message
    );
}
