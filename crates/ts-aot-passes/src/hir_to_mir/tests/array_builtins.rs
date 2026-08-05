use super::common::*;

#[test]
fn array_is_array_call_emits_array_is_array_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let mir = c.convert_expr(
        &array_method_call_with_arg(
            "isArray",
            HirExpr::Local {
                id: LocalId::from_raw(11),
                ty: arr_ty,
                span: Span::default(),
            },
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "Array.isArray(value) must not emit E0404, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.isArray(value) must return a Local, got {mir:?}"
    );
    let is_array_args = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayIsArray,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert_eq!(
        is_array_args.map(Vec::len),
        Some(1),
        "Array.isArray(value) must pass exactly 1 arg to __ts_aot_array_is_array (the value, no implicit receiver); got args={is_array_args:?}, full out: {out:?}"
    );
}

#[test]
fn array_is_array_call_with_non_array_arg_emits_array_is_array_false_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mir = c.convert_expr(
        &array_method_call_with_arg(
            "isArray",
            HirExpr::Local {
                id: LocalId::from_raw(13),
                ty: i64_ty,
                span: Span::default(),
            },
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "Array.isArray(value) with non-array arg must not emit E0404, got {:?}",
        cx.diagnostics()
    );
    let false_op_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayIsArrayFalse,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        false_op_count, 1,
        "Array.isArray(value) with non-array arg must emit exactly 1 ArrayIsArrayFalse op; got out={out:?}, mir={mir:?}"
    );
}

#[test]
fn array_unknown_namespace_method_falls_through_to_indirect_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &array_method_call_with_arg(
            "flat",
            HirExpr::Local {
                id: LocalId::from_raw(12),
                ty: unit_ty(),
                span: Span::default(),
            },
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "Array.flat() (not in supported set) must not emit E0404, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::IndirectCall { .. }),
        "Array.flat() must fall through to MirExpr::IndirectCall, got {mir:?}"
    );
}

#[test]
fn new_array_with_no_args_emits_array_create_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Global {
            name: Atom::new_inline("Array"),
            ty: unit_ty(),
            span: Span::default(),
        }),
        args: Vec::new(),
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "new Array() must return MirExpr::Local, got {mir:?}"
    );
    let has_create = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                ..
            }
        )
    });
    assert!(
        has_create,
        "new Array() must emit MirStmt::Runtime {{ op: ArrayCreate, .. }}, got: {out:?}"
    );
    let has_struct_let = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::StructLiteral { .. }),
                ..
            }
        )
    });
    assert!(
        !has_struct_let,
        "new Array() must NOT take the struct-literal fallback path, got: {out:?}"
    );
}

#[test]
fn new_array_with_one_arg_emits_array_create_with_len_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Global {
            name: Atom::new_inline("Array"),
            ty: unit_ty(),
            span: Span::default(),
        }),
        args: vec![HirExpr::Int(5, Span::default())],
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "new Array(5) must return MirExpr::Local, got {mir:?}"
    );
    let create_with_len = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayCreateWithLen,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert_eq!(
        create_with_len.map(Vec::len),
        Some(1),
        "new Array(5) must emit ArrayCreateWithLen with 1 arg (the length), got: {out:?}"
    );
    let push_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        push_count, 0,
        "new Array(5) must NOT emit ArrayPush (single numeric arg = length, not item), got: {out:?}"
    );
}

#[test]
fn new_array_with_non_numeric_single_arg_emits_create_then_push() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let arr_ty = types.intern(&Type::Array { element: string_ty });
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Global {
            name: Atom::new_inline("Array"),
            ty: unit_ty(),
            span: Span::default(),
        }),
        args: vec![HirExpr::String(Atom::new_inline("hello"), Span::default())],
        ty: arr_ty,
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
    let has_create_with_len = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !has_create_with_len,
        "new Array(\"hello\") (non-numeric single arg) must NOT emit ArrayCreateWithLen \
         (JS semantics: non-numeric single arg = 1-element array, not length-N array), got: {out:?}"
    );
    let has_create = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                ..
            }
        )
    });
    assert!(
        has_create,
        "new Array(\"hello\") (non-numeric single arg) must emit ArrayCreate, got: {out:?}"
    );
    let push_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        push_count, 1,
        "new Array(\"hello\") (non-numeric single arg) must emit 1 ArrayPush (one-element array), got: {out:?}"
    );
}

