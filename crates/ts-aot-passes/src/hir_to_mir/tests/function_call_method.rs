use super::common::*;

#[test]
fn function_call_method_drops_this_arg_and_emits_direct_call() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("add"),
        FunctionId::from_raw(7),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &function_method_call_with_args(
            "add",
            "call",
            vec![
                HirExpr::Unit(Span::default()),
                HirExpr::Int(1, Span::default()),
                HirExpr::Int(2, Span::default()),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "add.call(undefined, 1, 2) must compile without errors, got {:?}",
        cx.diagnostics()
    );
    let MirExpr::Call { callee, args, .. } = &mir else {
        panic!("add.call must lower to MirExpr::Call, got {mir:?}");
    };
    assert_eq!(
        *callee,
        FunctionId::from_raw(7),
        "add.call must resolve to the add FunctionId, got {callee:?}"
    );
    assert_eq!(
        args.len(),
        2,
        "add.call(undefined, 1, 2) must drop thisArg and keep 2 args, got {args:?}"
    );
    let int_count = args
        .iter()
        .filter(|a| matches!(a, MirExpr::Int { value: 1 | 2, .. }))
        .count();
    assert_eq!(
        int_count, 2,
        "remaining args must be the literal 1 and 2, got {args:?}"
    );
    assert!(
        out.is_empty(),
        "convert_expr on add.call(undefined, 1, 2) with pure-literal args must NOT push any \
         MirStmt to `out` — the caller wraps the returned MirExpr in MirStmt::Expr itself; \
         pushing here would cause DOUBLE function execution when the call is used in a \
         value context (e.g. `let x = f.call(null, 1, 2) + 1`). got: {out:?}"
    );
}

#[test]
fn function_call_method_with_only_this_arg_emits_zero_arg_call() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("noop"),
        FunctionId::from_raw(3),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &function_method_call_with_args("noop", "call", vec![HirExpr::Unit(Span::default())]),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let MirExpr::Call { callee, args, .. } = &mir else {
        panic!("noop.call(undefined) must lower to MirExpr::Call, got {mir:?}");
    };
    assert_eq!(*callee, FunctionId::from_raw(3));
    assert!(
        args.is_empty(),
        "noop.call(undefined) must drop thisArg and keep 0 args, got {args:?}"
    );
}

#[test]
fn function_call_method_with_no_args_emits_e0406_with_callee_span() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let mut cx = ctx();
    let callee_span = Span::new(200, 215);
    let call_expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("f"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("call"),
            ty: unit_ty(),
            span: callee_span,
        })),
        args: vec![],
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    let out = &mut Vec::new();
    let mir = c.convert_expr(
        &call_expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406 = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406,
        1,
        "f.call() with no args must emit exactly one E0406, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0406")
        .expect("E0406");
    assert_eq!(
        diag.span, callee_span,
        "E0406 from f.call() must carry the call expression's span \
         ({callee_span:?}) so the user can navigate to it; the previous \
         Span::new(0, 0) gave a useless empty span. got: {:?}",
        diag.span
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "f.call() with no args must return MirExpr::Unit, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "no MirStmt must be pushed on error, got {out:?}"
    );
}

