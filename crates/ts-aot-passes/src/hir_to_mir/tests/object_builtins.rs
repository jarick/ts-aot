use super::common::*;

#[test]
fn resolve_field_id_call_owner_with_registered_struct_id_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let call_ret_ty = TypeId::from_raw(91);
    let sid = ts_aot_core::StructId::from_raw(0);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(call_ret_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("answer");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(42));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: call_ret_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(42),
        "Call-owner with a registered struct id must resolve the field id by looking up (sid, field_name); got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "a registered struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_non_typed_owner_emits_p0011() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    c.set_field_id_lookup(HashMap::new());

    let owner = HirExpr::Int(0, Span::default());
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(99),
        "non-typed owner must fall back to placeholder after emitting P0011"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0011"),
        "non-typed owner (Int) must surface P0011; got {:?}",
        cx.diagnostics()
    );
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "P0012 must not be reported when the failure is the owner type, not the missing struct id"
    );
}

#[test]
fn resolve_field_id_type_assertion_owner_with_registered_target_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let target_ty = TypeId::from_raw(101);
    let sid = ts_aot_core::StructId::from_raw(1);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(target_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("tag");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(7));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::TypeAssertion {
        expr: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),

            span: Span::default(),
        }),
        target: target_ty,

        span: Span::default(),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(7),
        "(obj as T).field must resolve via TypeAssertion's target type when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "TypeAssertion owner with registered target struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_type_assertion_owner_without_registered_target_emits_p0012() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    c.set_field_id_lookup(HashMap::new());

    let owner = HirExpr::TypeAssertion {
        expr: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),

            span: Span::default(),
        }),
        target: TypeId::from_raw(202),

        span: Span::default(),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(99),
        "TypeAssertion owner with unregistered target must fall back to placeholder after emitting P0012"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "TypeAssertion owner whose target type has no registered struct id must surface P0012, not P0011; got {:?}",
        cx.diagnostics()
    );
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0011"),
        "P0011 must not be reported when the owner is typed but the target struct id is missing"
    );
}

#[test]
fn resolve_field_id_assignment_owner_with_registered_ty_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let assign_ty = TypeId::from_raw(103);
    let sid = ts_aot_core::StructId::from_raw(2);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(assign_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("payload");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(13));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::Assignment {
        target: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),

            span: Span::default(),
        }),
        value: Box::new(HirExpr::Local {
            id: LocalId::from_raw(1),
            ty: assign_ty,

            span: Span::default(),
        }),
        ty: assign_ty,

        span: Span::default(),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(13),
        "(obj = makeC()).field must resolve via Assignment's ty when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "Assignment owner with registered ty struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_compound_update_owner_with_registered_ty_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let target_ty = TypeId::from_raw(104);
    let sid = ts_aot_core::StructId::from_raw(3);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(target_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("count");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(21));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::CompoundUpdate {
        target: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: target_ty,

            span: Span::default(),
        }),
        op: HirBinaryOp::Add,
        rhs: Box::new(HirExpr::Int(1, Span::default())),
        post: false,
        ty: target_ty,
        span: Span::default(),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(21),
        "(obj += 1).field must resolve via CompoundUpdate's ty when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "CompoundUpdate owner with registered ty struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn object_keys_call_emits_object_keys_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call_with_arg(
            "keys",
            HirExpr::Local {
                id: LocalId::from_raw(7),
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
        "Object.keys(map) must return a Local, got {mir:?}"
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "Object.keys(map) must not emit E0404, got {:?}",
        cx.diagnostics()
    );
    let has_object_keys = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::ObjectKeys,
                ..
            }
        )
    });
    assert!(
        has_object_keys,
        "Object.keys(map) must emit MirStmt::Runtime {{ op: ObjectKeys, .. }}, got: {out:?}"
    );
    let object_keys_args = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::ObjectKeys,
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
        object_keys_args.map(Vec::len),
        Some(1),
        "Object.keys(map) must pass exactly 1 arg to __ts_aot_object_keys \
         (the map, no implicit receiver); got args={object_keys_args:?}, full out: {out:?}"
    );
}

#[test]
fn object_get_prototype_of_call_emits_unsupported_builtin_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &object_method_call_with_arg(
            "getPrototypeOf",
            HirExpr::Local {
                id: LocalId::from_raw(7),
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
    let has_runtime = out.iter().any(|s| matches!(s, MirStmt::Runtime { .. }));
    assert!(
        !has_runtime,
        "Object.getPrototypeOf(map) must NOT lower to a runtime op (no prototype chain in AOT); got: {out:?}"
    );
    let e0406 = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406,
        1,
        "Object.getPrototypeOf(map) must emit exactly one E0406 unsupported-builtin diagnostic, got: {:?}",
        cx.diagnostics()
    );
    assert!(
        cx.diagnostics()
            .iter()
            .any(|d| d.message.contains("getPrototypeOf is not supported")),
        "E0406 message must mention Object.getPrototypeOf unavailability, got: {:?}",
        cx.diagnostics()
    );
}

#[test]
fn object_set_prototype_of_call_inlines_assign_no_runtime_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call_with_args(
            "setPrototypeOf",
            vec![
                HirExpr::Int(0, Span::default()),
                HirExpr::Int(1, Span::default()),
            ],
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
        "Object.setPrototypeOf(target, proto) must not emit E0404, got {:?}",
        cx.diagnostics()
    );
    let has_runtime = out.iter().any(|s| matches!(s, MirStmt::Runtime { .. }));
    assert!(
        !has_runtime,
        "Object.setPrototypeOf(target, proto) is a no-op in AOT; must inline via MirStmt::Assign, \
         no MirStmt::Runtime, got: {out:?}"
    );
    let has_assign = out.iter().any(|s| matches!(s, MirStmt::Assign { .. }));
    assert!(
        has_assign,
        "Object.setPrototypeOf(target, proto) must emit MirStmt::Assign copying the first arg to dest, got: {out:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Object.setPrototypeOf(target, proto) must return a Local, got {mir:?}"
    );
}

