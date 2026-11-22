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
        narrowed: false,
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert_eq!(mir_block.len(), 1);
    assert_eq!(locals.len(), 1);
    assert_eq!(locals[0].name, Atom::new_inline("11"));
}

#[test]
fn convert_let_without_reassignment_keeps_immutable() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::new_inline("1"),
        ty: unit_ty(),
        init: Some(int_lit(1)),
        narrowed: false,
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Let { mutable, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Let, got {:?}", mir_block.stmts[0]);
    };
    assert!(
        !mutable,
        "let x = 1; with no reassignment must produce mutable: false, got {mutable}"
    );
    assert!(
        !locals[0].mutable,
        "MirLocalDecl for unreassigned let must be immutable, got {locals:?}"
    );
}

#[test]
fn convert_let_with_reassignment_marks_mutable() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![
        HirStmt::Let {
            id: LocalId::from_raw(0),
            name: Atom::new_inline("1"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Expr {
            expr: HirExpr::Assignment {
                target: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                value: Box::new(int_lit(2)),
                ty: unit_ty(),
                span: Span::default(),
            },
        },
    ]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Let { mutable, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Let, got {:?}", mir_block.stmts[0]);
    };
    assert!(
        *mutable,
        "let x = 1; x = 2; must produce mutable: true, got {mutable}"
    );
    assert!(
        locals[0].mutable,
        "MirLocalDecl for reassigned let must be mutable, got {locals:?}"
    );
}

fn assignment_to(id: LocalId, value: i64) -> HirStmt {
    HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(HirExpr::Local {
                id,
                ty: unit_ty(),
                span: Span::default(),
            }),
            value: Box::new(int_lit(value)),
            ty: unit_ty(),
            span: Span::default(),
        },
    }
}

fn assert_let_mutability(block: HirBlock, expected: bool, label: &str) {
    assert_let_mutability_with_types(block, expected, label, &mut empty_types());
}

fn assert_let_mutability_with_types(
    block: HirBlock,
    expected: bool,
    label: &str,
    types: &mut TypeTable,
) {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let _ = types.intern(&Type::I64);
    let (mir_block, locals) = c.convert_block(&block, types, &mut cx);
    assert!(
        !cx.has_errors(),
        "{label} must compile, got: {:?}",
        cx.diagnostics()
    );
    let MirStmt::Let { mutable, .. } = &mir_block.stmts[0] else {
        panic!(
            "{label} expected first stmt to be MirStmt::Let, got {:?}",
            mir_block.stmts[0]
        );
    };
    assert!(
        *mutable == expected,
        "{label} must produce mutable: {expected}, got {mutable}"
    );
    assert!(
        locals[0].mutable == expected,
        "{label} MirLocalDecl must be mutable: {expected}, got {locals:?}"
    );
}

#[test]
fn convert_let_reassigned_inside_if_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(assignment_to(x, 2)),
            otherwise: None,
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; if (true) { x = 2 }");
}

#[test]
fn convert_let_reassigned_inside_while_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::While {
            cond: HirExpr::Bool(false, Span::default()),
            body: Box::new(assignment_to(x, 2)),
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; while (false) { x = 2 }");
}

#[test]
fn convert_let_reassigned_inside_dowhile_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::DoWhile {
            body: Box::new(assignment_to(x, 2)),
            cond: HirExpr::Bool(false, Span::default()),
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; do { x = 2 } while (false)");
}

#[test]
fn convert_let_reassigned_inside_forof_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::ForOf {
            binding: LocalId::from_raw(1),
            iter: HirExpr::Local {
                id: LocalId::from_raw(2),
                ty: arr_ty,
                span: Span::default(),
            },
            body: Box::new(assignment_to(x, 2)),
        },
    ]);
    assert_let_mutability_with_types(
        block,
        true,
        "let x = 1; for (y of []) { x = 2 }",
        &mut types,
    );
}

#[test]
fn convert_let_reassigned_inside_forin_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::ForIn {
            binding: LocalId::from_raw(1),
            iter: HirExpr::Unit(Span::default()),
            body: Box::new(assignment_to(x, 2)),
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; for (y in {}) { x = 2 }");
}

#[test]
fn convert_let_reassigned_inside_switch_case_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let case = HirSwitchCase::new(Some(int_lit(1)), vec![assignment_to(x, 2)]);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Switch {
            disc: int_lit(1),
            cases: vec![case],
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; switch (1) { case 1: x = 2 }");
}

