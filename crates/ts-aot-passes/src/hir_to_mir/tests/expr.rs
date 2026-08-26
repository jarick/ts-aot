use super::common::*;

#[test]
fn convert_expr_unit_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Unit(Span::default()),
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Unit
    );
    assert!(out.is_empty());
}

#[test]
fn convert_expr_bool_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Bool(true, Span::default()),
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Bool(true)
    );
}

#[test]
fn convert_expr_int_emits_struct_with_value() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &int_lit(42),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Int { value, .. } => assert_eq!(value, 42),
        other => panic!("expected Int, got {other:?}"),
    }
    assert!(out.is_empty());
}

#[test]
fn convert_expr_string_emits_string() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &HirExpr::String(Atom::new_inline("5"), Span::default()),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::String { id, .. } => assert_eq!(id, Atom::new_inline("5")),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn convert_expr_null_emits_null() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &HirExpr::Null(Span::default()),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Null { .. }));
}

#[test]
fn convert_expr_undefined_becomes_unit() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Undefined(Span::default()),
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Unit
    );
}

#[test]
fn convert_expr_local_remaps_id() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let old = LocalId::from_raw(7);
    let expr = HirExpr::Local {
        id: old,
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Local(lid) => assert_ne!(lid, old),
        other => panic!("expected Local, got {other:?}"),
    }
    assert_eq!(c.peek_next_local(), 1);
}

#[test]
fn convert_expr_global_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Global {
        name: Atom::new_inline("13"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(mir, MirExpr::Global(Atom::new_inline("13")));
}

#[test]
fn convert_expr_binary_converts_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Binary {
        op: HirBinaryOp::Add,
        lhs: Box::new(int_lit(1)),
        rhs: Box::new(int_lit(2)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(
        mir,
        MirExpr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ));
}

#[test]
fn convert_expr_unary_converts_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Unary {
        op: HirUnaryOp::Not,
        expr: Box::new(HirExpr::Bool(true, Span::default())),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(
        mir,
        MirExpr::Unary {
            op: UnaryOp::Not,
            ..
        }
    ));
}

#[test]
fn convert_expr_field_converts_owner() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Field {
        owner: Box::new(int_lit(0)),
        field: FieldId::from_raw(3),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Field { field, .. } if field == FieldId::from_raw(3)));
}

#[test]
fn convert_expr_index_converts_parts() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Index {
        owner: Box::new(int_lit(0)),
        index: Box::new(int_lit(1)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Index { .. }));
}

#[test]
fn convert_expr_call_resolves_callee() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(2)),
        args: vec![int_lit(1)],
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Call { callee, args, .. } => {
            assert_eq!(callee, FunctionId::from_raw(2));
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Call, got {other:?}"),
    }
}

#[test]
fn convert_expr_call_to_promise_all_lowers_to_runtime_op_with_element_turbofish() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let array_promise_ty = types.intern(&Type::Array {
        element: promise_i64_ty,
    });
    let array_i64_ty = types.intern(&Type::Array { element: i64_ty });
    let result_promise_ty = types.intern(&Type::Promise {
        ok: array_i64_ty,
        err: None,
    });
    let arr_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: array_promise_ty,
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: TypeId::from_raw(0),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("all"),
            ty: result_promise_ty,
            span: Span::default(),
        })),
        args: vec![arr_local],
        ty: result_promise_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let runtime = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, target_ty, .. } => Some((*op, *target_ty)),
            _ => None,
        })
        .expect("expected a Runtime stmt from Promise.all dispatch");
    assert_eq!(runtime.0, RuntimeOp::PromiseAll);
    assert_eq!(
        runtime.1,
        Some(i64_ty),
        "target_ty must extract Promise<i64>::ok = i64 for turbofish"
    );
}

#[test]
fn convert_expr_call_to_promise_race_lowers_to_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let array_promise_ty = types.intern(&Type::Array {
        element: promise_i64_ty,
    });
    let result_promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: TypeId::from_raw(0),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("race"),
            ty: result_promise_ty,
            span: Span::default(),
        })),
        args: vec![HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: array_promise_ty,
            span: Span::default(),
        }],
        ty: result_promise_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let op = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, .. } => Some(*op),
            _ => None,
        })
        .expect("expected a Runtime stmt from Promise.race dispatch");
    assert_eq!(op, RuntimeOp::PromiseRace);
}

#[test]
fn convert_expr_call_to_unknown_promise_method_does_not_match_dispatch() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let array_promise_ty = types.intern(&Type::Array {
        element: promise_ty,
    });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: TypeId::from_raw(0),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("nonexistent"),
            ty: TypeId::from_raw(0),
            span: Span::default(),
        })),
        args: vec![HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: array_promise_ty,
            span: Span::default(),
        }],
        ty: TypeId::from_raw(0),
        type_args: vec![],
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let no_runtime = !out.iter().any(|s| matches!(s, MirStmt::Runtime { .. }));
    assert!(
        no_runtime,
        "Promise.nonexistent must not produce MirStmt::Runtime"
    );
}

#[test]
fn convert_expr_call_to_promise_resolve_lowers_to_runtime_op_with_value_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: TypeId::from_raw(0),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("resolve"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![HirExpr::Int(42, Span::default())],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let runtime = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, target_ty, .. } => Some((*op, *target_ty)),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.resolve dispatch");
    assert_eq!(runtime.0, RuntimeOp::PromiseResolveStatic);
    assert_eq!(
        runtime.1,
        Some(i64_ty),
        "Promise.resolve target_ty must be value's type for turbofish"
    );
}

#[test]
fn convert_expr_call_to_promise_reject_lowers_to_runtime_op_with_promise_inner_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: TypeId::from_raw(0),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("reject"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![HirExpr::String(Atom::new_inline("nope"), Span::default())],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let runtime = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, target_ty, .. } => Some((*op, *target_ty)),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.reject dispatch");
    assert_eq!(runtime.0, RuntimeOp::PromiseRejectStatic);
    assert_eq!(
        runtime.1,
        Some(i64_ty),
        "Promise.reject target_ty must be the outer Promise's ok type"
    );
}

