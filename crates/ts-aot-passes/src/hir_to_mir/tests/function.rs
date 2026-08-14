use super::common::*;

#[test]
fn convert_function_nested_let_in_if_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(HirStmt::Let {
                id: LocalId::from_raw(7),
                name: Atom::new_inline("99"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
            }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert_eq!(
        mir.body.locals.len(),
        1,
        "nested let must surface in body.locals"
    );
    assert_eq!(mir.body.locals[0].name, Atom::new_inline("99"));
}

#[test]
fn convert_function_nested_let_in_while_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::While {
            cond: HirExpr::Bool(true, Span::default()),
            body: Box::new(HirStmt::Let {
                id: LocalId::from_raw(11),
                name: Atom::new_inline("33"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert!(
        names.contains(&"33".to_owned()),
        "while-body let must surface in body.locals (got {names:?})"
    );
}

#[test]
fn convert_function_nested_let_in_forof_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::ForOf {
            binding: LocalId::from_raw(20),
            iter: int_lit(0),
            body: Box::new(HirStmt::Let {
                id: LocalId::from_raw(21),
                name: Atom::new_inline("77"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert_eq!(mir.body.locals.len(), 2, "for-of binding + nested let");
    assert!(
        names.iter().any(|n| n.starts_with("__for_of_")),
        "for-of binding must use a unique __for_of_<counter> synth name (allocated by \
         unique_synth_local_name from the converter's fresh_local counter, so the suffix tracks \
         the next_local counter rather than a fixed local id) to avoid collisions with user code, \
         got names: {names:?}"
    );
    assert!(names.contains(&"77".to_owned()), "nested let name");
}

#[test]
fn convert_function_forof_synth_name_avoids_user_local_collision() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![
            HirStmt::Let {
                id: LocalId::from_raw(0),
                name: Atom::new_inline("__for_of_1"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            },
            HirStmt::ForOf {
                binding: LocalId::from_raw(20),
                iter: int_lit(0),
                body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert!(
        names.contains(&"__for_of_1".to_owned()),
        "user local named __for_of_1 must keep its name, got names: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("__for_of_") && n != "__for_of_1"),
        "for-of binding must get a collision-free name starting with __for_of_ but distinct from the user local, got names: {names:?}"
    );
    let unique: std::collections::HashSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "local names must stay unique");
}

#[test]
fn convert_function_forin_synth_name_avoids_user_local_collision() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![
            HirStmt::Let {
                id: LocalId::from_raw(0),
                name: Atom::new_inline("__for_in_1"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            },
            HirStmt::ForIn {
                binding: LocalId::from_raw(20),
                iter: int_lit(0),
                body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert!(
        names.contains(&"__for_in_1".to_owned()),
        "user local named __for_in_1 must keep its name, got names: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("__for_in_") && n != "__for_in_1"),
        "for-in binding must get a collision-free name starting with __for_in_ but distinct from the user local, got names: {names:?}"
    );
    let unique: std::collections::HashSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "local names must stay unique");
}

#[test]
fn convert_function_basic_shape() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![HirParam {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: true,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(
        &f,
        FunctionId::from_raw(0),
        Some("f".to_owned()),
        HashMap::new(),
        &mut cx,
    );
    assert_eq!(mir.id, FunctionId::from_raw(0));
    assert_eq!(mir.params.len(), 1);
    assert!(!mir.effects.is_async);
}

#[test]
fn convert_function_let_after_params_gets_fresh_id() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![
            HirParam {
                name: Atom::new_inline("10"),
                ty: unit_ty(),
            },
            HirParam {
                name: Atom::new_inline("11"),
                ty: unit_ty(),
            },
        ],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Let {
            id: LocalId::from_raw(5),
            name: Atom::new_inline("99"),
            ty: unit_ty(),
            init: Some(int_lit(0)),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert_eq!(mir.params.len(), 2);
    assert_eq!(mir.body.locals.len(), 1);
    let let_id = mir.body.locals[0].id;
    assert_ne!(let_id, mir.params[0].id);
    assert_ne!(let_id, mir.params[1].id);
    assert!(
        let_id.raw() >= mir.params.len() as u32,
        "let id {} should be >= params len {}",
        let_id.raw(),
        mir.params.len()
    );
}

#[test]
fn convert_function_marks_async_effect() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: Vec::new(),
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert!(mir.effects.is_async);
}

#[test]
fn convert_function_body_references_param_id_resolves_to_param() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![HirParam {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),

                span: Span::default(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let param_id = mir.params[0].id;
    let referenced = match &mir.body.block.stmts[0] {
        MirStmt::Expr(MirExpr::Local(lid)) => *lid,
        other => panic!("expected Expr(Local), got {other:?}"),
    };
    assert_eq!(
        referenced, param_id,
        "HIR LocalId(0) in body must resolve to the MIR param id, not a fresh local"
    );
    assert!(
        mir.body.locals.is_empty(),
        "no extra locals should be allocated for the param reference itself"
    );
}

#[test]
fn convert_function_await_emits_mir_await_expr_without_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Await {
                expr: Box::new(int_lit(1)),
                ty: unit_ty(),

                span: Span::default(),
            }),
        }],
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    match mir.body.block.stmts.last().expect("non-empty body") {
        MirStmt::Return(Some(MirExpr::Await { .. })) => {}
        other => panic!("expected last stmt Return(Some(MirExpr::Await)), got {other:?}"),
    };
    assert!(
        mir.body.locals.is_empty(),
        "await no longer needs a temp local (no state machine), got: {:?}",
        mir.body.locals
    );
}

#[test]
fn convert_function_new_alloc_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
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
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let alloc = match mir.body.block.stmts.last().expect("non-empty body") {
        MirStmt::Return(Some(MirExpr::Local(lid))) => *lid,
        other => panic!("expected last stmt Return(Some(Local)), got {other:?}"),
    };
    assert!(
        mir.body.locals.iter().any(|l| l.id == alloc),
        "new alloc {alloc:?} must be in body.locals"
    );
}

#[test]
fn convert_function_temp_locals_drained_only_once() {
    let new_expr = HirExpr::New {
        callee: Box::new(HirExpr::Global {
            name: Atom::new_inline("99"),
            ty: unit_ty(),
            span: Span::default(),
        }),
        args: Vec::new(),
        ty: unit_ty(),
        span: Span::default(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Await {
                expr: Box::new(HirExpr::Binary {
                    op: ts_aot_ir_hir::HirBinaryOp::Add,
                    lhs: Box::new(new_expr.clone()),
                    rhs: Box::new(new_expr),
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                ty: unit_ty(),

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert!(
        !mir.body.locals.is_empty(),
        "body must allocate temp locals (the two HirExpr::New each get a fresh local) so the \
         drain-once behavior is actually exercised; got an empty body.locals: {:?}",
        mir.body.locals
    );
    let local_ids: Vec<u32> = mir.body.locals.iter().map(|l| l.id.raw()).collect();
    let unique: std::collections::HashSet<u32> = local_ids.iter().copied().collect();
    assert_eq!(
        local_ids.len(),
        unique.len(),
        "no duplicate locals (drilled into body.locals)"
    );
}

#[test]
fn convert_function_can_throw_true_when_body_has_throw_stmt() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw { expr: int_lit(0) }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert!(
        mir.effects.can_throw,
        "function containing HirStmt::Throw must surface can_throw=true"
    );
}

#[test]
fn convert_function_can_throw_false_when_body_has_no_throw_stmt() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert!(
        !mir.effects.can_throw,
        "function without throw must surface can_throw=false"
    );
}

#[test]
fn convert_function_can_throw_recurses_into_nested_blocks() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: HirExpr::Bool(true, Span::default()),
            then: Box::new(HirStmt::Throw { expr: int_lit(0) }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    assert!(
        mir.effects.can_throw,
        "nested throw inside If must propagate to can_throw"
    );
}

#[test]
fn convert_function_build_params_preserves_param_atom_name() {
    use ts_aot_ir_hir::HirParam;
    let sentinel_symbol = Atom::new_inline("__sentinel__");
    let first_id = Atom::new_inline("first");
    let second_id = Atom::new_inline("second");
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![
            HirParam {
                name: first_id.clone(),
                ty: unit_ty(),
            },
            HirParam {
                name: second_id.clone(),
                ty: unit_ty(),
            },
        ],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(0), None, HashMap::new(), &mut cx);
    let first_name = mir.params[0].name.clone();
    let second_name = mir.params[1].name.clone();
    assert_ne!(
        first_name, second_name,
        "distinct param names must yield distinct Atoms"
    );
    assert_ne!(
        first_name, sentinel_symbol,
        "MirParam.name must be the source Atom (not coincidentally equal to a pre-existing entry); got {:?}",
        first_name
    );
    assert_eq!(
        first_name.as_str(),
        first_id.as_str(),
        "MirParam.name must equal the source Atom (content-equivalent Atom); got {:?} vs {}",
        first_name,
        first_id
    );
}

#[test]
fn convert_function_with_remap_uses_remap_only_for_call_sites() {
    let f = HirFunction {
        name: Atom::new_inline("7"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: Vec::new(),
                ty: unit_ty(),
                type_args: vec![],

                span: Span::default(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut remap = HashMap::new();
    remap.insert(FunctionId::from_raw(0), FunctionId::from_raw(42));
    let mut cx = ctx();
    let mir = run_convert(&f, FunctionId::from_raw(5), None, remap, &mut cx);
    assert_eq!(
        mir.id,
        FunctionId::from_raw(5),
        "declaration id is the caller-provided value, not remapped"
    );
    let call_callee = match &mir.body.block.stmts[0] {
        MirStmt::Expr(MirExpr::Call { callee, .. }) => *callee,
        other => panic!("expected Call, got {other:?}"),
    };
    assert_eq!(
        call_callee,
        FunctionId::from_raw(42),
        "call site remapped via function_remap"
    );
}