#[test]
fn convert_let_reassigned_in_any_of_two_switch_cases_marks_mutable() {
    let x = LocalId::from_raw(0);
    let case1 = HirSwitchCase::new(Some(int_lit(1)), vec![assignment_to(x, 2)]);
    let case2 = HirSwitchCase::new(Some(int_lit(2)), vec![assignment_to(x, 3)]);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(0)),
            narrowed: false,
        },
        HirStmt::Switch {
            disc: int_lit(1),
            cases: vec![case1, case2],
        },
    ]);
    assert_let_mutability(
        block,
        true,
        "let x = 0; switch (1) { case 1: x = 2; case 2: x = 3 } — a let reassigned in any case \
         body must be marked mutable (positive regression check for the switch case mutability \
         path; the case body inherits the enclosing mutable_locals)",
    );
}

#[test]
fn convert_let_reassigned_inside_try_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Try {
            body: Box::new(assignment_to(x, 2)),
            catch: None,
            finally: None,
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; try { x = 2 } catch {}");
}

#[test]
fn convert_let_reassigned_inside_catch_body_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Try {
            body: Box::new(HirStmt::Throw {
                expr: HirExpr::Unit(Span::default()),
            }),
            catch: Some(ts_aot_ir_hir::HirCatchClause::new(
                None,
                Box::new(assignment_to(x, 2)),
            )),
            finally: None,
        },
    ]);
    assert_let_mutability(block, true, "let x = 1; try { throw 0 } catch { x = 2 }");
}

#[test]
fn convert_let_reassigned_inside_block_scope_marks_mutable() {
    let x = LocalId::from_raw(0);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Block(vec![assignment_to(x, 2)]),
    ]);
    assert_let_mutability(block, true, "let x = 1; { x = 2 }");
}

#[test]
fn convert_let_unrelated_assignment_in_block_keeps_immutable() {
    let x = LocalId::from_raw(0);
    let y = LocalId::from_raw(1);
    let block = HirBlock(vec![
        HirStmt::Let {
            id: x,
            name: Atom::new_inline("x"),
            ty: unit_ty(),
            init: Some(int_lit(1)),
            narrowed: false,
        },
        HirStmt::Block(vec![assignment_to(y, 2)]),
    ]);
    assert_let_mutability(block, false, "let x = 1; { y = 2 } (y is unrelated to x)");
}

fn assert_nested_let_mutable(stmts: Vec<HirStmt>, label: &str) {
    assert_nested_let_mutable_with_types(stmts, label, &mut empty_types());
}

fn assert_nested_let_mutable_with_types(stmts: Vec<HirStmt>, label: &str, types: &mut TypeTable) {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let _ = types.intern(&Type::I64);
    let block = HirBlock(stmts);
    let (mir_block, locals) = c.convert_block(&block, types, &mut cx);
    assert!(
        !cx.has_errors(),
        "{label} must compile, got: {:?}",
        cx.diagnostics()
    );
    let x_local = locals
        .iter()
        .find(|l| l.name == Atom::new_inline("x"))
        .unwrap_or_else(|| panic!("{label} must declare local 'x', got locals: {locals:?}"));
    assert!(
        x_local.mutable,
        "{label} MirLocalDecl for nested let must be mutable, got: {x_local:?}"
    );
    let nested_let = has_mutable_let_in_nested_body(&mir_block);
    assert!(
        nested_let,
        "{label} must contain a MirStmt::Let with mutable: true somewhere in the nested body, got: {mir_block:?}"
    );
}

fn has_mutable_let_in_nested_body(block: &MirBlock) -> bool {
    fn visit_stmt(stmt: &MirStmt, found: &mut bool) {
        match stmt {
            MirStmt::Let { mutable, .. } => {
                if !*found && *mutable {
                    *found = true;
                }
            }
            MirStmt::If {
                then_block,
                else_block,
                ..
            } => {
                for s in &then_block.stmts {
                    visit_stmt(s, found);
                }
                if let Some(eb) = else_block {
                    for s in &eb.stmts {
                        visit_stmt(s, found);
                    }
                }
            }
            MirStmt::While { body, .. } | MirStmt::DoWhile { body, .. } => {
                for s in &body.stmts {
                    visit_stmt(s, found);
                }
            }
            MirStmt::ForOf { body, .. } | MirStmt::ForIn { body, .. } => {
                for s in &body.stmts {
                    visit_stmt(s, found);
                }
            }
            MirStmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                for s in &body.stmts {
                    visit_stmt(s, found);
                }
                if let Some(c) = catch {
                    for s in &c.stmts {
                        visit_stmt(s, found);
                    }
                }
                if let Some(f) = finally {
                    for s in &f.stmts {
                        visit_stmt(s, found);
                    }
                }
            }
            MirStmt::Switch { cases, default, .. } => {
                for case in cases {
                    for s in &case.body.stmts {
                        visit_stmt(s, found);
                    }
                }
                if let Some(d) = default {
                    for s in &d.stmts {
                        visit_stmt(s, found);
                    }
                }
            }
            _ => {}
        }
    }
    let mut found = false;
    for s in &block.stmts {
        visit_stmt(s, &mut found);
    }
    found
}

