use super::common::*;

#[test]
fn convert_block_empty_produces_empty() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let (block, locals) = c.convert_block(&HirBlock(Vec::new()), &mut empty_types(), &mut cx);
    assert!(block.is_empty());
    assert!(locals.is_empty());
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_await_emits_mir_await_expr_without_temp_local() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(HirExpr::Await {
            expr: Box::new(int_lit(1)),
            ty: unit_ty(),

            span: Span::default(),
        }),
    }]);
    let (mir, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        locals.is_empty(),
        "await no longer needs a temp local (no state machine), got: {locals:?}"
    );
    assert!(
        matches!(
            &mir.stmts.as_slice(),
            [MirStmt::Return(Some(MirExpr::Await { expr: _, ty: _ }))],
        ),
        "expected Return(MirExpr::Await), got: {:?}",
        mir.stmts
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_direct_drains_new_alloc_temp_local() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(HirExpr::New {
            callee: Box::new(HirExpr::Global {
                name: Atom::new_inline("99"),
                ty: unit_ty(),

                span: Span::default(),
            }),
            args: Vec::new(),
            ty: unit_ty(),

            span: Span::default(),
        }),
    }]);
    let (_, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        locals.iter().any(|l| l.mutable),
        "new alloc must appear as mutable temp local in convert_block's locals"
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_let_creates_local_and_let_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::new_inline("11"),
        ty: unit_ty(),
        init: Some(int_lit(5)),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert_eq!(mir_block.len(), 1);
    assert_eq!(locals.len(), 1);
    assert_eq!(locals[0].name, Atom::new_inline("11"));
}

#[test]
fn convert_block_expr_emits_expr_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Expr { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Expr(_)));
}

#[test]
fn convert_block_return_emits_return() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(int_lit(0)),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Return(_)));
}

#[test]
fn convert_block_if_emits_if_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::If {
        cond: HirExpr::Bool(true, Span::default()),
        then: Box::new(HirStmt::Expr { expr: int_lit(1) }),
        otherwise: None,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::If { .. }));
}

#[test]
fn convert_block_while_emits_while() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(true, Span::default()),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::While { .. }));
}

#[test]
fn convert_block_while_cond_with_side_effects_keeps_cond_as_loop_condition() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let MirStmt::While { cond, body } = &mir_block.stmts[1] else {
        panic!(
            "expected MirStmt::While at index 1, got {:?}",
            mir_block.stmts[1]
        );
    };
    assert!(
        matches!(*cond, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0)),
        "MirStmt::While.cond must be the real cond expression (not Bool(true) forever-loop), got {:?}",
        cond
    );
    let inner_while_body = match &body.stmts[0] {
        MirStmt::While { body: inner, .. } => &inner.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_while_body
            .iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Int { value: 0, .. }))),
        "original body stmts must remain in inner-while body, got {:?}",
        inner_while_body
    );
}

#[test]
fn convert_block_while_false_does_not_loop_forever() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(false, Span::default()),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let MirStmt::While { cond, .. } = &mir_block.stmts[1] else {
        panic!("expected MirStmt::While at index 1");
    };
    assert!(matches!(*cond, MirExpr::Bool(false)));
    assert!(!matches!(*cond, MirExpr::Bool(true)));
}

#[test]

fn convert_block_while_continue_re_evaluates_cond_via_inner_wrapper() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Template {
        tag: None,
        expressions: vec![],
        cooked_parts: vec![None],
        raw_parts: vec![None],
        ty: unit_ty(),

        span: Span::default(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Continue { label: None }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while_idx = mir_block
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected outer MirStmt::While");
    let outer_while = match &mir_block.stmts[outer_while_idx] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected MirStmt::While, got {other:?}"),
    };
    let inner_while = match &outer_while.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_while.iter().any(|s| matches!(s, MirStmt::Break)),
        "user's Continue must be rewritten to MirStmt::Break targeting the inner wrapper, got {:?}",
        inner_while
    );
    let cond_let_idx = outer_while
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::Let { .. }))
        .expect("cond Let (1-part template) must be present in outer-while body");
    let inner_while_idx_in_outer = 0;
    assert!(
        cond_let_idx > inner_while_idx_in_outer,
        "cond Let (idx {}) must appear AFTER the inner-while wrapper (idx {}) so cond re-evaluates each iteration (1-part template emits Let, not Runtime); got stmts {:?}",
        cond_let_idx,
        inner_while_idx_in_outer,
        outer_while.stmts
    );
}