#[test]
fn new_array_with_multi_args_emits_create_then_pushes() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Global {
            name: Atom::new_inline("Array"),
            ty: unit_ty(),
            span: Span::default(),
        }),
        args: vec![
            HirExpr::Int(1, Span::default()),
            HirExpr::Int(2, Span::default()),
            HirExpr::Int(3, Span::default()),
        ],
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "new Array(1, 2, 3) must return MirExpr::Local, got {mir:?}"
    );
    let has_create = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                ..
            }
        )
    });
    assert!(
        has_create,
        "new Array(1, 2, 3) must first emit ArrayCreate (empty vec), got: {out:?}"
    );
    let push_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        push_count, 3,
        "new Array(1, 2, 3) must emit 3 ArrayPush stmts, got: {out:?}"
    );
    let has_create_with_len = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !has_create_with_len,
        "new Array(1, 2, 3) must NOT emit ArrayCreateWithLen (multi-arg = items, not length), got: {out:?}"
    );
}

#[test]
fn array_from_call_emits_array_from_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &array_method_call_with_arg(
            "from",
            HirExpr::Local {
                id: LocalId::from_raw(13),
                ty: unit_ty(),
                span: Span::default(),
            },
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from(value) must return MirExpr::Local, got {mir:?}"
    );
    let from_args = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFrom,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert_eq!(
        from_args.map(Vec::len),
        Some(1),
        "Array.from(value) must pass exactly 1 arg (the source); got args={from_args:?}, full out: {out:?}"
    );
}

#[test]
fn array_from_with_non_global_mapfn_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = array_method_call_with_arg(
        "from",
        HirExpr::Local {
            id: LocalId::from_raw(14),
            ty: unit_ty(),
            span: Span::default(),
        },
    );
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Local {
            id: LocalId::from_raw(15),
            ty: unit_ty(),
            span: Span::default(),
        });
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from with a non-global mapFn must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from with non-global mapFn (Local id=14) must return MirExpr::Unit, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "Array.from with non-global mapFn (Local id=14) must NOT emit any MirStmt, got: {out:?}"
    );
}

#[test]
fn array_of_with_no_args_emits_array_create_only() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("of"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: Vec::new(),
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "Array.of() must return MirExpr::Local, got {mir:?}"
    );
    let push_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        push_count, 0,
        "Array.of() with no args must NOT emit ArrayPush, got: {out:?}"
    );
    let has_create = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                ..
            }
        )
    });
    assert!(has_create, "Array.of() must emit ArrayCreate, got: {out:?}");
}

#[test]
fn array_of_with_multi_args_emits_create_then_pushes() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("of"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![
            HirExpr::Int(10, Span::default()),
            HirExpr::Int(20, Span::default()),
            HirExpr::Int(30, Span::default()),
        ],
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "Array.of(10, 20, 30) must return MirExpr::Local, got {mir:?}"
    );
    let push_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        push_count, 3,
        "Array.of(10, 20, 30) must emit 3 ArrayPush stmts, got: {out:?}"
    );
}

#[test]
fn array_of_with_side_effect_args_does_not_reconvert_args() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("of"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Array"),
                    ty: unit_ty(),
                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("from"),
                ty: unit_ty(),
                span: Span::default(),
            })),
            args: vec![HirExpr::String(Atom::new_inline("abc"), Span::default())],
            ty: arr_ty,
            type_args: vec![],
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let from_string_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayFromString,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        from_string_count, 1,
        "Array.of(Array.from('abc')) must emit ArrayFromString exactly once (reconverting args would double side effects), got {from_string_count}: {out:?}"
    );
}