#[test]
fn convert_expr_call_to_promise_then_with_global_handler_lowers_to_instance_runtime_op() {
    let mut c = ExprConverter::new();
    let mut name_to_function: std::collections::HashMap<Atom, FunctionId> =
        std::collections::HashMap::new();
    name_to_function.insert(Atom::new_inline("global_handler"), FunctionId::from_raw(0));
    c.name_to_function = std::sync::Arc::new(name_to_function);
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let promise_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_i64_ty,
        span: Span::default(),
    };
    let handler = HirExpr::Global {
        name: Atom::new_inline("global_handler"),
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(promise_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("then"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![handler],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let op = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, .. } => Some(*op),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.then dispatch");
    assert_eq!(op, RuntimeOp::PromiseThenInstance);
}

#[test]
fn convert_expr_call_to_promise_catch_with_global_handler_lowers_to_instance_runtime_op() {
    let mut c = ExprConverter::new();
    let mut name_to_function: std::collections::HashMap<Atom, FunctionId> =
        std::collections::HashMap::new();
    name_to_function.insert(Atom::new_inline("err_handler"), FunctionId::from_raw(0));
    c.name_to_function = std::sync::Arc::new(name_to_function);
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let promise_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_i64_ty,
        span: Span::default(),
    };
    let handler = HirExpr::Global {
        name: Atom::new_inline("err_handler"),
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(promise_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("catch"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![handler],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let op = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, .. } => Some(*op),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.catch dispatch");
    assert_eq!(op, RuntimeOp::PromiseCatchInstance);
}

#[test]
fn convert_expr_call_to_promise_finally_with_global_handler_lowers_to_instance_runtime_op() {
    let mut c = ExprConverter::new();
    let mut name_to_function: std::collections::HashMap<Atom, FunctionId> =
        std::collections::HashMap::new();
    name_to_function.insert(Atom::new_inline("cleanup"), FunctionId::from_raw(0));
    c.name_to_function = std::sync::Arc::new(name_to_function);
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let promise_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_i64_ty,
        span: Span::default(),
    };
    let handler = HirExpr::Global {
        name: Atom::new_inline("cleanup"),
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(promise_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("finally"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![handler],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let op = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, .. } => Some(*op),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.finally dispatch");
    assert_eq!(op, RuntimeOp::PromiseFinallyInstance);
}

#[test]
fn convert_expr_call_to_promise_then_with_closure_handler_lowers_to_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let promise_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_i64_ty,
        span: Span::default(),
    };
    let closure = HirExpr::Closure {
        id: LocalId::from_raw(1),
        params: vec![],
        captures: vec![],
        body: vec![],
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(promise_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("then"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![closure],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "closure handler must not produce diagnostics, got: {:?}",
        cx.diagnostics()
    );
    let op = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime { op, .. } => Some(*op),
            _ => None,
        })
        .expect("expected Runtime stmt from Promise.then with closure handler");
    assert_eq!(op, RuntimeOp::PromiseThenInstance);
}

#[test]
fn convert_expr_call_to_promise_then_on_non_promise_owner_falls_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let owner_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let handler = HirExpr::Global {
        name: Atom::new_inline("user_then"),
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(owner_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("then"),
            ty: i64_ty,
            span: Span::default(),
        })),
        args: vec![handler],
        ty: i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        !out.iter().any(|s| matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::PromiseThenInstance,
                ..
            }
        )),
        "non-Promise owner must NOT match PromiseThenInstance dispatch (would miscompile \
         user-defined `then` method); got: {out:?}"
    );
    let promise_diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.message.contains("Promise.prototype.then"));
    assert!(
        promise_diag.is_none(),
        "non-Promise owner must not produce Promise-specific diagnostics; got: {promise_diag:?}"
    );
}

#[test]
fn convert_expr_struct_literal_converts_fields() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::StructLiteral {
        ty: unit_ty(),
        fields: vec![(FieldId::from_raw(0), int_lit(7))],

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::StructLiteral { .. }));
}

#[test]
fn convert_expr_distinct_struct_literal_types_get_distinct_struct_ids() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let type_a = TypeId::from_raw(11);
    let type_b = TypeId::from_raw(22);
    let mut shared_ids = empty_struct_ids();
    let mut shared_next = empty_next_struct();
    let mir_a = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_a,
            fields: Vec::new(),

            span: Span::default(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let mir_b = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_b,
            fields: Vec::new(),

            span: Span::default(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let id_a = match mir_a {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    let id_b = match mir_b {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    assert_ne!(
        id_a, id_b,
        "distinct HIR types must map to distinct MIR StructIds (got {id_a:?} and {id_b:?})"
    );
    let mir_a_again = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_a,
            fields: Vec::new(),

            span: Span::default(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let id_a_again = match mir_a_again {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    assert_eq!(
        id_a, id_a_again,
        "same HIR type must map to the same MIR StructId across calls"
    );
}

#[test]
fn convert_expr_array_emits_runtime_stmt() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::ArrayLiteral {
        elements: vec![int_lit(1), int_lit(2)],
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(
        out[0],
        MirStmt::Runtime {
            op: RuntimeOp::ArrayCreate,
            dest: Some(_),
            ..
        }
    ));
}

#[test]
fn convert_expr_array_returns_local_to_dest() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::ArrayLiteral {
        elements: vec![int_lit(1)],
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let dest_id = match &out[0] {
        MirStmt::Runtime { dest: Some(d), .. } => *d,
        other => panic!("expected Runtime with dest, got {other:?}"),
    };
    assert_eq!(mir, MirExpr::Local(dest_id));
}

#[test]
fn convert_expr_template_returns_local_to_dest() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Template {
        tag: None,
        expressions: vec![int_lit(1)],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let runtime_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        runtime_count, 2,
        "3-part template must chain via N-1 = 2 StringConcat Runtime stmts; got out={out:?}"
    );
    let final_dest = match out.last().expect("at least one stmt") {
        MirStmt::Runtime { dest: Some(d), .. } => *d,
        other => panic!("expected trailing StringConcat Runtime with dest, got {other:?}"),
    };
    assert_eq!(
        mir,
        MirExpr::Local(final_dest),
        "convert_expr must return the LAST chained concat's dest local"
    );
}

#[test]

fn convert_expr_await_emits_mir_await_expr() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Await {
        expr: Box::new(int_lit(1)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 0);
    assert!(matches!(mir, MirExpr::Await { expr: _, ty: _ }));
}

#[test]
fn convert_expr_closure_with_empty_body_lowers_to_local_binding() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Closure {
        id: LocalId::from_raw(0),
        params: Vec::new(),
        captures: Vec::new(),
        body: Vec::new(),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "closure should bind to a local, got {mir:?}"
    );
    assert!(
        !cx.has_errors(),
        "empty-body closure must not produce diagnostics"
    );
    assert!(
        !out.is_empty(),
        "closure should emit at least one MirStmt (the Let binding)"
    );
}

#[test]
fn convert_expr_closure_with_captures_emits_p0005_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let captured_local = HirExpr::Local {
        id: LocalId::from_raw(42),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Closure {
        id: LocalId::from_raw(0),
        params: Vec::new(),
        captures: vec![captured_local],
        body: Vec::new(),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(mir, MirExpr::Unit, "capturing closure must lower to Unit");
    assert!(cx.has_errors());
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 diagnostic for capturing closure");
    assert!(diag.message.contains("captur"));
}

#[test]
fn convert_expr_assignment_to_local_emits_local_place() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(local),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        out.len(),
        2,
        "Assignment must emit Let value + Assign (no return-clone), got {out:?}"
    );
    let MirStmt::Let {
        init: Some(value_init),
        local: value_temp,
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=value, got {:?}", out[0]);
    };
    assert!(
        matches!(value_init, MirExpr::Int { value: 7, .. }),
        "Let init must capture the original RHS expression, got {value_init:?}"
    );
    assert!(matches!(
        out[1],
        MirStmt::Assign {
            target: ts_aot_ir_mir::MirPlace::Local { .. },
            value: MirExpr::Local(_),
            ..
        }
    ));
    let MirStmt::Assign {
        value: assign_value,
        ..
    } = &out[1]
    else {
        panic!("expected Assign, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = assign_value else {
        panic!(
            "Assign value must load from the materialized value temp (no value.clone), got {assign_value:?}"
        );
    };
    assert_eq!(
        *assign_src, *value_temp,
        "Assign value must point at the same temp as the Let init"
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_expr_assignment_returns_assigned_value() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(local),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let MirExpr::Local(returned) = mir else {
        panic!("assignment must yield MirExpr::Local(value_temp), got {mir:?}");
    };
    let MirStmt::Let {
        local: value_temp, ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=value, got {:?}", out[0]);
    };
    assert_eq!(
        returned, *value_temp,
        "assignment must yield the same temp that holds the assigned value"
    );
}

#[test]

fn convert_expr_assignment_to_invalid_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(call),
        value: Box::new(int_lit(1)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(cx.has_errors());
    assert!(
        !out.iter().any(|s| matches!(s, MirStmt::Assign { .. })),
        "no Assign must be emitted for invalid target, got {out:?}"
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0006")
        .expect("expected P0006 diagnostic for invalid assignment target");
    assert_eq!(diag.message, "expression is not a valid assignment target");
}

#[test]
fn convert_expr_assignment_to_field_emits_field_place() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let base = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let field = HirExpr::Field {
        owner: Box::new(base),
        field: FieldId::from_raw(2),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 2);
    assert!(matches!(&out[0], MirStmt::Let { init: Some(_), .. }));
    assert!(matches!(
        &out[1],
        MirStmt::Assign {
            target: ts_aot_ir_mir::MirPlace::Field { .. },
            value: MirExpr::Local(_),
            ..
        }
    ));
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Local-owner field access with no struct id registered for unit_ty() must surface P0012 (missing struct id) instead of silently dropping to placeholder; got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_assignment_to_indexed_field_emits_field_with_index_base() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let idx = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: unit_ty(),

        span: Span::default(),
    };
    let indexed = HirExpr::Index {
        owner: Box::new(arr),
        index: Box::new(idx),
        ty: unit_ty(),

        span: Span::default(),
    };
    let field = HirExpr::Field {
        owner: Box::new(indexed),
        field: FieldId::from_raw(3),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 2);
    let MirStmt::Assign { target, .. } = &out[1] else {
        panic!("expected out[1] = Assign, got {:?}", out[1]);
    };
    match target {
        ts_aot_ir_mir::MirPlace::Field { base, field, .. } => {
            assert_eq!(*field, FieldId::from_raw(3));
            assert!(matches!(**base, ts_aot_ir_mir::MirPlaceBase::Index { .. }));
        }
        other => panic!("expected Field place with Index base, got {other:?}"),
    }
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Index-owner field access with no struct id registered must surface P0012; got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_optional_chain_wraps_ty_as_optional_of_inner_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let expr = HirExpr::OptionalChain {
        base: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: unit_ty(),

            span: Span::default(),
        }),
        ty: TypeId::from_raw(7),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());
    let MirExpr::OptionalChain { base, ty } = &mir else {
        panic!("expected MirExpr::OptionalChain, got {mir:?}");
    };
    assert!(matches!(base.as_ref(), MirExpr::Local(_)));
    let expected_opt_ty = types.intern(&ts_aot_core::Type::Optional {
        inner: TypeId::from_raw(0),
    });
    assert_eq!(
        *ty, expected_opt_ty,
        "convert_expr must wrap OptionalChain.ty as Type::Optional {{ inner: <base_inner.ty> }} \
         (PR 1.4 frontend-type-analysis closure). \
         Frontend sets ty to inner type, backend Optional-aware path needs Type::Optional wrapper."
    );
}