#[test]
fn function_apply_method_with_literal_array_spreads_args_into_direct_call() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("add"),
        FunctionId::from_raw(7),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &function_method_call_with_args(
            "add",
            "apply",
            vec![
                HirExpr::Unit(Span::default()),
                int_array_literal(vec![10, 20]),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "add.apply(undefined, [10, 20]) must compile without errors, got {:?}",
        cx.diagnostics()
    );
    let MirExpr::Call { callee, args, .. } = &mir else {
        panic!("add.apply must lower to MirExpr::Call, got {mir:?}");
    };
    assert_eq!(*callee, FunctionId::from_raw(7));
    assert_eq!(
        args.len(),
        2,
        "add.apply must spread the 2-element array into 2 args, got {args:?}"
    );
    let int_values: Vec<i128> = args
        .iter()
        .filter_map(|a| match a {
            MirExpr::Int { value, .. } => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(
        int_values,
        vec![10_i128, 20],
        "spread args must preserve original order and values, got {args:?}"
    );
}

#[test]
fn function_apply_method_with_non_literal_array_emits_e0406_with_callee_span() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let mut cx = ctx();
    let callee_span = Span::new(100, 130);
    let call_expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("f"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("apply"),
            ty: unit_ty(),
            span: callee_span,
        })),
        args: vec![
            HirExpr::Unit(Span::default()),
            HirExpr::Local {
                id: LocalId::from_raw(42),
                ty: unit_ty(),
                span: Span::default(),
            },
        ],
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    let out = &mut Vec::new();
    let mir = c.convert_expr(
        &call_expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406 = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406,
        1,
        "f.apply with a non-literal array must emit E0406, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0406")
        .expect("E0406");
    assert!(
        diag.message.contains("literal array"),
        "E0406 message must explain the literal-array requirement, got: {}",
        diag.message
    );
    assert_eq!(
        diag.span, callee_span,
        "E0406 from f.apply must carry the call expression's span \
         ({callee_span:?}) so the user can navigate to it. got: {:?}",
        diag.span
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "f.apply with non-literal must return MirExpr::Unit, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "no MirStmt must be pushed on error, got {out:?}"
    );
}

#[test]
fn function_bind_method_emits_e0406_not_yet_supported_with_callee_span() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let mut cx = ctx();
    let callee_span = Span::new(42, 58);
    let call_expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("f"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("bind"),
            ty: unit_ty(),
            span: callee_span,
        })),
        args: vec![
            HirExpr::Unit(Span::default()),
            HirExpr::Int(1, Span::default()),
            HirExpr::Int(2, Span::default()),
        ],
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    let out = &mut Vec::new();
    let mir = c.convert_expr(
        &call_expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406 = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406,
        1,
        "f.bind must emit exactly one E0406, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0406")
        .expect("E0406");
    assert!(
        diag.message.contains("bind") && diag.message.contains("not yet supported"),
        "E0406 message must mention bind not yet supported, got: {}",
        diag.message
    );
    assert_eq!(
        diag.span, callee_span,
        "E0406 from f.bind must carry the call expression's span \
         ({callee_span:?}) so the user can navigate to it; the previous \
         Span::new(0, 0) gave a useless empty span. got: {:?}",
        diag.span
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "f.bind must return MirExpr::Unit, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "no MirStmt must be pushed on error, got {out:?}"
    );
}

#[test]
fn function_method_call_on_unknown_global_does_not_match_function_dispatch() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let _mir = c.convert_expr(
        &function_method_call_with_args(
            "undefinedFn",
            "call",
            vec![
                HirExpr::Unit(Span::default()),
                HirExpr::Int(1, Span::default()),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !out.iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Call { .. }))),
        "unknownFn.call must NOT match the function-method dispatch (no name_to_function entry); \
         the call must fall through to the general IndirectCall path. Got: {out:?}"
    );
}

#[test]
fn function_call_method_in_let_init_emits_call_exactly_once() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("add"),
        FunctionId::from_raw(7),
    )]));
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::new_inline("x"),
        ty: unit_ty(),
        init: Some(HirExpr::Binary {
            op: ts_aot_ir_hir::HirBinaryOp::Add,
            lhs: Box::new(function_method_call_with_args(
                "add",
                "call",
                vec![
                    HirExpr::Unit(Span::default()),
                    HirExpr::Int(1, Span::default()),
                    HirExpr::Int(2, Span::default()),
                ],
            )),
            rhs: Box::new(HirExpr::Int(1, Span::default())),
            ty: unit_ty(),
            span: Span::default(),
        }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);

    fn count_calls(e: &MirExpr, target: FunctionId) -> usize {
        let mut n = 0;
        match e {
            MirExpr::Call { callee, args, .. } => {
                if *callee == target {
                    n += 1;
                }
                for a in args {
                    n += count_calls(a, target);
                }
            }
            MirExpr::Binary { left, right, .. } => {
                n += count_calls(left, target);
                n += count_calls(right, target);
            }
            MirExpr::Unary { expr, .. } => n += count_calls(expr, target),
            _ => {}
        }
        n
    }

    let target = FunctionId::from_raw(7);
    let mut total_calls = 0;
    for s in &mir_block.stmts {
        match s {
            MirStmt::Expr(e) | MirStmt::Return(Some(e)) => {
                total_calls += count_calls(e, target);
            }
            MirStmt::Let { init: Some(e), .. } => {
                total_calls += count_calls(e, target);
            }
            _ => {}
        }
    }
    assert_eq!(
        total_calls, 1,
        "let x = add.call(null, 1, 2) + 1 must lower to exactly ONE Call node for `add` \
         across the entire block MIR; pushing MirStmt::Expr(call) inside the dispatch \
         AND returning MirExpr::Call would surface the call twice. Got: {mir_block:?}"
    );
}

