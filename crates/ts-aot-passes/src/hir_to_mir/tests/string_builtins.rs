use super::common::*;

#[test]
fn string_from_char_code_call_emits_string_from_char_code_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &string_method_call_with_args(
            "fromCharCode",
            vec![
                HirExpr::Int(65, Span::default()),
                HirExpr::Int(66, Span::default()),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        cx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0404" || d.code.as_str() == "E0406")
            .count(),
        0,
        "String.fromCharCode must not emit E0404/E0406, got {:?}",
        cx.diagnostics()
    );
    let stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::StringFromCharCode,
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
        stmt.is_some(),
        "String.fromCharCode(65, 66) must emit StringFromCharCode, got: {out:?}"
    );
    assert_eq!(
        stmt.expect("checked is_some above").len(),
        2,
        "String.fromCharCode(65, 66) must pass all args variadically, got {stmt:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "String.fromCharCode must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn string_from_code_point_call_emits_string_from_code_point_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &string_method_call_with_args(
            "fromCodePoint",
            vec![HirExpr::Int(0x1_F600, Span::default())],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        cx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0404" || d.code.as_str() == "E0406")
            .count(),
        0,
        "String.fromCodePoint must not emit E0404/E0406, got {:?}",
        cx.diagnostics()
    );
    let stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::StringFromCodePoint,
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
        stmt.is_some(),
        "String.fromCodePoint(0x1F600) must emit StringFromCodePoint, got: {out:?}"
    );
    assert_eq!(
        stmt.expect("checked is_some above").len(),
        1,
        "String.fromCodePoint(0x1F600) must pass 1 arg, got {stmt:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "String.fromCodePoint must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn string_index_of_call_emits_string_index_of_op_with_implicit_receiver() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let mir = c.convert_expr(
        &string_instance_method_call_with_args(
            HirExpr::Local {
                id: LocalId::from_raw(20),
                ty: str_ty,
                span: Span::default(),
            },
            "indexOf",
            vec![
                HirExpr::String(Atom::new_inline("ell"), Span::default()),
                HirExpr::Int(0, Span::default()),
            ],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        cx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0404" || d.code.as_str() == "E0406")
            .count(),
        0,
        "\"hello\".indexOf(\"ell\", 0) must not emit E0404/E0406, got {:?}",
        cx.diagnostics()
    );
    let stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::StringIndexOf,
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
        stmt.is_some(),
        "\"hello\".indexOf(\"ell\", 0) must emit StringIndexOf, got: {out:?}"
    );
    assert_eq!(
        stmt.expect("checked is_some above").len(),
        3,
        "StringIndexOf must pass receiver + 2 args (haystack, needle, fromIndex), got {stmt:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "String.prototype.indexOf must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn string_char_at_call_emits_string_char_at_op_with_implicit_receiver() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let mir = c.convert_expr(
        &string_instance_method_call_with_args(
            HirExpr::Local {
                id: LocalId::from_raw(21),
                ty: str_ty,
                span: Span::default(),
            },
            "charAt",
            vec![HirExpr::Int(0, Span::default())],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        cx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0404" || d.code.as_str() == "E0406")
            .count(),
        0,
        "\"hello\".charAt(0) must not emit E0404/E0406, got {:?}",
        cx.diagnostics()
    );
    let stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::StringCharAt,
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
        stmt.is_some(),
        "\"hello\".charAt(0) must emit StringCharAt, got: {out:?}"
    );
    assert_eq!(
        stmt.expect("checked is_some above").len(),
        2,
        "StringCharAt must pass receiver + 1 arg (string, idx), got {stmt:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "String.prototype.charAt must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn string_index_of_call_with_wrong_arity_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let mut expr = string_instance_method_call_with_args(
        HirExpr::Local {
            id: LocalId::from_raw(22),
            ty: str_ty,
            span: Span::default(),
        },
        "indexOf",
        vec![
            HirExpr::String(Atom::new_inline("a"), Span::default()),
            HirExpr::String(Atom::new_inline("b"), Span::default()),
            HirExpr::String(Atom::new_inline("c"), Span::default()),
        ],
    );
    if let HirExpr::Call { ty, .. } = &mut expr {
        *ty = unit_ty();
    }
    c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
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
        "\"s\".indexOf(\"a\", \"b\", \"c\") (3 args, max 2) must emit E0406, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn string_index_of_call_with_one_arg_emits_op_with_default_from_index_zero() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let mir = c.convert_expr(
        &string_instance_method_call_with_args(
            HirExpr::Local {
                id: LocalId::from_raw(23),
                ty: str_ty,
                span: Span::default(),
            },
            "indexOf",
            vec![HirExpr::String(Atom::new_inline("ell"), Span::default())],
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert_eq!(
        cx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "E0404" || d.code.as_str() == "E0406")
            .count(),
        0,
        "\"hello\".indexOf(\"ell\") (1 arg) must be accepted (fromIndex optional, defaults to 0), got {:?}",
        cx.diagnostics()
    );
    let stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::StringIndexOf,
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
        stmt.is_some(),
        "\"hello\".indexOf(\"ell\") must emit StringIndexOf, got: {out:?}"
    );
    assert_eq!(
        stmt.expect("checked is_some above").len(),
        3,
        "StringIndexOf must pass receiver + needle + default fromIndex=0, got {stmt:?}"
    );
    let args = stmt.expect("checked is_some above");
    assert!(
        matches!(args[2], MirExpr::Int { value: 0, .. }),
        "fromIndex=0 must be synthesized at position 2 (after receiver + needle), got {:?}",
        args[2]
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn string_index_of_call_evaluates_receiver_before_arguments() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let mut receiver =
        string_method_call_with_args("fromCharCode", vec![HirExpr::Int(65, Span::default())]);
    if let HirExpr::Call { ty, .. } = &mut receiver {
        *ty = str_ty;
    }
    let arg = HirExpr::Local {
        id: LocalId::from_raw(50),
        ty: str_ty,
        span: Span::default(),
    };
    let expr = string_instance_method_call_with_args(receiver, "indexOf", vec![arg]);
    c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let receiver_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringFromCharCode,
                ..
            }
        )
    });
    let indexof_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringIndexOf,
                ..
            }
        )
    });
    let receiver_idx = receiver_idx.expect("receiver call must emit StringFromCharCode runtime op");
    let indexof_idx =
        indexof_idx.expect("String.prototype.indexOf must emit StringIndexOf runtime op");
    assert!(
        receiver_idx < indexof_idx,
        "JS evaluation order: receiver must be converted (and its statement emitted) BEFORE arguments. \
         receiver StringFromCharCode at index {receiver_idx}, StringIndexOf at index {indexof_idx}, out: {out:?}"
    );
}