#[test]
fn convert_expr_assignment_to_optional_chain_field_emits_chain_base() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let obj = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let chain_base = HirExpr::OptionalChain {
        base: Box::new(obj),
        ty: unit_ty(),

        span: Span::default(),
    };
    let target = HirExpr::Field {
        owner: Box::new(chain_base),
        field: FieldId::from_raw(2),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let MirStmt::Assign { target, .. } = &out[out.len() - 1] else {
        panic!("expected final stmt to be MirStmt::Assign for obj?.x = y, got {out:?}");
    };
    let MirPlace::Field { base, field, .. } = target else {
        panic!("expected MirPlace::Field, got {target:?}");
    };
    assert_eq!(*field, FieldId::from_raw(2));
    let MirPlaceBase::Chain {
        base: chain_base_mir,
        ..
    } = base.as_ref()
    else {
        panic!(
            "MirPlace::Field.base must be MirPlaceBase::Chain (PR 1.4: obj?.x = y wires Chain through mir_expr_to_place), got {base:?}"
        );
    };
    assert!(
        matches!(chain_base_mir.as_ref(), MirExpr::Local(_)),
        "MirPlaceBase::Chain.base must be the materialized inner expression (Local), \
         not wrapped in MirExpr::OptionalChain (PR 1.4: the inverse mapping \
         `Chain -> OptionalChain` lives in mir_place_base_to_expr, kept intact). \
         Got: {chain_base_mir:?}"
    );
}

#[test]
fn convert_expr_indirect_call_emits_indirect_call_arm_for_optional_chain_callee() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let fn_ty = types.intern(&ts_aot_core::Type::I64);
    let opt_fn_ty = types.intern(&ts_aot_core::Type::Optional { inner: fn_ty });
    let obj = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: opt_fn_ty,

        span: Span::default(),
    };
    let optional_chain_callee = HirExpr::OptionalChain {
        base: Box::new(obj),
        ty: opt_fn_ty,

        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(optional_chain_callee)),
        args: vec![int_lit(7)],
        ty: fn_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let MirExpr::IndirectCall { callee, args, .. } = &mir else {
        panic!(
            "expected MirExpr::IndirectCall (PR 1.4: HirCallee::Indirect must always emit IndirectCall, \
             no Runtime::CallIndirect fallback), got {mir:?}"
        );
    };
    let MirExpr::OptionalChain { .. } = callee.as_ref() else {
        panic!(
            "IndirectCall.callee must be the OptionalChain expression (not materialized), got {callee:?}"
        );
    };
    assert_eq!(args.len(), 1, "call args must be preserved");
}

#[test]
fn convert_expr_indirect_call_with_function_typed_callee_emits_e0405_error_and_unit_expr() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let i64_ty = types.intern(&ts_aot_core::Type::I64);
    let fn_ty = types.intern(&ts_aot_core::Type::Fn {
        params: vec![i64_ty],
        ret: i64_ty,
        err: None,
    });
    let cb = HirExpr::Global {
        name: Atom::from("cb"),
        ty: fn_ty,

        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(cb)),
        args: vec![int_lit(42)],
        ty: i64_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "function-typed indirect call must lower to MirExpr::Unit (Type::Fn cannot be called), got {mir:?}"
    );
    let has_e0405 = cx
        .diagnostics()
        .iter()
        .any(|d| d.code.as_str() == "E0405" && d.severity == ts_aot_core::Severity::Error);
    assert!(
        has_e0405,
        "function-typed indirect call must emit E0405 error, got: {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_typeof_lowers_to_mir_typeof_without_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Unary {
        op: HirUnaryOp::TypeOf,
        expr: Box::new(int_lit(1)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "PR 1.6: TypeOf is now a real op (not NotYetImplemented), got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::TypeOf { .. }),
        "TypeOf must lower to MirExpr::TypeOf, got {mir:?}"
    );
}

#[test]
fn convert_expr_new_lowers_callee_for_side_effects() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let global_ty = TypeId::from_raw(0);
    let callee_fn_id = FunctionId::from_raw(99);
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Call {
            callee: HirCallee::Function(callee_fn_id),
            args: Vec::new(),
            ty: global_ty,
            type_args: vec![],

            span: Span::default(),
        }),
        args: Vec::new(),
        ty: global_ty,

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut struct_id_map,
        &mut next_struct_id,
        &mut empty_types(),
        &mut cx,
    );
    let call_callees: Vec<FunctionId> = out
        .iter()
        .filter_map(|s| match s {
            MirStmt::Expr(MirExpr::Call { callee, .. }) => Some(*callee),
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ..
            } => Some(*callee),
            _ => None,
        })
        .collect();
    assert!(
        call_callees.contains(&callee_fn_id),
        "callee's factory call must appear in output before placeholder ctor, got {call_callees:?}"
    );
    assert!(
        call_callees.contains(&PLACEHOLDER_FUNCTION),
        "placeholder ctor call must still appear, got {call_callees:?}"
    );
}

#[test]
fn convert_expr_assignment_to_field_with_call_base_materializes_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(7),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(42)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Call-owner field access with no struct id registered must surface P0012 (missing struct id); got {:?}",
        cx.diagnostics()
    );
    let has_let_for_call = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    assert!(
        has_let_for_call,
        "Call base must be materialized into a temp local via MirStmt::Let"
    );
    let has_assign_to_field = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Assign {
                target: MirPlace::Field { .. },
                ..
            }
        )
    });
    assert!(
        has_assign_to_field,
        "Field assignment must follow the materialized temp local"
    );
}