#[test]
fn function_call_method_with_effectful_arg_emits_runtime_stmt_exactly_once() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &function_method_call_with_args(
            "f",
            "call",
            vec![
                HirExpr::Null(Span::default()),
                HirExpr::Call {
                    callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                        owner: Box::new(HirExpr::Global {
                            name: Atom::new_inline("Math"),
                            ty: unit_ty(),
                            span: Span::default(),
                        }),
                        field: FieldId::from_raw(0),
                        field_name: Atom::new_inline("abs"),
                        ty: unit_ty(),
                        span: Span::default(),
                    })),
                    args: vec![HirExpr::Local {
                        id: LocalId::from_raw(99),
                        ty: unit_ty(),
                        span: Span::default(),
                    }],
                    ty: unit_ty(),
                    type_args: vec![],
                    span: Span::default(),
                },
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "f.call(null, Math.abs(x)) must compile without diagnostics, got: {:?}",
        cx.diagnostics()
    );
    let math_abs_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::MathAbs,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        math_abs_count, 1,
        "f.call(null, Math.abs(x)) must lower the effectful argument to exactly ONE MathAbs \
         runtime statement; if `mir_args` is materialized BEFORE the function-method dispatch \
         fires (the bug being prevented), the Math.abs call gets re-converted and pushed twice. \
         got: {out:?}"
    );
    let MirExpr::Call { callee, args, .. } = &mir else {
        panic!("f.call(null, Math.abs(x)) must lower to MirExpr::Call, got {mir:?}");
    };
    assert_eq!(
        *callee,
        FunctionId::from_raw(11),
        "f.call must resolve to the f FunctionId, got {callee:?}"
    );
    assert_eq!(
        args.len(),
        1,
        "f.call(null, Math.abs(x)) must drop thisArg and forward only the Math.abs result, \
         got {args:?}"
    );
    let math_abs_dest = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime {
                op: RuntimeOp::MathAbs,
                dest: Some(d),
                ..
            } => Some(*d),
            _ => None,
        })
        .expect("the MathAbs runtime statement must have a dest local");
    assert!(
        matches!(args[0], MirExpr::Local(d) if d == math_abs_dest),
        "f.call's sole forwarded arg must be the Local dest of the MathAbs runtime statement, \
         got: {:?}, expected Local({math_abs_dest:?})",
        args[0]
    );
}

#[test]
fn function_apply_method_with_non_nullish_thisarg_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Local {
            id: LocalId::from_raw(7),
            ty: unit_ty(),
            span: Span::default(),
        },
        "apply",
        vec![int_array_literal(vec![1, 2])],
    );
}