#[test]
fn array_from_string_literal_emits_array_from_string_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::String(Atom::new_inline("abc"), Span::default())],
        ty: arr_ty,
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
        matches!(mir, MirExpr::Local(_)),
        "Array.from('abc') must return MirExpr::Local, got {mir:?}"
    );
    let from_string_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFromString,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert!(
        from_string_stmt.is_some(),
        "Array.from('abc') must emit ArrayFromString, got: {out:?}"
    );
    let from_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayFrom,
                ..
            }
        )
    });
    assert!(
        !from_stmt,
        "Array.from('abc') must NOT emit ArrayFrom (string literal routes to ArrayFromString), got: {out:?}"
    );
}

#[test]
fn array_from_with_string_typed_variable_emits_array_from_string_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::Local {
            id: LocalId::from_raw(50),
            ty: string_ty,
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        0,
        "Array.from(<string-typed local>) must NOT emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from(<string-typed local>) must return MirExpr::Local, got {mir:?}"
    );
    let from_string_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFromString,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert!(
        from_string_stmt.is_some(),
        "Array.from(<string-typed local>) must emit ArrayFromString (inferred string type), got: {out:?}"
    );
    let from_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayFrom,
                ..
            }
        )
    });
    assert!(
        !from_stmt,
        "Array.from(<string-typed local>) must NOT emit ArrayFrom (string-typed source routes to ArrayFromString), got: {out:?}"
    );
}

#[test]
fn array_from_with_object_literal_length_emits_array_create_with_len() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::new_inline("length"),
                value: HirExpr::Int(3, Span::default()),
            }],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        0,
        "Array.from({{length: 3}}) must NOT trigger E0402 (object literals are detected pre-conversion), got {:?}",
        cx.diagnostics()
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        0,
        "Array.from({{length: 3}}) must NOT emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from({{length: 3}}) must return MirExpr::Local, got {mir:?}"
    );
    let with_len_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayCreateWithLen,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert!(
        with_len_stmt.is_some(),
        "Array.from({{length: 3}}) must emit ArrayCreateWithLen, got: {out:?}"
    );
    let len_arg = with_len_stmt.and_then(|args| args.first());
    if let Some(MirExpr::Int { value, .. }) = len_arg {
        assert_eq!(
            *value, 3,
            "Array.from({{length: 3}}) must pass 3 to ArrayCreateWithLen, got {value}"
        );
    } else {
        panic!(
            "Array.from({{length: 3}}) must pass MirExpr::Int(3) as the length arg, got {len_arg:?}"
        );
    }
}

#[test]
fn parse_array_index_rejects_oversized_indices_at_u32_max_boundary() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![
                ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(0, Span::default()),
                },
                ObjectLiteralField::Property {
                    name: Atom::new_inline("4294967295"),
                    value: HirExpr::Int(0, Span::default()),
                },
            ],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let with_len_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !with_len_stmt,
        "Array.from({{length: 0, 4294967295: 0}}) (idx == u32::MAX) must NOT emit ArrayCreateWithLen \
         (4294967295 is not a valid ES array index — max is 2^32 - 2 = 4294967294), got: {out:?}"
    );
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        1,
        "Array.from({{length: 0, 4294967295: 0}}) (oversized index) must fall through to E0402, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn parse_array_index_accepts_max_valid_es_array_index() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![
                ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(0, Span::default()),
                },
                ObjectLiteralField::Property {
                    name: Atom::new_inline("4294967294"),
                    value: HirExpr::Int(0, Span::default()),
                },
            ],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let with_len_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        with_len_stmt,
        "Array.from({{length: 0, 4294967294: 0}}) (idx == u32::MAX - 1, the max valid ES array index) \
         must emit ArrayCreateWithLen, got: {out:?}"
    );
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        0,
        "Array.from({{length: 0, 4294967294: 0}}) (max valid index) must NOT trigger E0402, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn array_from_with_object_literal_length_and_extra_field_falls_through_to_e0402() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![
                ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(3, Span::default()),
                },
                ObjectLiteralField::Property {
                    name: Atom::new_inline("foo"),
                    value: HirExpr::Int(0, Span::default()),
                },
            ],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        1,
        "Array.from({{length: 3, foo: 0}}) (extra fields) must fall through and trigger E0402 \
         (only the bare {{length: N}} pattern is supported in AOT), got {:?}",
        cx.diagnostics()
    );
    let with_len_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !with_len_stmt,
        "Array.from({{length: 3, foo: 0}}) must NOT emit ArrayCreateWithLen (only single-length pattern), got: {out:?}"
    );
}