#[test]
fn convert_expr_assignment_to_field_with_call_base_keeps_call_in_order() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(1)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let let_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    let assign_idx = out.iter().position(|s| matches!(s, MirStmt::Assign { .. }));
    let (Some(li), Some(ai)) = (let_idx, assign_idx) else {
        panic!("expected both materialize-Let and Assign stmts, got {out:?}");
    };
    assert!(
        li < ai,
        "materialize-Let for call base must precede Field Assign, got let@{li}, assign@{ai}"
    );
}

#[test]
fn convert_expr_assignment_lhs_base_materializes_before_rhs_side_effects() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let value_expr = HirExpr::Template {
        tag: None,
        expressions: vec![rhs_call],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(value_expr),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let materialize_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    let rhs_runtime_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                ..
            }
        )
    });
    let (Some(mi), Some(ri)) = (materialize_idx, rhs_runtime_idx) else {
        panic!("expected both materialize-Let and Runtime stmts, got {out:?}");
    };
    assert!(
        mi < ri,
        "LHS base materialize (obj()) must precede RHS side effects (template Runtime); got materialize@{mi}, rhs@{ri}"
    );
}

#[test]
fn span_does_not_block_compile() {
    let _ = Span::new(0, 0);
}

#[test]
fn ternary_preserves_short_circuit_branches_not_in_outer_block() {
    let side_effect_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(false, Span::default())),
                then_branch: Box::new(side_effect_call),
                else_branch: Box::new(HirExpr::Int(0, Span::default())),
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
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &std::sync::Arc::new(empty_hir()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
        &[],
    );
    let outer_stmts = &mir.body.block.stmts;
    let outer_has_call_directly = outer_stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
        )
    });
    assert!(
        !outer_has_call_directly,
        "BUG: then_branch side effect was emitted in the outer block (no short-circuit); outer_stmts={outer_stmts:?}"
    );
    let if_idx = outer_stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::If {
                    cond: MirExpr::Bool(false),
                    ..
                }
            )
        })
        .expect("expected MirStmt::If for the Ternary (cond = false)");
    let MirStmt::If {
        then_block,
        else_block,
        ..
    } = &outer_stmts[if_idx]
    else {
        unreachable!()
    };
    let then_has_call = then_block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
                | MirStmt::Assign {
                    value: MirExpr::Call { .. },
                    ..
                }
        )
    });
    assert!(
        then_has_call,
        "then_branch side effect must live inside then_block"
    );
    let else_block = else_block
        .as_ref()
        .expect("Ternary must produce an else block");
    let else_has_call = else_block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
                | MirStmt::Assign {
                    value: MirExpr::Call { .. },
                    ..
                }
        )
    });
    assert!(
        !else_has_call,
        "else_branch must not contain then_branch call"
    );
}

#[test]
fn sequence_preserves_side_effects_of_intermediate_expressions() {
    let first_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let second_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(8)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Sequence {
                exprs: vec![first_call, second_call],
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
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &std::sync::Arc::new(empty_hir()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
        &[],
    );
    let stmts = &mir.body.block.stmts;
    let first_call_pos = stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { callee, .. }) if *callee == FunctionId::from_raw(7)
        )
    });
    assert!(
        first_call_pos.is_some(),
        "BUG: intermediate Call in Sequence is dropped; must be emitted as MirStmt::Expr before the return; got stmts={stmts:?}"
    );
    let MirStmt::Return(ret_value) = stmts.last().expect("expected trailing Return") else {
        panic!("expected trailing Return, got {stmts:?}");
    };
    let ret_call = ret_value
        .as_ref()
        .expect("return must carry the last sequence element value");
    assert!(
        matches!(ret_call, MirExpr::Call { callee, .. } if *callee == FunctionId::from_raw(8)),
        "Return must carry the LAST sequence element (call to fn #8), got {ret_call:?}"
    );
}

#[test]
fn convert_expr_compound_update_postfix_returns_old_value_via_local() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        2,
        "postfix must emit Let then Assign, got {out:?}"
    );

    let MirStmt::Let {
        local: old_temp,
        init: Some(init),
        ..
    } = &out[0]
    else {
        panic!("expected Let init=load(target), got {:?}", out[0]);
    };
    let init_local = match init {
        MirExpr::Local(id) => *id,
        other => panic!("postfix Let init must be a load of the target local, got {other:?}"),
    };
    assert_eq!(
        init_local,
        LocalId::from_raw(0),
        "postfix Let must capture the target's value before assignment"
    );

    let MirStmt::Assign {
        target: place,
        value,
    } = &out[1]
    else {
        panic!("expected Assign, got {:?}", out[1]);
    };
    let MirExpr::Binary { left, right, .. } = value else {
        panic!("postfix Assign value must be Binary(old + rhs), got {value:?}");
    };
    let MirExpr::Local(left_id) = left.as_ref() else {
        panic!("postfix Binary.left must reuse the old temp, got {left:?}");
    };
    assert_eq!(
        *left_id, *old_temp,
        "postfix Binary.left must reference the old temp captured before the Assign"
    );
    let MirExpr::Int { value: rhs_val, .. } = right.as_ref() else {
        panic!("postfix Binary.right must be rhs MirExpr, got {right:?}");
    };
    assert_eq!(*rhs_val, 1);
    assert!(
        matches!(place, ts_aot_ir_mir::MirPlace::Local { id } if *id == LocalId::from_raw(0)),
        "postfix Assign target must be the original target local, got {place:?}"
    );

    let MirExpr::Local(returned) = mir else {
        panic!("postfix CompoundUpdate must return MirExpr::Local(old_temp), got {mir:?}");
    };
    assert_eq!(
        returned, *old_temp,
        "postfix must return the OLD value, not the new value"
    );
}

#[test]
fn convert_expr_compound_update_prefix_returns_new_value_via_local() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(2)),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        3,
        "prefix must emit Let(old)=target, Let(new)=Binary(old+rhs), Assign, got {out:?}"
    );

    let MirStmt::Let {
        local: old_temp,
        init: Some(old_init),
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=load(target), got {:?}", out[0]);
    };
    let MirExpr::Local(old_init_id) = old_init else {
        panic!("prefix must load old value via MirExpr::Local(target), got {old_init:?}");
    };
    assert_eq!(
        *old_init_id,
        LocalId::from_raw(0),
        "old temp must be initialized by reading the target local"
    );

    let MirStmt::Let {
        local: new_temp,
        init: Some(new_init),
        ..
    } = &out[1]
    else {
        panic!(
            "expected out[1] = Let init=Binary(old + rhs), got {:?}",
            out[1]
        );
    };
    let MirExpr::Binary { left, right, .. } = new_init else {
        panic!("prefix Let init must be Binary(old + rhs), got {new_init:?}");
    };
    let MirExpr::Local(left_id) = left.as_ref() else {
        panic!("prefix Binary.left must reuse the old temp, got {left:?}");
    };
    assert_eq!(
        *left_id, *old_temp,
        "prefix Binary.left must reference the old temp captured before RHS side effects"
    );
    let MirExpr::Int { value: rhs_val, .. } = right.as_ref() else {
        panic!("prefix Binary.right must be rhs MirExpr, got {right:?}");
    };
    assert_eq!(*rhs_val, 2);

    let MirStmt::Assign {
        target: place,
        value,
    } = &out[2]
    else {
        panic!("expected out[2] = Assign, got {:?}", out[2]);
    };
    let MirExpr::Local(assign_src) = value else {
        panic!("prefix Assign value must be MirExpr::Local(new_temp), got {value:?}");
    };
    assert_eq!(
        *assign_src, *new_temp,
        "prefix Assign must write from the materialized new-value temp"
    );
    assert!(
        matches!(place, ts_aot_ir_mir::MirPlace::Local { id } if *id == LocalId::from_raw(0)),
        "prefix Assign target must be the original target local, got {place:?}"
    );

    let MirExpr::Local(returned) = mir else {
        panic!("prefix CompoundUpdate must return MirExpr::Local(new_temp), got {mir:?}");
    };
    assert_eq!(
        returned, *new_temp,
        "prefix must return the materialized NEW value"
    );
}