#[test]
fn convert_let_inside_if_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::If {
        cond: HirExpr::Bool(true, Span::default()),
        then: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
        otherwise: None,
    }];
    assert_nested_let_mutable(stmts, "if (cond) { let x = 1; x = 2; }");
}

#[test]
fn convert_let_inside_while_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::While {
        cond: HirExpr::Bool(false, Span::default()),
        body: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
    }];
    assert_nested_let_mutable(stmts, "while (cond) { let x = 1; x = 2; }");
}

#[test]
fn convert_let_inside_dowhile_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
        cond: HirExpr::Bool(false, Span::default()),
    }];
    assert_nested_let_mutable(stmts, "do { let x = 1; x = 2; } while (cond)");
}

#[test]
fn convert_let_inside_forof_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let stmts = vec![HirStmt::ForOf {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Local {
            id: LocalId::from_raw(2),
            ty: arr_ty,
            span: Span::default(),
        },
        body: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
    }];
    assert_nested_let_mutable_with_types(stmts, "for (y of []) { let x = 1; x = 2; }", &mut types);
}

#[test]
fn convert_let_inside_forin_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::ForIn {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Unit(Span::default()),
        body: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
    }];
    assert_nested_let_mutable(stmts, "for (y in {}) { let x = 1; x = 2; }");
}

#[test]
fn convert_let_inside_try_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::Try {
        body: Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ])),
        catch: None,
        finally: None,
    }];
    assert_nested_let_mutable(stmts, "try { let x = 1; x = 2; } catch {}");
}

#[test]
fn convert_let_inside_catch_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::Try {
        body: Box::new(HirStmt::Throw {
            expr: HirExpr::Unit(Span::default()),
        }),
        catch: Some(ts_aot_ir_hir::HirCatchClause::new(
            None,
            Box::new(HirStmt::Block(vec![
                HirStmt::Let {
                    id: x,
                    name: Atom::new_inline("x"),
                    ty: unit_ty(),
                    init: Some(int_lit(1)),
                    narrowed: false,
                },
                assignment_to(x, 2),
            ])),
        )),
        finally: None,
    }];
    assert_nested_let_mutable(stmts, "try { throw 0 } catch { let x = 1; x = 2; }");
}