#[test]
fn array_from_with_object_literal_non_int_length_falls_through_to_e0402() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::new_inline("length"),
                value: HirExpr::Local {
                    id: LocalId::from_raw(99),
                    ty: unit_ty(),
                    span: Span::default(),
                },
            }],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let with_len_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !with_len_stmt,
        "Array.from({{length: <local>}}) must NOT emit ArrayCreateWithLen (only literal int length), got: {out:?}"
    );
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        1,
        "Array.from({{length: <local>}}) must fall through to E0402, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn array_from_with_object_literal_negative_int_length_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::new_inline("length"),
                value: HirExpr::Int(-1, Span::default()),
            }],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let with_len_stmt = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                ..
            }
        )
    });
    assert!(
        !with_len_stmt,
        "Array.from({{length: -1}}) (negative literal length) must NOT emit ArrayCreateWithLen (validation rejects upfront), got: {out:?}"
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from({{length: -1}}) (negative literal length) must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from({{length: -1}}) must return MirExpr::Unit on E0406, got {mir:?}"
    );
}

#[test]
fn array_from_with_object_literal_length_and_indexed_values_emits_create_then_sets() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![
                ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(3, Span::default()),
                },
                ObjectLiteralField::Property {
                    name: Atom::new_inline("0"),
                    value: HirExpr::Int(10, Span::default()),
                },
                ObjectLiteralField::Property {
                    name: Atom::new_inline("1"),
                    value: HirExpr::Int(20, Span::default()),
                },
            ],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        0,
        "Array.from({{length: 3, 0: 10, 1: 20}}) must NOT trigger E0402, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from({{length: 3, 0: 10, 1: 20}}) must return MirExpr::Local, got {mir:?}"
    );
    let create_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayCreateWithLen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        create_count, 1,
        "Array.from({{length: 3, ...}}) must emit exactly 1 ArrayCreateWithLen, got: {out:?}"
    );
    let set_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArraySet,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        set_count, 2,
        "Array.from({{length: 3, 0: 10, 1: 20}}) must emit 2 ArraySet ops, got: {out:?}"
    );
}

#[test]
fn array_from_with_object_literal_length_and_global_mapfn_emits_array_from_length_mapped() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![
            HirExpr::ObjectLiteral {
                fields: vec![ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(3, Span::default()),
                }],
                ty: unit_ty(),
                span: Span::default(),
            },
            HirExpr::Global {
                name: Atom::new_inline("doubleIndex"),
                ty: unit_ty(),
                span: Span::default(),
            },
        ],
        ty: arr_ty,
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
    let e0402_count = diag_count(cx.diagnostics(), "E0402");
    assert_eq!(
        e0402_count,
        0,
        "Array.from({{length: 3}}, doubleIndex) must NOT trigger E0402, got {:?}",
        cx.diagnostics()
    );
    let w0001_count = diag_count(cx.diagnostics(), "W0001");
    assert_eq!(
        w0001_count,
        0,
        "Array.from({{length: 3}}, doubleIndex) (2 args, no thisArg) must NOT emit W0001, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from({{length: 3}}, doubleIndex) must return MirExpr::Local, got {mir:?}"
    );
    let length_mapped = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFromLengthMapped,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert!(
        length_mapped.is_some(),
        "Array.from({{length: 3}}, doubleIndex) must emit ArrayFromLengthMapped, got: {out:?}"
    );
    let mapped_args = length_mapped.expect("checked is_some above");
    assert_eq!(
        mapped_args.len(),
        2,
        "ArrayFromLengthMapped must take 2 args (len, mapfn), got {mapped_args:?}"
    );
    if let MirExpr::Int { value, .. } = &mapped_args[0] {
        assert_eq!(*value, 3, "first arg must be length 3, got {value}");
    } else {
        panic!("first arg must be MirExpr::Int, got {:?}", mapped_args[0]);
    }
}