#[test]
fn function_call_method_unit_thisarg_does_not_push_extra_statement() {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let _mir = c.convert_expr(
        &function_method_call_with_args(
            "f",
            "call",
            vec![
                HirExpr::Unit(Span::default()),
                HirExpr::Int(7, Span::default()),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        out.is_empty(),
        "f.call(undefined, 7) with a literal thisArg must NOT push any side-effect \
         statement (HirExpr::Unit is one of the only nullish forms accepted as thisArg in AOT; \
         the dispatch recognizes it and never calls convert_expr on it). Out should stay empty; \
         the dispatch returns the call expression and the caller wraps it in MirStmt::Expr. \
         got: {out:?}"
    );
}

#[test]
fn function_call_thisarg_null_literal_dispatches() {
    function_call_with_nullish_thisarg_dispatches_call(
        HirExpr::Null(Span::default()),
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_undefined_literal_dispatches() {
    function_call_with_nullish_thisarg_dispatches_call(
        HirExpr::Undefined(Span::default()),
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_local_read_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Local {
            id: LocalId::from_raw(7),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_global_read_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Global {
            name: Atom::new_inline("globalThis"),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_field_read_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(1),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(2),
            field_name: Atom::new_inline("x"),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_index_read_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Index {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(1),
                ty: unit_ty(),
                span: Span::default(),
            }),
            index: Box::new(HirExpr::Int(3, Span::default())),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_typeof_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Unary {
            op: ts_aot_ir_hir::HirUnaryOp::TypeOf,
            expr: Box::new(HirExpr::Local {
                id: LocalId::from_raw(1),
                ty: unit_ty(),
                span: Span::default(),
            }),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_binary_math_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Binary {
            op: ts_aot_ir_hir::HirBinaryOp::Add,
            lhs: Box::new(HirExpr::Int(1, Span::default())),
            rhs: Box::new(HirExpr::Int(2, Span::default())),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_unary_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Unary {
            op: ts_aot_ir_hir::HirUnaryOp::Neg,
            expr: Box::new(HirExpr::Int(5, Span::default())),
            ty: unit_ty(),
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_with_call_side_effect_emits_e0406() {
    function_call_with_non_nullish_thisarg_emits_e0406(
        HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                name: Atom::new_inline("getThis"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: vec![],
            ty: unit_ty(),
            type_args: vec![],
            span: Span::default(),
        },
        "call",
        vec![HirExpr::Int(1, Span::default())],
    );
}

#[test]
fn function_call_thisarg_non_nullish_primitives_emit_e0406() {
    let cases: Vec<(&str, HirExpr)> = vec![
        (
            "BigInt literal",
            HirExpr::BigInt {
                value: Atom::new_inline("42"),
                ty: unit_ty(),
                span: Span::default(),
            },
        ),
        (
            "RegExp literal",
            HirExpr::RegExp {
                pattern: Atom::new_inline("x"),
                flags: Atom::new_inline("g"),
                ty: unit_ty(),
                span: Span::default(),
            },
        ),
        (
            "tagged template (would lower to MirExpr::TemplateStringsArray)",
            HirExpr::Template {
                tag: Some(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("tag"),
                        ty: unit_ty(),
                        span: Span::default(),
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("fn"),
                    ty: unit_ty(),
                    span: Span::default(),
                })),
                expressions: vec![HirExpr::Int(1, Span::default())],
                cooked_parts: vec![Some(Atom::new_inline("a")), Some(Atom::new_inline("b"))],
                raw_parts: vec![Some(Atom::new_inline("a")), Some(Atom::new_inline("b"))],
                ty: unit_ty(),
                span: Span::default(),
            },
        ),
    ];
    for (label, thisarg) in cases {
        let mut c = ExprConverter::new();
        c.name_to_function = Arc::new(HashMap::from([(
            Atom::new_inline("f"),
            FunctionId::from_raw(11),
        )]));
        let out = &mut Vec::new();
        let mut cx = ctx();
        let mut full_args = vec![thisarg, HirExpr::Int(1, Span::default())];
        let mir = c.convert_expr(
            &function_method_call_with_args("f", "call", std::mem::take(&mut full_args)),
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx,
        );
        let e0406_matches: Vec<_> = cx
            .diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0406")
            .collect();
        assert_eq!(
            e0406_matches.len(),
            1,
            "{label} as f.call thisArg must emit exactly one E0406 \
             (non-nullish, no `this` binding in AOT), got diagnostics: {:?}",
            cx.diagnostics()
        );
        let e0406 = e0406_matches[0];
        assert!(
            e0406.message.contains("no `this` binding"),
            "{label} as f.call thisArg must use the no-`this`-binding message, got: {:?}",
            e0406.message
        );
        assert!(
            matches!(mir, MirExpr::Unit),
            "{label} as f.call thisArg must return MirExpr::Unit, got {mir:?}"
        );
        assert!(
            out.is_empty(),
            "{label} as f.call thisArg must NOT emit any MirStmt, got: {out:?}"
        );
    }
}