#[test]
fn convert_let_inside_finally_body_reassigned_marks_mutable() {
    let x = LocalId::from_raw(0);
    let stmts = vec![HirStmt::Try {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        catch: None,
        finally: Some(Box::new(HirStmt::Block(vec![
            HirStmt::Let {
                id: x,
                name: Atom::new_inline("x"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
                narrowed: false,
            },
            assignment_to(x, 2),
        ]))),
    }];
    assert_nested_let_mutable(stmts, "try { 0 } finally { let x = 1; x = 2; }");
}

#[test]
fn convert_let_assigned_via_field_target_marks_mutable() {
    let o = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("x"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            value: Box::new(int_lit(2)),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(o, &body, &empty_types()),
        "let o = 1; o.x = 2 (field assignment) must mark o as mutable"
    );
}

#[test]
fn convert_let_assigned_via_index_target_marks_mutable() {
    let arr = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(HirExpr::Index {
                owner: Box::new(HirExpr::Local {
                    id: arr,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                index: Box::new(int_lit(0)),
                ty: unit_ty(),
                span: Span::default(),
            }),
            value: Box::new(int_lit(2)),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(arr, &body, &empty_types()),
        "let arr = 1; arr[0] = 2 (index assignment) must mark arr as mutable"
    );
}

#[test]
fn convert_let_assigned_via_compound_update_field_marks_mutable() {
    let o = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::CompoundUpdate {
            target: Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("x"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            op: HirBinaryOp::Add,
            rhs: Box::new(int_lit(1)),
            post: false,
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(o, &body, &empty_types()),
        "let o = 1; o.x += 1 (compound update on field) must mark o as mutable"
    );
}

#[test]
fn convert_let_consumed_by_forof_iter_marks_mutable() {
    let g = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let body = vec![HirStmt::ForOf {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Local {
            id: g,
            ty: gen_ty,
            span: Span::default(),
        },
        body: Box::new(HirStmt::ret(None)),
    }];
    assert!(
        is_local_reassigned(g, &body, &types),
        "let g = gen(); for (x of g) (for-of over Generator consumes &mut) must mark g as mutable"
    );
}

#[test]
fn convert_let_consumed_by_forof_over_array_does_not_mark_mutable() {
    let arr = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let body = vec![HirStmt::ForOf {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Local {
            id: arr,
            ty: arr_ty,
            span: Span::default(),
        },
        body: Box::new(HirStmt::ret(None)),
    }];
    assert!(
        !is_local_reassigned(arr, &body, &types),
        "let arr: i64[]; for (x of arr) (for-of over Array takes &[T], not &mut) must NOT mark arr as mutable"
    );
}

#[test]
fn convert_let_consumed_by_forin_over_array_does_not_mark_mutable() {
    let arr = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let body = vec![HirStmt::ForIn {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Local {
            id: arr,
            ty: arr_ty,
            span: Span::default(),
        },
        body: Box::new(HirStmt::ret(None)),
    }];
    assert!(
        !is_local_reassigned(arr, &body, &types),
        "let arr: i64[]; for (k in arr) (for-in over Array does not take &mut) must NOT mark arr as mutable"
    );
}

#[test]
fn convert_let_consumed_by_next_on_non_generator_type_does_not_mark_mutable() {
    let o = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: arr_ty,
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("next"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        !is_local_reassigned(o, &body, &types),
        "let o: i64[]; o.next() (next on a known non-Generator type) must NOT mark o as mutable; \
         the wrapper only treats deferred generator methods on Generator (or unknown) types as \
         evidence of mutability"
    );
}

#[test]
fn convert_let_consumed_by_return_on_non_generator_type_does_not_mark_mutable() {
    let o = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: arr_ty,
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("return"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        !is_local_reassigned(o, &body, &types),
        "let o: i64[]; o.return() (deferred generator method on a known non-Generator type) must \
         NOT mark o as mutable; the wrapper restricts the receiver-mutable mark to types that \
         resolve to Generator"
    );
}

#[test]
fn convert_let_consumed_by_generator_next_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: g,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("next"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); g.next() (consumes &mut self) must mark g as mutable"
    );
}

#[test]
fn convert_let_consumed_by_return_on_generator_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: g,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("return"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); g.return() (consumes &mut self) must mark g as mutable — \
         `return` is in the deferred generator method set (next/return/throw) and the owner \
         type is unknown, so the wrapper treats it as a generator method dispatch"
    );
}

#[test]
fn convert_let_consumed_by_throw_on_generator_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: g,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("throw"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); g.throw() (consumes &mut self) must mark g as mutable — \
         `throw` is in the deferred generator method set (next/return/throw) and the owner \
         type is unknown, so the wrapper treats it as a generator method dispatch"
    );
}

#[test]
fn convert_let_consumed_by_throw_on_non_generator_type_does_not_mark_mutable() {
    let o = LocalId::from_raw(0);
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: arr_ty,
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("throw"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        !is_local_reassigned(o, &body, &types),
        "let o: i64[]; o.throw() (deferred generator method on a known non-Generator type) must \
         NOT mark o as mutable; the wrapper restricts the receiver-mutable mark to types that \
         resolve to Generator"
    );
}

#[test]
fn convert_let_consumed_by_next_inside_forof_iter_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::ForOf {
        binding: LocalId::from_raw(1),
        iter: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: g,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("next"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
        body: Box::new(HirStmt::ret(None)),
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); for (x of g.next()) (g.next() inside for-of iter) must mark g as mutable"
    );
}