#[test]
fn array_from_with_object_literal_length_and_indexed_values_and_mapfn_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![
            HirExpr::ObjectLiteral {
                fields: vec![
                    ObjectLiteralField::Property {
                        name: Atom::new_inline("length"),
                        value: HirExpr::Int(3, Span::default()),
                    },
                    ObjectLiteralField::Property {
                        name: Atom::new_inline("0"),
                        value: HirExpr::Int(10, Span::default()),
                    },
                ],
                ty: unit_ty(),
                span: Span::default(),
            },
            HirExpr::Global {
                name: Atom::new_inline("doubleIndex"),
                ty: unit_ty(),
                span: Span::default(),
            },
        ],
        ty: arr_ty,
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
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from({{length: 3, 0: 10}}, doubleIndex) (mixing indexed values with mapFn) must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from({{length: 3, 0: 10}}, doubleIndex) must return MirExpr::Unit on E0406, got {mir:?}"
    );
    let length_mapped = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ArrayFromLengthMapped,
                ..
            }
        )
    });
    assert!(
        !length_mapped,
        "Array.from({{length: 3, 0: 10}}, doubleIndex) must NOT emit ArrayFromLengthMapped (mixing form is rejected), got: {out:?}"
    );
    let set_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArraySet,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        set_count, 0,
        "Array.from({{length: 3, 0: 10}}, doubleIndex) must NOT emit ArraySet (the source array is rejected, no allocation), got: {out:?}"
    );
}

#[test]
fn array_from_with_object_literal_length_exceeding_max_dense_array_len_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let over_cap = i64::from(ts_aot_core::MAX_DENSE_ARRAY_LEN) + 1;
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![HirExpr::ObjectLiteral {
            fields: vec![ObjectLiteralField::Property {
                name: Atom::new_inline("length"),
                value: HirExpr::Int(over_cap, Span::default()),
            }],
            ty: unit_ty(),
            span: Span::default(),
        }],
        ty: arr_ty,
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
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from({{length: {over_cap}}}) must emit E0406 (exceeds AOT dense-Vec cap of {}), got {:?}",
        ts_aot_core::MAX_DENSE_ARRAY_LEN,
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from({{length: {over_cap}}}) must return MirExpr::Unit on E0406, got {mir:?}"
    );
    let create_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayCreateWithLen,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        create_count, 0,
        "Array.from({{length: {over_cap}}}) must NOT emit ArrayCreateWithLen (validation rejects upfront), got: {out:?}"
    );
}

#[test]
fn array_from_with_object_literal_length_and_global_mapfn_and_thisarg_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![
            HirExpr::ObjectLiteral {
                fields: vec![ObjectLiteralField::Property {
                    name: Atom::new_inline("length"),
                    value: HirExpr::Int(3, Span::default()),
                }],
                ty: unit_ty(),
                span: Span::default(),
            },
            HirExpr::Global {
                name: Atom::new_inline("doubleIndex"),
                ty: unit_ty(),
                span: Span::default(),
            },
            HirExpr::Local {
                id: LocalId::from_raw(99),
                ty: unit_ty(),
                span: Span::default(),
            },
        ],
        ty: arr_ty,
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
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from({{length: 3}}, mapFn, thisArg) (3 args) must emit E0406 (thisArg not supported in AOT), got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from({{length: 3}}, mapFn, thisArg) must return MirExpr::Unit on E0406, got {mir:?}"
    );
    let any_runtime = out.iter().any(|s| matches!(s, MirStmt::Runtime { .. }));
    assert!(
        !any_runtime,
        "Array.from({{length: 3}}, mapFn, thisArg) must NOT emit any runtime stmt (rejected at lowering), got: {out:?}"
    );
}