#[test]
fn convert_block_while_break_breaks_outer_via_sentinel() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(true, Span::default()),
        body: Box::new(HirStmt::Break { label: None }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while = match &mir_block.stmts[1] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected MirStmt::While at index 1, got {other:?}"),
    };
    let inner_while = match &outer_while.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    let has_assign_then_break = inner_while.windows(2).any(|w| {
        matches!(
            w[0],
            MirStmt::Assign {
                target: MirPlace::Local { .. },
                value: MirExpr::Bool(true),
            }
        ) && matches!(w[1], MirStmt::Break)
    });
    assert!(
        has_assign_then_break,
        "user's Break must be rewritten to is_break=true; Break targeting the inner wrapper, got {:?}",
        inner_while
    );
}

#[test]
fn convert_block_dowhile_executes_body_at_least_once() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Bool(false, Span::default()),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let body_stmts = match &mir_block.stmts[2] {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 2, got {other:?}"),
    };
    let inner_while_body = match &body_stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner While, got {other:?}"),
    };
    assert!(
        inner_while_body
            .iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Int { value: 0, .. }))),
        "body stmts must end up in inner-while, got {:?}",
        inner_while_body
    );
}

#[test]
fn convert_block_dowhile_continue_still_evaluates_cond() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Continue { label: None }),
        cond: HirExpr::Bool(false, Span::default()),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let while_stmt = &mir_block.stmts[2];
    let while_body = match while_stmt {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 2, got {other:?}"),
    };
    let inner_while_body = match &while_body[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner While, got {other:?}"),
    };
    assert!(
        inner_while_body.iter().any(|s| matches!(s, MirStmt::Break)),
        "user's Continue must be rewritten to Break targeting the inner wrapper, got {:?}",
        inner_while_body
    );
    let cond = match while_stmt {
        MirStmt::While { cond, .. } => cond,
        _ => unreachable!(),
    };
    assert!(
        matches!(
            cond,
            MirExpr::Binary {
                op: BinaryOp::Or,
                ..
            }
        ),
        "while cond must be `__first || cond`, got {cond:?}"
    );
}

#[test]

fn convert_block_while_call_cond_evaluated_once_per_iteration() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: Vec::new(),
            ty: unit_ty(),
            type_args: vec![],

            span: Span::default(),
        },
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while = mir_block
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::While { cond, .. } => Some(cond),
            _ => None,
        })
        .expect("expected outer MirStmt::While");
    assert!(
        matches!(*outer_while, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0)),
        "While.cond must hold the original Call (re-evaluated each iter by the header itself), got {outer_while:?}"
    );
    let outer_while_body = match mir_block.stmts.last().expect("non-empty") {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected MirStmt::While, got {other:?}"),
    };
    let contains_not_call_break = outer_while_body.iter().any(|s| {
        matches!(
            s,
            MirStmt::If {
                cond: MirExpr::Unary {
                    op: UnaryOp::Not,
                    expr,
                    ..
                },
                ..
            } if matches!(**expr, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0))
        )
    });
    assert!(
        !contains_not_call_break,
        "loop body must NOT contain `if !Call break` (would call the function a second time per iter); got {:?}",
        outer_while_body
    );
}