#[test]
fn convert_let_in_while_cond_consuming_next_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::While {
        cond: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: g,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("next"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
        body: Box::new(HirStmt::ret(None)),
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); while (g.next()) (next in cond) must mark g as mutable"
    );
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
        matches!(*cond, MirExpr::Bool(true)),
        "the header must be `while true`: the real condition is checked inside the body so the \
         statements it needs are declared before the first check, got {:?}",
        cond
    );
    let MirStmt::If {
        cond: guard,
        then_block,
        ..
    } = &body.stmts[0]
    else {
        panic!(
            "the loop body must start with the condition check, got {:?}",
            body.stmts
        );
    };
    assert!(
        matches!(
            guard,
            MirExpr::Unary {
                op: UnaryOp::Not,
                expr,
                ..
            } if matches!(**expr, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0))
        ),
        "the check must negate the real cond expression, got {guard:?}"
    );
    assert!(
        matches!(then_block.stmts.as_slice(), [MirStmt::Break]),
        "a false cond must break the loop, got {then_block:?}"
    );
    assert_eq!(
        body.stmts
            .iter()
            .map(|s| count_calls_in_stmt(s, FunctionId::from_raw(0).raw()))
            .sum::<usize>(),
        1,
        "the cond call must be evaluated exactly once per iteration, got {:?}",
        body.stmts
    );
    let inner_while_body = body
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::While { body: inner, .. } => Some(&inner.stmts),
            _ => None,
        })
        .expect("the loop body wrapper must still be an inner MirStmt::While");
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
    let MirStmt::While { cond, body } = &mir_block.stmts[1] else {
        panic!("expected MirStmt::While at index 1");
    };
    assert!(matches!(*cond, MirExpr::Bool(true)));
    let MirStmt::If { cond: guard, .. } = &body.stmts[0] else {
        panic!(
            "the loop body must start with the condition check, got {:?}",
            body.stmts
        );
    };
    assert!(
        matches!(
            guard,
            MirExpr::Unary { op: UnaryOp::Not, expr, .. } if matches!(**expr, MirExpr::Bool(false))
        ),
        "`while (false)` must break before the body runs, got {guard:?}"
    );
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
    let cond_let_idx = outer_while
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::Let { .. }))
        .expect("cond Let (1-part template) must be present in outer-while body");
    let check_idx = outer_while
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::If {
                    cond: MirExpr::Unary {
                        op: UnaryOp::Not,
                        ..
                    },
                    ..
                }
            )
        })
        .expect("the outer-while body must check the condition");
    let inner_while_idx_in_outer = outer_while
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected inner MirStmt::While");
    assert!(
        cond_let_idx < check_idx && check_idx < inner_while_idx_in_outer,
        "cond Let (idx {}) must be declared inside the loop body BEFORE the check (idx {}) and the \
         wrapper (idx {}), so the first check sees it and every iteration re-evaluates it; got \
         stmts {:?}",
        cond_let_idx,
        check_idx,
        inner_while_idx_in_outer,
        outer_while.stmts
    );
    let inner_while = match &outer_while.stmts[inner_while_idx_in_outer] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_while.iter().any(|s| matches!(s, MirStmt::Break)),
        "user's Continue must be rewritten to MirStmt::Break targeting the inner wrapper, got {:?}",
        inner_while
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
    let inner_while_idx = outer_while
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected inner MirStmt::While");
    let inner_while = match &outer_while.stmts[inner_while_idx] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    let is_break = inner_while
        .windows(2)
        .find_map(|w| match &w[0] {
            MirStmt::Assign {
                target: MirPlace::Local { id },
                value: MirExpr::Bool(true),
            } if matches!(w[1], MirStmt::Break) => Some(*id),
            _ => None,
        })
        .expect(
            "user's Break must be rewritten to is_break=true; Break targeting the inner wrapper",
        );
    assert!(
        outer_while.stmts.iter().any(|s| matches!(
            s,
            MirStmt::If {
                cond: MirExpr::Local(id),
                then_block,
                ..
            } if *id == is_break && matches!(then_block.stmts.as_slice(), [MirStmt::Break])
        )),
        "the is_break sentinel must break the outer loop, got {:?}",
        outer_while.stmts
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
    let body_stmts = match &mir_block.stmts[1] {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 1, got {other:?}"),
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
    let while_stmt = &mir_block.stmts[1];
    let while_body = match while_stmt {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 1, got {other:?}"),
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
    let MirStmt::If { cond: guard, .. } = while_body.last().expect("non-empty") else {
        panic!(
            "the do-while body must end with the condition check, got {:?}",
            while_body
        );
    };
    assert!(
        matches!(
            guard,
            MirExpr::Unary { op: UnaryOp::Not, expr, .. } if matches!(**expr, MirExpr::Bool(false))
        ),
        "the loop must re-check the cond after the body (and after a Continue), got {guard:?}"
    );
}

#[test]
fn convert_block_dowhile_cond_statements_are_inside_the_body_before_the_check() {
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
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_body = match &mir_block.stmts[1] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected MirStmt::While at index 1, got {other:?}"),
    };
    let inner_while_idx = outer_body
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected the body wrapper");
    let cond_let_idx = outer_body
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::Let { .. }))
        .expect("the cond statements must be lowered inside the loop body");
    let check_idx = outer_body
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::If {
                    cond: MirExpr::Unary {
                        op: UnaryOp::Not,
                        ..
                    },
                    ..
                }
            )
        })
        .expect("expected the condition check");
    assert!(
        inner_while_idx < cond_let_idx && cond_let_idx < check_idx,
        "a do-while must run its body first, then evaluate the cond statements, then check them; \
         got {:?}",
        outer_body.stmts
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
    let MirStmt::While { cond, body } = mir_block.stmts.last().expect("non-empty") else {
        panic!("expected the outer MirStmt::While to be the last statement");
    };
    assert!(
        matches!(*cond, MirExpr::Bool(true)),
        "the header must be `while true`; the call is checked inside the body so the first check \
         happens after the call is evaluated, got {cond:?}"
    );
    let total = body
        .stmts
        .iter()
        .map(|s| count_calls_in_stmt(s, FunctionId::from_raw(0).raw()))
        .sum::<usize>();
    assert_eq!(
        total, 1,
        "the cond call must be evaluated exactly once per iteration, got {:?}",
        body.stmts
    );
    let MirStmt::If {
        cond: guard,
        then_block,
        ..
    } = &body.stmts[0]
    else {
        panic!(
            "the loop body must start with the condition check, got {:?}",
            body.stmts
        );
    };
    assert!(
        matches!(
            guard,
            MirExpr::Unary { op: UnaryOp::Not, expr, .. }
                if matches!(**expr, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0))
        ),
        "the check must negate the call itself, got {guard:?}"
    );
    assert!(matches!(then_block.stmts.as_slice(), [MirStmt::Break]));
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
    let (outer_cond, outer_body) = match &mir_block.stmts[1] {
        MirStmt::While { cond, body } => (cond, body),
        other => panic!("expected MirStmt::While at index 1, got {other:?}"),
    };
    assert!(matches!(*outer_cond, MirExpr::Bool(true)));
    let inner_while_idx = outer_body
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected inner MirStmt::While");
    let guard_idx = outer_body
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::If {
                    cond: MirExpr::Unary {
                        op: UnaryOp::Not,
                        ..
                    },
                    ..
                }
            )
        })
        .expect("expected the condition check");
    assert!(
        guard_idx > inner_while_idx,
        "`do {{}} while (false)` must run the body before the check (and then break), got {:?}",
        outer_body.stmts
    );
    let MirStmt::If { cond: guard, .. } = &outer_body.stmts[guard_idx] else {
        unreachable!()
    };
    assert!(
        matches!(guard, MirExpr::Unary { expr, .. } if matches!(**expr, MirExpr::Bool(false))),
        "the check must negate the false cond so the loop runs the body exactly once, got {guard:?}"
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
fn convert_block_forof_generator_inner_extracts_item_ty() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let i64_ty = types.intern(&Type::I64);
    let inner_gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let outer_gen_ty = types.intern(&Type::Generator {
        inner: inner_gen_ty,
        is_async: false,
    });
    let iter = HirExpr::Local {
        id: LocalId::from_raw(7),
        ty: outer_gen_ty,
        span: Span::default(),
    };
    let next_call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: inner_gen_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("next"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: Vec::new(),
        span: Span::default(),
    };
    let block = HirBlock(vec![HirStmt::ForOf {
        binding: LocalId::from_raw(0),
        iter,
        body: Box::new(HirStmt::Expr { expr: next_call }),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut types, &mut cx);
    let decl = locals
        .iter()
        .find(|l| l.name.as_str().starts_with("__for_of_"))
        .expect("for-of binding local must be present");
    assert_eq!(
        decl.ty,
        inner_gen_ty,
        "item_ty must be extracted from `Type::Generator {{ inner }}` (got TypeId({}))",
        decl.ty.raw()
    );
    assert!(
        !decl.mutable,
        "for-of binding local must start as `mutable: false` so the backend's `collect_written_locals` \
         can promote it to `mut` only when the body actually writes to the binding (preserves support \
         for `for (const x of arr)` where x is never reassigned, while still emitting `mut` when the \
         body does assign back)"
    );
    let for_of_iter_ty = mir_block
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::ForOf { iter_ty, .. } => Some(*iter_ty),
            _ => None,
        })
        .expect("converted block must contain a MirStmt::ForOf");
    assert_eq!(
        for_of_iter_ty,
        outer_gen_ty,
        "MirStmt::ForOf.iter_ty must carry the original `Type::Generator {{ inner: Generator<...> }}` of the iterable (got TypeId({}))",
        for_of_iter_ty.raw()
    );
    assert!(
        !cx.has_errors(),
        "for-of over Generator<Generator<i64>> must not error, got: {:?}",
        cx.diagnostics()
    );
}