#[test]
fn convert_expr_compound_update_rhs_call_evaluated_only_once() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_call),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut call_in_init_count = 0;
    let mut assign_value_is_binary = false;
    for stmt in out.iter() {
        match stmt {
            MirStmt::Let {
                init: Some(init), ..
            } if expr_contains_call(init) => {
                call_in_init_count += 1;
            }
            MirStmt::Assign {
                value: MirExpr::Binary { .. },
                ..
            } => assign_value_is_binary = true,
            _ => {}
        }
    }
    assert_eq!(
        call_in_init_count, 1,
        "rhs Call must appear in exactly one Let init (the materialized new value), got {out:?}"
    );
    assert!(
        !assign_value_is_binary,
        "Assign value must not be a Binary (which would re-run rhs on every place eval), got {out:?}"
    );

    let MirStmt::Assign { value, .. } = &out[2] else {
        panic!("expected Assign at index 2, got {:?}", out[2]);
    };
    let MirStmt::Let {
        local: new_temp, ..
    } = &out[1]
    else {
        panic!("expected Let new_temp at index 1, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = value else {
        panic!(
            "Assign value must be MirExpr::Local pointing at the materialized new temp, got {value:?}"
        );
    };
    assert_eq!(
        *assign_src, *new_temp,
        "Assign must write from the materialized new-value temp"
    );
}

#[test]
fn convert_expr_compound_update_postfix_index_target_materializes_base_and_index() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(9)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut arr_call_inits = 0;
    let mut i_call_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(7) {
                arr_call_inits += 1;
            } else if *callee == FunctionId::from_raw(9) {
                i_call_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_call_inits, 1,
        "arr() must be materialized exactly once (in target base), got {out:?}"
    );
    assert_eq!(
        i_call_inits, 1,
        "i() must be materialized exactly once (in target index), got {out:?}"
    );

    let assign_target = match out.last() {
        Some(MirStmt::Assign { target, .. }) => target,
        other => panic!("expected last stmt to be Assign, got {other:?}"),
    };
    fn assert_place_is_pure(place: &MirPlace, path: &str, out: &[MirStmt]) {
        match place {
            MirPlace::Local { .. } => {}
            MirPlace::Field { base, .. } => {
                assert_place_base_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirPlace::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
        }
    }
    fn assert_place_base_is_pure(base: &MirPlaceBase, path: &str, out: &[MirStmt]) {
        match base {
            MirPlaceBase::Local(_) => {}
            MirPlaceBase::Field { base, .. } => {
                assert_place_base_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirPlaceBase::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
            MirPlaceBase::Chain { base, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.chain-base"), out);
            }
        }
    }
    fn assert_mir_expr_is_pure(expr: &MirExpr, path: &str, out: &[MirStmt]) {
        match expr {
            MirExpr::Local(_) => {}
            MirExpr::Field { base, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirExpr::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
            other => panic!(
                "Assign target subtree at {path} must be a pure Local/Field/Index, got {other:?}; full out: {out:?}"
            ),
        }
    }
    assert_place_is_pure(assign_target, "Assign.target", out);
}

#[test]
fn convert_expr_compound_update_prefix_index_target_materializes_base_and_index() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(11)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(13)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut arr_inits = 0;
    let mut i_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(11) {
                arr_inits += 1;
            } else if *callee == FunctionId::from_raw(13) {
                i_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_inits, 1,
        "arr() in prefix ++ must also be materialized once, got {out:?}"
    );
    assert_eq!(
        i_inits, 1,
        "i() in prefix ++ must also be materialized once, got {out:?}"
    );
}

#[test]
fn convert_expr_compound_update_postfix_index_then_field_target_materializes_all() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(17)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(19)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let index_target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let target = HirExpr::Field {
        owner: Box::new(index_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Field-owner for arr()[i()].field++ with no struct id registered must surface P0012; got {:?}",
        cx.diagnostics()
    );

    let mut arr_inits = 0;
    let mut i_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(17) {
                arr_inits += 1;
            } else if *callee == FunctionId::from_raw(19) {
                i_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_inits, 1,
        "nested arr()[i()].field++ must materialize arr() once, got {out:?}"
    );
    assert_eq!(
        i_inits, 1,
        "nested arr()[i()].field++ must materialize i() once, got {out:?}"
    );
}

#[test]
fn convert_expr_compound_update_index_target_plus_call_rhs_each_call_once() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(21)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(23)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(25)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_call),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut counts: HashMap<u32, usize> = HashMap::new();
    fn visit(e: &MirExpr, counts: &mut HashMap<u32, usize>) {
        if let MirExpr::Call { callee, .. } = e {
            let key = callee.raw();
            counts.entry(key).and_modify(|c| *c += 1).or_insert(1);
        }
        match e {
            MirExpr::Binary { left, right, .. } => {
                visit(left, counts);
                visit(right, counts);
            }
            MirExpr::Field { base, .. } => visit(base, counts),
            MirExpr::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
            MirExpr::Unary { expr, .. } => visit(expr, counts),
            MirExpr::Call { args, .. } => {
                for a in args {
                    visit(a, counts);
                }
            }
            MirExpr::Await { expr, .. } => visit(expr, counts),
            MirExpr::Yield { expr, .. } => {
                if let Some(e) = expr.as_ref() {
                    visit(e, counts);
                }
            }
            _ => {}
        }
    }
    fn visit_place(p: &MirPlace, counts: &mut HashMap<u32, usize>) {
        match p {
            MirPlace::Local { .. } => {}
            MirPlace::Field { base, .. } => visit_place_base(base, counts),
            MirPlace::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
        }
    }
    fn visit_place_base(b: &MirPlaceBase, counts: &mut HashMap<u32, usize>) {
        match b {
            MirPlaceBase::Local(_) => {}
            MirPlaceBase::Field { base, .. } => visit_place_base(base, counts),
            MirPlaceBase::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
            MirPlaceBase::Chain { base, .. } => visit(base, counts),
        }
    }
    for stmt in out.iter() {
        match stmt {
            MirStmt::Let {
                init: Some(init), ..
            } => visit(init, &mut counts),
            MirStmt::Assign { target, value } => {
                visit_place(target, &mut counts);
                visit(value, &mut counts);
            }
            _ => {}
        }
    }
    assert_eq!(
        counts.get(&21).copied().unwrap_or(0),
        1,
        "arr() must run once, got {:?}; out: {out:?}",
        counts
    );
    assert_eq!(
        counts.get(&23).copied().unwrap_or(0),
        1,
        "i() must run once, got {:?}; out: {out:?}",
        counts
    );
    assert_eq!(
        counts.get(&25).copied().unwrap_or(0),
        1,
        "rhs f() must run once, got {:?}; out: {out:?}",
        counts
    );
}