#[test]
fn convert_block_dowhile_false_runs_body_exactly_once_not_infinite() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Bool(false, Span::default()),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let outer_while = match &mir_block.stmts[2] {
        MirStmt::While { cond, body } => (cond, body),
        other => panic!("expected MirStmt::While at index 2, got {other:?}"),
    };
    let first_id_local = match outer_while.0 {
        MirExpr::Binary {
            op: BinaryOp::Or,
            left,
            ..
        } => match left.as_ref() {
            MirExpr::Local(id) => *id,
            other => panic!("expected first_id Local, got {other:?}"),
        },
        other => panic!("expected first_id || cond_mir, got {other:?}"),
    };
    let inner_while = match &outer_while.1.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    let first_id_reset = inner_while.iter().any(|s| {
        matches!(
            s,
            MirStmt::Assign {
                target: MirPlace::Local { id },
                value: MirExpr::Bool(false),
            } if *id == first_id_local
        )
    });
    assert!(
        first_id_reset,
        "first_id must be reset to false inside the inner wrapper so the next iter's outer-while entry checks cond_mir (and `do {{}} while (false)` doesn't infinite-loop), got inner stmts {:?}",
        inner_while
    );
}

#[test]
fn convert_block_forof_emits_forof() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::ForOf {
        binding: LocalId::from_raw(0),
        iter: int_lit(0),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::ForOf { .. }));
    assert_eq!(locals.len(), 1);
}

#[test]
fn convert_block_forin_emits_forin_not_forof() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::ForIn {
        binding: LocalId::from_raw(0),
        iter: int_lit(0),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        matches!(mir_block.stmts[0], MirStmt::ForIn { .. }),
        "HirStmt::ForIn must lower to MirStmt::ForIn (got {:?})",
        mir_block.stmts[0]
    );
    assert!(!matches!(mir_block.stmts[0], MirStmt::ForOf { .. }));
    assert_eq!(locals.len(), 1);
}

#[test]
fn convert_block_break_continue_pass_through() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![
        HirStmt::Break { label: None },
        HirStmt::Continue { label: None },
    ]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Break));
    assert!(matches!(mir_block.stmts[1], MirStmt::Continue));
}

#[test]
fn convert_block_throw_emits_throw() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Throw { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Throw { .. }));
}

#[test]
fn convert_block_switch_emits_switch_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![
            ts_aot_ir_hir::HirSwitchCase::new(
                Some(HirExpr::Int(1, Span::default())),
                vec![HirStmt::ret(None)],
            ),
            ts_aot_ir_hir::HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(matches!(mir_block.stmts[0], MirStmt::Switch { .. }));
    if let MirStmt::Switch {
        disc,
        cases,
        default,
    } = &mir_block.stmts[0]
    {
        assert!(matches!(disc.as_ref(), MirExpr::Int { .. }));
        assert_eq!(cases.len(), 1);
        assert!(matches!(cases[0].value, ConstValue::Int(1)));
        assert!(default.is_some());
    } else {
        panic!("expected MirStmt::Switch");
    }
}

#[test]
fn convert_block_switch_non_terminating_case_inserts_implicit_break() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(1, Span::default())),
            vec![HirStmt::expr(int_lit(0))],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0005"),
        "non-terminating case must emit a fall-through P0005 warning"
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let last_stmt = cases[0]
        .body
        .stmts
        .last()
        .expect("case body must have at least one stmt");
    assert!(
        matches!(last_stmt, MirStmt::Break),
        "non-terminating case body must end with implicit MirStmt::Break, got {last_stmt:?}"
    );
}

#[test]
fn convert_block_switch_terminating_case_does_not_insert_break() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(1, Span::default())),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0005"),
        "terminating case must not emit P0005 warning"
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let last_stmt = cases[0].body.stmts.last().expect("case body");
    assert!(
        matches!(last_stmt, MirStmt::Return(_)),
        "terminating case must keep its terminator, not get an extra Break, got {last_stmt:?}"
    );
}

#[test]
fn convert_block_switch_case_preserves_full_i128_int_value() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(7, Span::default())),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let ConstValue::Int(stored) = &cases[0].value else {
        panic!("expected ConstValue::Int");
    };
    assert_eq!(*stored, i128::from(7));
    assert!(
        !cx.diagnostics()
            .iter()
            .any(|d| d.message.contains("does not fit in i64")),
        "ConstValue::Int(i128) storage must not emit i64-overflow fallback diagnostic anymore"
    );
}