#[test]
fn receiver_has_own_property_returns_true_for_known_struct_field() {
    let mut types = TypeTable::new();
    let receiver_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline("Point"),
    });
    let struct_id = StructId::from_raw(7);
    let mut c = ExprConverter::new();
    c.struct_ids.insert(receiver_ty, struct_id);
    c.field_id_lookup
        .insert((struct_id, Atom::new_inline("x")), FieldId::from_raw(0));
    c.field_id_lookup
        .insert((struct_id, Atom::new_inline("y")), FieldId::from_raw(1));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &receiver_has_own_property_call(&mut types, receiver_ty, "x"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Bool(true)),
        "obj.hasOwnProperty('x') on Point{{x,y}} must lower to MirExpr::Bool(true), got {mir:?}"
    );
}

#[test]
fn receiver_has_own_property_returns_false_for_unknown_struct_field() {
    let mut types = TypeTable::new();
    let receiver_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline("Point"),
    });
    let struct_id = StructId::from_raw(8);
    let mut c = ExprConverter::new();
    c.struct_ids.insert(receiver_ty, struct_id);
    c.field_id_lookup
        .insert((struct_id, Atom::new_inline("x")), FieldId::from_raw(0));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &receiver_has_own_property_call(&mut types, receiver_ty, "missing"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Bool(false)),
        "obj.hasOwnProperty('missing') on Point{{x}} must lower to MirExpr::Bool(false), got {mir:?}"
    );
}

#[test]
fn receiver_has_own_property_falls_through_to_indirect_call_for_non_struct_receiver() {
    let mut types = TypeTable::new();
    let receiver_ty = types.intern(&Type::I64);
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &receiver_has_own_property_call(&mut types, receiver_ty, "x"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        0,
        "non-struct receiver must NOT trigger hasOwnProperty special-case (E0406), \
         must fall through to indirect-call; got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::IndirectCall { .. }),
        "non-struct receiver hasOwnProperty call must lower to MirExpr::IndirectCall, got {mir:?}"
    );
}

#[test]
fn receiver_has_own_property_emits_e0406_for_dynamic_key() {
    let mut types = TypeTable::new();
    let receiver_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline("Point"),
    });
    let struct_id = StructId::from_raw(9);
    let mut c = ExprConverter::new();
    c.struct_ids.insert(receiver_ty, struct_id);
    c.field_id_lookup
        .insert((struct_id, Atom::new_inline("x")), FieldId::from_raw(0));
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: receiver_ty,

                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("hasOwnProperty"),
            ty: types.intern(&Type::Bool),

            span: Span::default(),
        })),
        args: vec![HirExpr::Local {
            id: LocalId::from_raw(1),
            ty: types.intern(&Type::String),

            span: Span::default(),
        }],
        ty: types.intern(&Type::Bool),
        type_args: vec![],

        span: Span::default(),
    };
    let out = &mut Vec::new();
    let mut cx = ctx();
    let _mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        1,
        "dynamic (non-literal) key must emit E0406, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn bare_has_own_property_field_access_emits_e0407() {
    let mut types = TypeTable::new();
    let receiver_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline("Point"),
    });
    let struct_id = StructId::from_raw(10);
    let mut c = ExprConverter::new();
    c.struct_ids.insert(receiver_ty, struct_id);
    let expr = HirExpr::Field {
        owner: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: receiver_ty,
            span: Span::default(),
        }),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("hasOwnProperty"),
        ty: types.intern(&Type::Bool),
        span: Span::default(),
    };
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0407_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0407")
        .count();
    assert_eq!(
        e0407_count,
        1,
        "bare `obj.hasOwnProperty` (no call) must emit E0407, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_not_emitted_for_local_receiver_keys_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &local_method_call("keys"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "myMap.keys() with local receiver must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_not_emitted_for_local_receiver_user_defined_method() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &local_method_call("getPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "user-defined `.getPrototypeOf` on local receiver must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_not_emitted_for_object_local_global_other_method() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &object_method_call("assign"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        diag_count(cx.diagnostics(), "E0404"),
        0,
        "Object.assign() (not in banned set) must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn object_set_prototype_of_no_args_emits_e0406_no_panic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call("setPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        1,
        "Object.setPrototypeOf() with no args must emit E0406 (not panic), got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "must return MirExpr::Unit on arity error, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "no stmts should be emitted on arity error, got: {out:?}"
    );
}

#[test]
fn object_keys_no_args_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call("keys"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        1,
        "Object.keys() with no args must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(matches!(mir, MirExpr::Unit));
    assert!(out.is_empty());
}

#[test]
fn object_keys_too_many_args_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut expr = object_method_call_with_arg("keys", HirExpr::Int(1, Span::default()));
    if let HirExpr::Call { args, .. } = &mut expr {
        args.push(HirExpr::Int(2, Span::default()));
    }
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        1,
        "Object.keys(a, b) with 2 args must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(matches!(mir, MirExpr::Unit));
    assert!(out.is_empty());
}

#[test]
fn object_get_prototype_of_no_args_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call("getPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .count();
    assert_eq!(
        e0406_count,
        1,
        "Object.getPrototypeOf() with no args must emit E0406, got {:?}",
        cx.diagnostics()
    );
    assert!(matches!(mir, MirExpr::Unit));
    assert!(out.is_empty());
}