fn assert_forof_e0406<F>(
    iter_ty_fn: F,
    iter_span: Span,
    message_fragments: &[&str],
    label: &str,
) -> (Vec<MirLocalDecl>, Span, TypeId)
where
    F: FnOnce(&mut TypeTable) -> TypeId,
{
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let iter_ty = iter_ty_fn(&mut types);
    let iter = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: iter_ty,
        span: iter_span,
    };
    let block = HirBlock(vec![HirStmt::ForOf {
        binding: LocalId::from_raw(0),
        iter,
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (_mir_block, locals) = c.convert_block(&block, &mut types, &mut cx);
    let e0406: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .collect();
    assert_eq!(
        e0406.len(),
        1,
        "for-of over {label} must emit exactly one E0406 (no cascade of unrelated type errors), \
         got diagnostics: {:?}",
        cx.diagnostics()
    );
    for fragment in message_fragments {
        assert!(
            e0406[0].message.contains(fragment),
            "E0406 for for-of over {label} must contain {fragment:?} in the message so the user \
             sees a clear AOT-target message instead of an opaque type-mismatch cascade, got: {:?}",
            e0406[0].message
        );
    }
    let error_ty = types.intern(&Type::Error);
    (locals, e0406[0].span, error_ty)
}

#[test]
fn convert_block_forof_unsupported_string_iter_emits_e0406() {
    let iter_span = Span::new(10, 20);
    let (locals, e0406_span, error_ty) = assert_forof_e0406(
        |t| t.intern(&Type::String),
        iter_span,
        &["String", "not yet supported"],
        "String",
    );
    assert_eq!(
        e0406_span, iter_span,
        "E0406 for for-of over String must carry the iter expression's source span so the user \
         can locate the offending `for (... of ...)` site, got span {:?}",
        e0406_span
    );
    let decl = locals
        .iter()
        .find(|l| l.name.as_str().starts_with("__for_of_"))
        .expect(
            "for-of binding local must still be emitted so the rest of the pipeline does not crash",
        );
    assert_eq!(
        decl.ty,
        error_ty,
        "for-of binding local must fall back to the interned `Type::Error` TypeId after the E0406 \
         error so downstream passes see a coherent error type rather than a misleading concrete \
         iterable type (got {})",
        decl.ty.raw()
    );
}

#[test]
fn convert_block_forof_unsupported_typed_array_iter_emits_e0406() {
    let _ = assert_forof_e0406(
        |t| t.intern(&Type::Uint8Array),
        Span::default(),
        &["TypedArray"],
        "Uint8Array",
    );
}

#[test]
fn convert_block_forof_unsupported_generic_type_emits_e0406() {
    let _ = assert_forof_e0406(
        |t| {
            t.intern(&Type::Struct {
                id: StructId::from_raw(42),
            })
        },
        Span::default(),
        &["Array<T>", "Generator<T>", "Struct"],
        "Type::Struct",
    );
}

#[test]
fn convert_block_forof_unresolved_iter_type_does_not_cascade() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let iter = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: TypeId::from_raw(99),
        span: Span::new(10, 20),
    };
    let block = HirBlock(vec![HirStmt::ForOf {
        binding: LocalId::from_raw(0),
        iter,
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (_mir_block, locals) = c.convert_block(&block, &mut types, &mut cx);
    assert!(
        !cx.has_errors(),
        "an unresolved for-of iterable type must not produce a hard error (the iterable type is \
         already unresolved upstream); got {:?}",
        cx.diagnostics()
    );
    let decl = locals
        .iter()
        .find(|l| l.name.as_str().starts_with("__for_of_"))
        .expect(
            "for-of binding local must still be emitted so the rest of the pipeline does not crash",
        );
    assert!(
        matches!(types.resolve(decl.ty), Some(Type::Error)),
        "for-of binding local must fall back to `Type::Error` for an unresolved iterable type, \
         got {}",
        decl.ty.raw()
    );
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
fn convert_block_forin_binding_local_has_string_type() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let expected_string_ty = types.intern(&Type::String);
    let block = HirBlock(vec![HirStmt::ForIn {
        binding: LocalId::from_raw(0),
        iter: int_lit(0),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (_mir_block, locals) = c.convert_block(&block, &mut types, &mut cx);
    let decl = locals
        .iter()
        .find(|l| l.name.as_str().starts_with("__for_in_"))
        .expect("for-in binding local must be emitted");
    assert_eq!(
        decl.ty,
        expected_string_ty,
        "for-in binding local must be typed as Type::String (for-in yields string keys for \
         object iterables, got TypeId({}))",
        decl.ty.raw()
    );
    assert!(
        !decl.mutable,
        "for-in binding local must be immutable, got mutable={}",
        decl.mutable
    );
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
        !cx.diagnostics().is_empty(),
        "non-const case value (Binary expression) must emit a diagnostic"
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

#[test]
fn convert_let_consumed_by_next_inside_call_arg_marks_mutable() {
    let g = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("push"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: vec![HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: g,
                        ty: unit_ty(),
                        span: Span::default(),
                    }),
                    field: FieldId::from_raw(1),
                    field_name: Atom::new_inline("next"),
                    ty: unit_ty(),
                    span: Span::default(),
                })),
                args: Vec::new(),
                type_args: Vec::new(),
                ty: unit_ty(),
                span: Span::default(),
            }],
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        is_local_reassigned(g, &body, &empty_types()),
        "let g = gen(); arr.push(g.next()) (next nested in call arg) must mark g as mutable"
    );
}

#[test]
fn is_local_reassigned_plain_method_call_does_not_mark_owner_mutable() {
    let o = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: o,
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("someMethod"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: Vec::new(),
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        !is_local_reassigned(o, &body, &empty_types()),
        "let o = ...; o.someMethod() (non-`next` method call) must NOT mark o as mutable after \
         the wrapper delegates to collect_mutable_locals; the wrapper only treats `.next()` calls \
         and explicit assignment targets as evidence of mutability"
    );
}

#[test]
fn is_local_reassigned_method_call_with_owner_in_args_does_not_mark_mutable() {
    let o = LocalId::from_raw(0);
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(99)),
            args: vec![HirExpr::Local {
                id: o,
                ty: unit_ty(),
                span: Span::default(),
            }],
            type_args: Vec::new(),
            ty: unit_ty(),
            span: Span::default(),
        },
    }];
    assert!(
        !is_local_reassigned(o, &body, &empty_types()),
        "let o = ...; f(o) (a local passed as a function argument) must NOT mark o as mutable; \
         the wrapper only treats `.next()` and assignment targets as evidence of mutability"
    );
}