#[test]
fn array_from_with_global_function_mapfn_emits_array_from_mapped_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = array_method_call_with_arg(
        "from",
        HirExpr::Local {
            id: LocalId::from_raw(16),
            ty: unit_ty(),
            span: Span::default(),
        },
    );
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Global {
            name: Atom::new_inline("doubleIt"),
            ty: unit_ty(),
            span: Span::default(),
        });
    }
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    if let HirExpr::Call { ty, .. } = &mut expr {
        *ty = arr_ty;
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        0,
        "Array.from(arr, doubleIt) with global mapFn must NOT emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from(arr, doubleIt) must return MirExpr::Local, got {mir:?}"
    );
    let mapped_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFromMapped,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert_eq!(
        mapped_stmt.map(Vec::len),
        Some(2),
        "Array.from(arr, doubleIt) must emit ArrayFromMapped with 2 args, got: {out:?}"
    );
}

#[test]
fn array_from_with_lifted_closure_mapfn_emits_array_from_mapped() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = array_method_call_with_arg(
        "from",
        HirExpr::Local {
            id: LocalId::from_raw(19),
            ty: unit_ty(),
            span: Span::default(),
        },
    );
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Global {
            name: Atom::new_inline("__ts_aot_closure_0"),
            ty: unit_ty(),
            span: Span::default(),
        });
    }
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    if let HirExpr::Call { ty, .. } = &mut expr {
        *ty = arr_ty;
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        0,
        "Array.from(arr, __ts_aot_closure_0) (a closure lifted by `lower_closures`) \
         must NOT emit E0406 — closures ARE supported, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Array.from(arr, __ts_aot_closure_0) must return MirExpr::Local, got {mir:?}"
    );
    let mapped_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ArrayFromMapped,
            args,
            ..
        } = s
        {
            Some(args)
        } else {
            None
        }
    });
    assert_eq!(
        mapped_stmt.map(Vec::len),
        Some(2),
        "Array.from(arr, __ts_aot_closure_0) must emit ArrayFromMapped with 2 args, got: {out:?}"
    );
}

#[test]
fn array_from_with_no_args_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Array"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("from"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: Vec::new(),
        ty: arr_ty,
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
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from() with no args must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from() with no args must return MirExpr::Unit (rejected call produces no value), got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "Array.from() with no args must NOT emit any MirStmt (rejected before lowering), got: {out:?}"
    );
}

#[test]
fn array_from_with_three_args_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = array_method_call_with_arg(
        "from",
        HirExpr::Local {
            id: LocalId::from_raw(17),
            ty: unit_ty(),
            span: Span::default(),
        },
    );
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Global {
            name: Atom::new_inline("mapFn"),
            ty: unit_ty(),
            span: Span::default(),
        });
        args.push(HirExpr::Local {
            id: LocalId::from_raw(18),
            ty: unit_ty(),
            span: Span::default(),
        });
    }
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    if let HirExpr::Call { ty, .. } = &mut expr {
        *ty = arr_ty;
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from(arr, mapFn, thisArg) with 3 args must emit E0406 (thisArg not supported in AOT), got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "Array.from(arr, mapFn, thisArg) must return MirExpr::Unit on E0406, got {mir:?}"
    );
    let any_runtime = out.iter().any(|s| matches!(s, MirStmt::Runtime { .. }));
    assert!(
        !any_runtime,
        "Array.from(arr, mapFn, thisArg) must NOT emit any runtime stmt (rejected at lowering), got: {out:?}"
    );
}

#[test]
fn array_from_with_three_args_and_non_global_mapfn_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = array_method_call_with_arg(
        "from",
        HirExpr::Local {
            id: LocalId::from_raw(20),
            ty: unit_ty(),
            span: Span::default(),
        },
    );
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Local {
            id: LocalId::from_raw(21),
            ty: unit_ty(),
            span: Span::default(),
        });
        args.push(HirExpr::Local {
            id: LocalId::from_raw(22),
            ty: unit_ty(),
            span: Span::default(),
        });
    }
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    if let HirExpr::Call { ty, .. } = &mut expr {
        *ty = arr_ty;
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let e0406_count = diag_count(cx.diagnostics(), "E0406");
    assert_eq!(
        e0406_count,
        1,
        "Array.from(arr, localVar, thisArg) with non-global mapFn must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(matches!(mir, MirExpr::Unit));
    assert!(out.is_empty());
}