#[test]
fn convert_expr_compound_update_loads_old_value_before_rhs_runtime_stmt() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let f_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(101)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let rhs_template = HirExpr::Template {
        tag: None,
        expressions: vec![f_call],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_template),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let old_let_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Local(id)),
                ..
            } if *id == LocalId::from_raw(0)
        )
    });
    let rhs_runtime_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                ..
            }
        )
    });
    let (Some(li), Some(ri)) = (old_let_idx, rhs_runtime_idx) else {
        panic!(
            "expected both `Let old=target` and `MirStmt::Runtime(StringConcat)` to be emitted, got {out:?}"
        );
    };
    assert!(
        li < ri,
        "JS/TS compound assignment must read LHS (Let old=target) BEFORE evaluating the RHS (MirStmt::Runtime for template); otherwise an RHS that mutates the target would corrupt `old`. got let@{li}, rhs_runtime@{ri}; out: {out:?}"
    );

    let f_call_in_runtime_args = out.iter().any(|s| {
        if let MirStmt::Runtime {
            args,
            op: RuntimeOp::StringConcat,
            ..
        } = s
        {
            args.iter()
                .any(|a| matches!(a, MirExpr::Call { callee, .. } if callee.raw() == 101))
        } else {
            false
        }
    });
    assert!(
        f_call_in_runtime_args,
        "the RHS `f()` must end up inside the StringConcat Runtime stmt (i.e. as an arg), not duplicated elsewhere; got {out:?}"
    );

    let binary_left_uses_old_temp = out.iter().any(|s| {
        if let MirStmt::Let {
            init: Some(MirExpr::Binary { left, .. }),
            ..
        } = s
        {
            matches!(left.as_ref(), MirExpr::Local(_))
        } else {
            false
        }
    });
    assert!(
        binary_left_uses_old_temp,
        "the new-value Binary.left must reference the old temp (so the computed value uses the value read BEFORE the RHS mutation), not the live target; got {out:?}"
    );
}

#[test]
fn convert_expr_assignment_value_temp_carries_rhs_ty_not_type_zero() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let rhs_ty = TypeId::from_raw(17);
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(505)),
        args: Vec::new(),
        ty: rhs_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs_call),
        ty: rhs_ty,

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let MirStmt::Let {
        local: value_temp,
        ty: let_ty,
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=rhs, got {:?}", out[0]);
    };
    assert_eq!(
        *let_ty, rhs_ty,
        "Let init for value_temp must declare the RHS type ({rhs_ty:?}), not TypeId(0) — the prior code used TypeId(0) which silently mis-typed the materialized local"
    );

    let MirStmt::Assign {
        value: assign_value,
        ..
    } = &out[1]
    else {
        panic!("expected out[1] = Assign, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = assign_value else {
        panic!("Assign value must be MirExpr::Local(value_temp), got {assign_value:?}");
    };
    assert_eq!(
        *assign_src, *value_temp,
        "Assign must read from the same value_temp declared with the correct ty"
    );
}

#[test]
fn convert_expr_assignment_casts_rhs_to_primitive_target_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: f64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: i64_ty,
        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: f64_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());

    assert_eq!(
        out.len(),
        2,
        "expected exactly 2 stmts (Let + Assign), got: {:?}",
        out
    );

    let MirStmt::Let {
        local: value_temp,
        ty: let_ty,
        init,
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let, got {:?}", out[0]);
    };
    assert_eq!(
        *let_ty, f64_ty,
        "value_temp must be declared with the primitive target type when a cast applies"
    );
    assert!(
        matches!(
            init,
            Some(MirExpr::Cast {
                expr,
                ty
            }) if *ty == f64_ty && matches!(expr.as_ref(), MirExpr::Local(_))
        ),
        "Let init must be a Cast to the target type, got {init:?}"
    );

    let MirStmt::Assign {
        value: assign_value,
        ..
    } = &out[1]
    else {
        panic!("expected out[1] = Assign, got {:?}", out[1]);
    };
    assert_eq!(
        assign_value,
        &MirExpr::Local(*value_temp),
        "Assign must read from the cast temp"
    );
}

#[test]
fn convert_expr_assignment_with_string_rhs_to_f64_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let f64_ty = types.intern(&Type::F64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: f64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: str_ty,
        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: str_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "F64 target with String RHS must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0408")
        .expect("expected E0408 diagnostic for string-to-numeric assignment");
    assert!(
        diag.message.contains("string"),
        "E0408 diagnostic must mention the string type, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_void_rhs_to_typed_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Unit(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: void_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "I64 target with void/Unit RHS must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0409")
        .expect("expected E0409 diagnostic for void-to-typed assignment");
    assert!(
        diag.message.contains("void") || diag.message.contains("Unit"),
        "E0409 diagnostic must mention void/Unit, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_undefined_rhs_to_error_typed_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Undefined(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: error_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "i64 target with Undefined RHS resolving to Type::Error must produce a diagnostic, \
         got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0409")
        .expect("expected E0409 diagnostic for Undefined-to-Error assignment");
    assert!(
        diag.message.contains("void") || diag.message.contains("Unit"),
        "E0409 diagnostic must mention void/Unit, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_i64_rhs_to_bool_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: bool_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: i64_ty,
        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: i64_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "Bool target with i64 RHS must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for bool<->numeric assignment");
    assert!(
        diag.message.contains("boolean"),
        "E0410 diagnostic must mention boolean, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_string_literal_rhs_to_numeric_target_falls_back_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::String(Atom::new_inline("hello"), Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: error_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "i64 target with String literal RHS (Error-typed) must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0408")
        .expect("expected E0408 diagnostic for string-literal-to-numeric assignment");
    assert!(
        diag.message.contains("string"),
        "E0408 diagnostic must mention string, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_numeric_rhs_to_string_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let _i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: string_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Int(42, Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: string_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "string target with numeric RHS must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0408")
        .expect("expected E0408 diagnostic for numeric-RHS-to-string-target assignment");
    assert!(
        diag.message.contains("string"),
        "E0408 diagnostic must mention string, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_bool_literal_rhs_to_numeric_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Bool(true, Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: error_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "i64 target with Bool literal RHS (Error-typed) must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for bool-literal-to-numeric assignment");
    assert!(
        diag.message.contains("boolean"),
        "E0410 diagnostic must mention boolean, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_null_rhs_to_optional_target_emits_typed_none() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let optional_i64_ty = types.intern(&Type::Optional { inner: i64_ty });
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: optional_i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Null(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: optional_i64_ty,
        span: Span::default(),
    };
    let result = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "null assignment to Type::Optional<i64> target must be accepted, got: {:?}",
        cx.diagnostics()
    );
    let MirStmt::Let {
        ty: declared_ty,
        init: Some(init_expr),
        ..
    } = &out[0]
    else {
        panic!(
            "expected first stmt to be a MirStmt::Let, got: {:?}",
            out[0]
        );
    };
    assert_eq!(
        *declared_ty, optional_i64_ty,
        "MirStmt::Let.ty must be the target optional type, not the value's plain Null ty, \
         got: {:?}",
        declared_ty
    );
    let MirExpr::Null { ty: null_ty } = init_expr else {
        panic!("init expr must be MirExpr::Null, got: {:?}", init_expr);
    };
    assert_eq!(
        *null_ty, optional_i64_ty,
        "MirExpr::Null.ty must be the target optional type, not the default Null ty, got: {:?}",
        null_ty
    );
    assert!(
        matches!(result, MirExpr::Local(_)),
        "assignment result must be a MirExpr::Local referencing the temp, got: {:?}",
        result
    );
}

#[test]
fn convert_expr_assignment_with_null_rhs_to_non_nullable_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Null(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: i64_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "null assignment to non-nullable Type::I64 target must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for null assignment to non-nullable target");
    assert!(
        diag.message.contains("null"),
        "E0410 diagnostic must mention null, got: {:?}",
        diag.message
    );
}

#[test]
fn convert_expr_assignment_with_null_rhs_to_union_with_null_target_emits_typed_none() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let null_ty = types.intern(&Type::Null);
    let string_ty = types.intern(&Type::String);
    let union_ty = types.intern(&Type::Union {
        variants: vec![string_ty, null_ty],
    });
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: union_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Null(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: union_ty,
        span: Span::default(),
    };
    let result = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "null assignment to Type::Union{{String, Null}} target must be accepted (target is \
         nullable because one of the variants is Null), got: {:?}",
        cx.diagnostics()
    );
    let MirStmt::Let {
        ty: declared_ty,
        init: Some(init_expr),
        ..
    } = &out[0]
    else {
        panic!(
            "expected first stmt to be a MirStmt::Let, got: {:?}",
            out[0]
        );
    };
    assert_eq!(
        *declared_ty, union_ty,
        "MirStmt::Let.ty must be the target union type, got: {:?}",
        declared_ty
    );
    let MirExpr::Null { ty: null_expr_ty } = init_expr else {
        panic!("init expr must be MirExpr::Null, got: {:?}", init_expr);
    };
    assert_eq!(
        *null_expr_ty, union_ty,
        "MirExpr::Null.ty must be the target union type, got: {:?}",
        null_expr_ty
    );
    let _ = i64_ty;
    assert!(
        matches!(result, MirExpr::Local(_)),
        "assignment result must be a MirExpr::Local referencing the temp, got: {:?}",
        result
    );
}

#[test]
fn convert_expr_assignment_with_int_literal_rhs_to_bool_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let bool_ty = types.intern(&Type::Bool);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: bool_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Int(42, Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: error_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "Bool target with Int literal RHS (Error-typed) must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for int-literal-to-bool assignment");
    assert!(
        diag.message.contains("boolean"),
        "E0410 diagnostic must mention boolean, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_assignment_with_null_literal_rhs_to_numeric_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Null(Span::default());
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: error_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "i64 target with Null literal RHS (Error-typed) must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for null-literal-to-numeric assignment");
    assert!(
        diag.message.contains("null"),
        "E0410 diagnostic must mention null, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected assignment must not emit Let/Assign stmts, got: {:?}",
        out
    );
}