#[test]
fn convert_block_switch_non_const_case_value_emits_p0006_error() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0, Span::default()),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Binary {
                op: ts_aot_ir_hir::HirBinaryOp::Add,
                lhs: Box::new(HirExpr::Int(1, Span::default())),
                rhs: Box::new(HirExpr::Int(2, Span::default())),
                ty: TypeId::from_raw(0),

                span: Span::default(),
            }),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        cx.has_errors(),
        "non-const case value (Binary expression) must emit a hard error, not a warning, \
         so compilation fails instead of silently dropping the case"
    );
    let p0006 = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0006")
        .expect("expected P0006 diagnostic for non-const case value");
    assert!(
        p0006.message.contains("switch case"),
        "P0006 message must clearly identify switch-case context, got: {}",
        p0006.message
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    assert!(
        cases.is_empty(),
        "non-const case value must be skipped (continue), not pushed as a malformed SwitchCase, got {} cases",
        cases.len()
    );
}

#[test]
fn convert_block_try_emits_try_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Try {
        body: Box::new(HirStmt::ret(None)),
        catch: Some(ts_aot_ir_hir::HirCatchClause::new(
            None,
            Box::new(HirStmt::ret(None)),
        )),
        finally: None,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(matches!(mir_block.stmts[0], MirStmt::Try { .. }));
    if let MirStmt::Try {
        body,
        catch_param,
        catch,
        finally,
    } = &mir_block.stmts[0]
    {
        assert_eq!(body.stmts.len(), 1);
        assert!(catch_param.is_none());
        assert!(catch.is_some());
        assert_eq!(catch.as_ref().unwrap().stmts.len(), 1);
        assert!(finally.is_none());
    } else {
        panic!("expected MirStmt::Try");
    }
}

#[test]
fn convert_block_try_finally_without_catch_emits_optional_catch_none() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Try {
        body: Box::new(HirStmt::ret(None)),
        catch: None,
        finally: Some(Box::new(HirStmt::ret(None))),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Try {
        body,
        catch,
        catch_param,
        finally,
    } = &mir_block.stmts[0]
    else {
        panic!("expected MirStmt::Try");
    };
    assert_eq!(body.stmts.len(), 1);
    assert!(
        catch.is_none(),
        "try-finally without catch clause must preserve `catch: None`, not encode as empty MirBlock. got: {catch:?}"
    );
    assert!(catch_param.is_none());
    assert!(finally.is_some());
}

#[test]
fn convert_block_expr_compound_update_emits_local_expr_stmt_not_binary() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Expr {
        expr: HirExpr::CompoundUpdate {
            target: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),

                span: Span::default(),
            }),
            op: HirBinaryOp::Add,
            rhs: Box::new(int_lit(1)),
            post: true,
            ty: unit_ty(),

            span: Span::default(),
        },
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());

    let trailing = mir_block
        .stmts
        .iter()
        .rev()
        .find_map(|s| match s {
            MirStmt::Expr(e) => Some(e),
            _ => None,
        })
        .expect("expression statement must emit MirStmt::Expr");
    assert!(
        matches!(trailing, MirExpr::Local(_)),
        "statement-level compound update must end in a load of the materialized temp, not a Binary that re-runs rhs, got {trailing:?}"
    );
}

#[test]
fn convert_block_expr_plain_assignment_returns_local_not_rhs() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(404)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let block = HirBlock(vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),

                span: Span::default(),
            }),
            value: Box::new(rhs_call),
            ty: unit_ty(),

            span: Span::default(),
        },
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());

    let rhs_call_count: usize = mir_block
        .stmts
        .iter()
        .map(|s| count_calls_in_stmt(s, 404))
        .sum();
    assert_eq!(
        rhs_call_count, 1,
        "statement-level `a = sideEffect()` must invoke sideEffect exactly once across the whole block (Assign + Expr trailing), got {mir_block:?}"
    );

    let trailing = mir_block
        .stmts
        .iter()
        .rev()
        .find_map(|s| match s {
            MirStmt::Expr(e) => Some(e),
            _ => None,
        })
        .expect("expression statement must emit MirStmt::Expr");
    assert!(
        matches!(trailing, MirExpr::Local(_)),
        "statement-level plain assignment must end in MirStmt::Expr(Local(value_temp)), not a re-evaluation of the RHS expression, got {trailing:?}"
    );
}
