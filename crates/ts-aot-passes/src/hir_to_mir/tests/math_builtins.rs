use super::common::*;

#[test]
fn math_unary_call_emits_math_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &math_method_call_with_arg("floor", HirExpr::Float(3.7_f64.to_bits(), Span::default())),
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
        "Math.floor(3.7) must not emit E0406, got {:?}",
        cx.diagnostics()
    );
    let floor_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::MathFloor,
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
        floor_stmt.is_some(),
        "Math.floor(3.7) must emit MathFloor, got: {out:?}"
    );
    assert_eq!(
        floor_stmt.expect("checked is_some above").len(),
        1,
        "Math.floor(3.7) must pass exactly 1 arg, got {floor_stmt:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "Math.floor(3.7) must return MirExpr::Local, got {mir:?}"
    );
}

#[test]
fn math_binary_call_emits_math_runtime_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let _mir = c.convert_expr(
        &math_method_call_with_2_args(
            "pow",
            HirExpr::Float(2.0_f64.to_bits(), Span::default()),
            HirExpr::Float(10.0_f64.to_bits(), Span::default()),
        ),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let pow_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::MathPow,
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
        pow_stmt.is_some(),
        "Math.pow(2, 10) must emit MathPow, got: {out:?}"
    );
    assert_eq!(
        pow_stmt.expect("checked is_some above").len(),
        2,
        "Math.pow(2, 10) must pass exactly 2 args, got {pow_stmt:?}"
    );
}

#[test]
fn math_random_call_emits_math_random_with_zero_args() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Math"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("random"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![],
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let random_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::MathRandom,
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
        random_stmt.is_some(),
        "Math.random() must emit MathRandom, got: {out:?}"
    );
    assert!(
        random_stmt.expect("checked is_some above").is_empty(),
        "Math.random() must take 0 args"
    );
}

#[test]
fn math_max_with_three_args_emits_math_max_with_three_mir_args() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = math_method_call_with_2_args(
        "max",
        HirExpr::Float(1.0_f64.to_bits(), Span::default()),
        HirExpr::Float(2.0_f64.to_bits(), Span::default()),
    );
    let expr3 = if let HirExpr::Call { mut args, .. } = expr {
        args.push(HirExpr::Float(3.0_f64.to_bits(), Span::default()));
        args
    } else {
        unreachable!()
    };
    let call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Math"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("max"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: expr3,
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &call,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let max_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::MathMax,
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
        max_stmt.is_some(),
        "Math.max(1, 2, 3) must emit MathMax, got: {out:?}"
    );
    assert_eq!(
        max_stmt.expect("checked is_some above").len(),
        3,
        "Math.max(1, 2, 3) must pass all 3 args (variadic), got {max_stmt:?}"
    );
}

#[test]
fn math_max_with_zero_args_emits_math_max_with_empty_args() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Math"),
                ty: unit_ty(),
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("max"),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args: vec![],
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    let _mir = c.convert_expr(
        &call,
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
        "Math.max() (0 args) must be accepted (variadic), got {:?}",
        cx.diagnostics()
    );
    let max_stmt = out.iter().find_map(|s| {
        if let MirStmt::Runtime {
            op: RuntimeOp::MathMax,
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
        max_stmt.is_some(),
        "Math.max() must emit MathMax, got: {out:?}"
    );
    assert!(
        max_stmt.expect("checked is_some above").is_empty(),
        "Math.max() must pass 0 args"
    );
}

#[test]
fn math_call_with_wrong_arity_emits_e0406() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let _mir = c.convert_expr(
        &math_method_call_with_2_args(
            "floor",
            HirExpr::Float(1.0_f64.to_bits(), Span::default()),
            HirExpr::Float(2.0_f64.to_bits(), Span::default()),
        ),
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
        "Math.floor(1.0, 2.0) (2 args, expected 1) must emit E0406, got {:?}",
        cx.diagnostics()
    );
}