#[test]
fn convert_expr_compound_update_with_bool_literal_rhs_to_numeric_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: i64_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Bool(true, Span::default());
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs),
        post: false,
        ty: i64_ty,
        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(
        cx.has_errors(),
        "i64 target with Bool literal RHS in compound update must produce a diagnostic, got: {:?}",
        cx.diagnostics()
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0410")
        .expect("expected E0410 diagnostic for compound update with bool-literal rhs");
    assert!(
        diag.message.contains("boolean"),
        "E0410 diagnostic must mention boolean, got: {:?}",
        diag.message
    );
    assert!(
        out.is_empty(),
        "rejected compound update must not emit any MirStmt, got: {:?}",
        out
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "rejected compound update must return MirExpr::Unit, got: {:?}",
        mir
    );
}

#[test]
fn convert_expr_compound_update_with_i64_rhs_casts_to_f64_target_in_postfix_and_prefix() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let i64_ty = types.intern(&Type::I64);

    out.clear();
    let target_post = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: f64_ty,
        span: Span::default(),
    };
    let rhs_i64 = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: i64_ty,
        span: Span::default(),
    };
    let postfix_expr = HirExpr::CompoundUpdate {
        target: Box::new(target_post),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_i64),
        post: true,
        ty: f64_ty,
        span: Span::default(),
    };
    let mir = c.convert_expr(
        &postfix_expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        2,
        "postfix must emit Let(old)=target, Assign(target)=Binary(old, Cast(rhs, f64)), got: {:?}",
        out
    );
    let MirStmt::Let {
        local: old_temp_post,
        ty: old_ty_post,
        ..
    } = &out[0]
    else {
        panic!(
            "postfix out[0] must be Let binding the old value, got: {:?}",
            out[0]
        );
    };
    assert_eq!(
        *old_ty_post, f64_ty,
        "postfix old_temp must be typed f64 (target type), got: {:?}",
        old_ty_post
    );
    let MirStmt::Assign { value, .. } = &out[1] else {
        panic!(
            "postfix out[1] must be Assign with Binary value, got: {:?}",
            out[1]
        );
    };
    let MirExpr::Binary {
        left,
        right,
        ty: bin_ty_post,
        ..
    } = value
    else {
        panic!(
            "postfix Assign value must be Binary(old + rhs_cast), got: {:?}",
            value
        );
    };
    assert_eq!(
        *bin_ty_post, f64_ty,
        "postfix Binary.ty must be f64, got: {:?}",
        bin_ty_post
    );
    let MirExpr::Local(left_id_post) = left.as_ref() else {
        panic!(
            "postfix Binary.left must reference old_temp, got: {:?}",
            left
        );
    };
    assert_eq!(
        *left_id_post, *old_temp_post,
        "postfix Binary.left must be the old_temp captured before RHS evaluation"
    );
    let MirExpr::Cast {
        expr: cast_inner,
        ty: cast_ty_post,
    } = right.as_ref()
    else {
        panic!(
            "postfix Binary.right must be MirExpr::Cast for i64 -> f64, got: {:?}",
            right
        );
    };
    assert_eq!(
        *cast_ty_post, f64_ty,
        "postfix Cast.ty must target f64 (the f64 target type), got: {:?}",
        cast_ty_post
    );
    let MirExpr::Local(rhs_local_id) = cast_inner.as_ref() else {
        panic!(
            "postfix Cast.expr must reference the i64 RHS local, got: {:?}",
            cast_inner
        );
    };
    assert_ne!(
        *rhs_local_id, *old_temp_post,
        "postfix Cast.expr must be a different local from old_temp (RHS, not target)"
    );
    let MirExpr::Local(returned) = mir else {
        panic!(
            "postfix CompoundUpdate must return MirExpr::Local(old_temp), got: {:?}",
            mir
        );
    };
    assert_eq!(
        returned, *old_temp_post,
        "postfix CompoundUpdate must return old_temp (the pre-update value)"
    );

    out.clear();
    let target_pre = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: f64_ty,
        span: Span::default(),
    };
    let rhs_i64_pre = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: i64_ty,
        span: Span::default(),
    };
    let prefix_expr = HirExpr::CompoundUpdate {
        target: Box::new(target_pre),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_i64_pre),
        post: false,
        ty: f64_ty,
        span: Span::default(),
    };
    let mir = c.convert_expr(
        &prefix_expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        3,
        "prefix must emit Let(old)=target, Let(new)=Binary(old, Cast(rhs, f64)), Assign(target)=Local(new), got: {:?}",
        out
    );
    let MirStmt::Let {
        local: old_temp_pre,
        ty: old_ty_pre,
        ..
    } = &out[0]
    else {
        panic!(
            "prefix out[0] must be Let binding the old value, got: {:?}",
            out[0]
        );
    };
    assert_eq!(
        *old_ty_pre, f64_ty,
        "prefix old_temp must be typed f64 (target type), got: {:?}",
        old_ty_pre
    );
    let MirStmt::Let {
        local: new_temp_pre,
        init: Some(new_init),
        ty: new_ty_pre,
        ..
    } = &out[1]
    else {
        panic!(
            "prefix out[1] must be Let binding the new value, got: {:?}",
            out[1]
        );
    };
    assert_eq!(
        *new_ty_pre, f64_ty,
        "prefix new_temp must be typed f64 (target type), got: {:?}",
        new_ty_pre
    );
    let MirExpr::Binary {
        left: pre_left,
        right: pre_right,
        ty: pre_bin_ty,
        ..
    } = new_init
    else {
        panic!(
            "prefix Let(new) init must be Binary(old + rhs_cast), got: {:?}",
            new_init
        );
    };
    assert_eq!(
        *pre_bin_ty, f64_ty,
        "prefix Binary.ty must be f64, got: {:?}",
        pre_bin_ty
    );
    let MirExpr::Local(pre_left_id) = pre_left.as_ref() else {
        panic!(
            "prefix Binary.left must reference old_temp, got: {:?}",
            pre_left
        );
    };
    assert_eq!(
        *pre_left_id, *old_temp_pre,
        "prefix Binary.left must be the old_temp captured before RHS evaluation"
    );
    let MirExpr::Cast {
        expr: pre_cast_inner,
        ty: pre_cast_ty,
    } = pre_right.as_ref()
    else {
        panic!(
            "prefix Binary.right must be MirExpr::Cast for i64 -> f64, got: {:?}",
            pre_right
        );
    };
    assert_eq!(
        *pre_cast_ty, f64_ty,
        "prefix Cast.ty must target f64 (the f64 target type), got: {:?}",
        pre_cast_ty
    );
    let MirExpr::Local(pre_rhs_id) = pre_cast_inner.as_ref() else {
        panic!(
            "prefix Cast.expr must reference the i64 RHS local, got: {:?}",
            pre_cast_inner
        );
    };
    assert_ne!(
        *pre_rhs_id, *old_temp_pre,
        "prefix Cast.expr must be a different local from old_temp (RHS, not target)"
    );
    assert_ne!(
        *pre_rhs_id, *new_temp_pre,
        "prefix Cast.expr must be a different local from new_temp (RHS, not computed value)"
    );
    let MirStmt::Assign {
        target: pre_place,
        value: pre_assign_value,
    } = &out[2]
    else {
        panic!("prefix out[2] must be Assign, got: {:?}", out[2]);
    };
    let MirExpr::Local(pre_assign_src) = pre_assign_value else {
        panic!(
            "prefix Assign value must be MirExpr::Local(new_temp), got: {:?}",
            pre_assign_value
        );
    };
    assert_eq!(
        *pre_assign_src, *new_temp_pre,
        "prefix Assign must write from the materialized new-value temp"
    );
    assert!(
        matches!(pre_place, ts_aot_ir_mir::MirPlace::Local { id } if *id == LocalId::from_raw(0)),
        "prefix Assign target must be the original f64 target local, got: {:?}",
        pre_place
    );
    let MirExpr::Local(pre_returned) = mir else {
        panic!(
            "prefix CompoundUpdate must return MirExpr::Local(new_temp), got: {:?}",
            mir
        );
    };
    assert_eq!(
        pre_returned, *new_temp_pre,
        "prefix CompoundUpdate must return new_temp (the post-update value)"
    );
}