#[test]
fn convert_block_dowhile_continue_cond_binary_uses_bool_ty() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let _i64_ty = types.intern(&Type::I64);
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Bool(false, Span::default()),
    }]);
    let (mir_block, _locals) = c.convert_block(&block, &mut types, &mut cx);
    assert!(
        !cx.has_errors(),
        "do-while must lower cleanly, got: {:?}",
        cx.diagnostics()
    );

    let is_break_ty = match &mir_block.stmts[0] {
        MirStmt::Let { ty, .. } => *ty,
        other => panic!("expected first stmt to be MirStmt::Let for is_break, got {other:?}"),
    };
    let while_body = match &mir_block.stmts[1] {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected second stmt to be the outer MirStmt::While, got {other:?}"),
    };
    let guard_ty = while_body
        .iter()
        .find_map(|s| match s {
            MirStmt::If {
                cond:
                    MirExpr::Unary {
                        op: UnaryOp::Not,
                        ty,
                        ..
                    },
                ..
            } => Some(*ty),
            _ => None,
        })
        .expect("the do-while body must check the condition");
    assert_ne!(
        guard_ty,
        TypeId::from_raw(0),
        "the check's negation ty must be the interned bool_ty, not the default TypeId::from_raw(0); \
         got {guard_ty:?}"
    );
    assert_eq!(
        guard_ty, is_break_ty,
        "the check's negation ty must equal the is_break local ty, got {guard_ty:?}"
    );
    let expected_bool_ty = types.intern(&Type::Bool);
    assert_eq!(
        guard_ty, expected_bool_ty,
        "the check's negation ty must resolve to the bool type, got {guard_ty:?}"
    );
}

#[test]
fn convert_block_starts_struct_id_counter_at_one() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let ty = types.intern(&Type::I64);
    let block = HirBlock(vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::new_inline("p"),
        ty,
        init: Some(HirExpr::StructLiteral {
            ty,
            fields: Vec::new(),

            span: Span::default(),
        }),
        narrowed: false,
    }]);
    let (mir_block, _locals) = c.convert_block(&block, &mut types, &mut cx);
    assert!(
        !cx.has_errors(),
        "unexpected diagnostics: {:?}",
        cx.diagnostics()
    );
    let MirStmt::Let {
        init: Some(init), ..
    } = &mir_block.stmts[0]
    else {
        panic!(
            "expected MirStmt::Let with init, got {:?}",
            mir_block.stmts[0]
        );
    };
    let MirExpr::StructLiteral { struct_id, .. } = init else {
        panic!("expected init to be MirExpr::StructLiteral, got {init:?}");
    };
    assert_eq!(
        *struct_id,
        StructId::from_raw(1),
        "first user struct literal in convert_block must be assigned StructId(1), not StructId(0) — \
         StructId(0) is reserved for the placeholder Type::Void / Type::Error path; got {struct_id:?}"
    );
}