#[test]
fn convert_expr_assignment_rhs_call_materialized_once_for_statement_and_return() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),

        span: Span::default(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(303)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let rhs_call_in_let_inits = out
        .iter()
        .filter(|s| {
            if let MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ..
            } = s
            {
                callee.raw() == 303
            } else {
                false
            }
        })
        .count();
    assert_eq!(
        rhs_call_in_let_inits, 1,
        "rhs Call must appear in exactly one Let init (the materialized value temp), got {out:?}"
    );

    let rhs_call_in_assign_values = out
        .iter()
        .filter(|s| {
            if let MirStmt::Assign {
                value: MirExpr::Call { callee, .. },
                ..
            } = s
            {
                callee.raw() == 303
            } else {
                false
            }
        })
        .count();
    assert_eq!(
        rhs_call_in_assign_values, 0,
        "Assign value must NOT be a Call (would re-run rhs in statement-context Expr), got {out:?}"
    );
}

#[test]
fn convert_expr_assignment_field_target_with_call_base_materializes_call_with_call_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let obj_ty = TypeId::from_raw(31);
    let obj_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(606)),
        args: Vec::new(),
        ty: obj_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(obj_call),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("x"),
        ty: obj_ty,

        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "non-registered struct id for Call-owner with ty {obj_ty:?} must surface P0012; got {:?}",
        cx.diagnostics()
    );

    let materialize_let = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ty,
                ..
            } if callee.raw() == 606 => Some(*ty),
            _ => None,
        })
        .expect("expected Let init=Call(obj) from materialize callback");
    assert_eq!(
        materialize_let, obj_ty,
        "MirStmt::Let for materialized obj() must declare the Call's ty ({obj_ty:?}), not TypeId(0) — downstream consumers see the wrong type otherwise"
    );
}

#[test]
fn convert_expr_compound_update_index_target_materializes_arr_call_with_arr_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_ty = TypeId::from_raw(53);
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(707)),
        args: Vec::new(),
        ty: arr_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(709)),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),

        span: Span::default(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: false,
        ty: unit_ty(),

        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors(), "arr()[i()]++ must not error");

    let arr_materialize_let_ty = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ty,
                ..
            } if callee.raw() == 707 => Some(*ty),
            _ => None,
        })
        .expect("expected Let init=Call(arr) from ensure_place_pure_components");
    assert_eq!(
        arr_materialize_let_ty, arr_ty,
        "MirStmt::Let for materialized arr() must declare the Call's ty ({arr_ty:?}), not TypeId(0)"
    );
}

#[test]
fn convert_expr_assignment_casts_i64_local_to_f32() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f32_ty = types.intern(&Type::F32);
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: f32_ty,
        span: Span::default(),
    };
    let rhs = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: i64_ty,
        span: Span::default(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs),
        ty: f32_ty,
        span: Span::default(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());

    let MirStmt::Let {
        ty: let_ty,
        init: Some(init),
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let with init, got {:?}", out[0]);
    };
    assert_eq!(
        *let_ty, f32_ty,
        "value_temp for i64 -> f32 assignment must be declared with f32 type"
    );
    assert!(
        matches!(
            init,
            MirExpr::Cast { expr, ty } if *ty == f32_ty && matches!(expr.as_ref(), MirExpr::Local(_))
        ),
        "i64 -> f32 assignment must emit MirExpr::Cast to f32, got {init:?}"
    );
}

#[test]
fn convert_expr_call_to_promise_then_with_mismatched_handler_emits_e0504_via_set_program() {
    let mut c = ExprConverter::new();
    let mut name_to_function: std::collections::HashMap<Atom, FunctionId> =
        std::collections::HashMap::new();
    name_to_function.insert(Atom::new_inline("bad_handler"), FunctionId::from_raw(0));
    c.name_to_function = std::sync::Arc::new(name_to_function);
    let mut types = TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let i64_ty = types.intern(&Type::I64);
    let promise_i64_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("bad_handler"),
        params: vec![HirParam {
            name: Atom::from("x"),
            ty: bool_ty,
        }],
        ret: i64_ty,
        throws: None,
        body: Vec::new(),
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    c.set_program(std::sync::Arc::new(prog));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let promise_local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_i64_ty,
        span: Span::default(),
    };
    let handler = HirExpr::Global {
        name: Atom::new_inline("bad_handler"),
        ty: TypeId::from_raw(0),
        span: Span::default(),
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(promise_local),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("then"),
            ty: promise_i64_ty,
            span: Span::default(),
        })),
        args: vec![handler],
        ty: promise_i64_ty,
        type_args: vec![],
        span: Span::default(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        mir,
        MirExpr::Unit,
        "mismatched handler signature must short-circuit to Unit (not fall through to generic call)"
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "E0504")
        .expect("expected E0504 diagnostic from validate_promise_handler_param_and_return");
    assert!(
        diag.message.contains("bad_handler"),
        "E0504 message must reference the offending handler name; got: {diag:?}"
    );
    assert!(
        diag.message.contains("parameter type is not"),
        "E0504 must report parameter-type mismatch (not arity); got: {diag:?}"
    );
}
