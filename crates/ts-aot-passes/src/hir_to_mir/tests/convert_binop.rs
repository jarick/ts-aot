use super::common::*;

#[test]
fn convert_binop_maps_all_variants() {
    let mut cx = ctx();
    assert_eq!(convert_binop(HirBinaryOp::Add, &mut cx), BinaryOp::Add);
    assert_eq!(convert_binop(HirBinaryOp::Sub, &mut cx), BinaryOp::Sub);
    assert_eq!(convert_binop(HirBinaryOp::Mul, &mut cx), BinaryOp::Mul);
    assert_eq!(convert_binop(HirBinaryOp::Div, &mut cx), BinaryOp::Div);
    assert_eq!(convert_binop(HirBinaryOp::Mod, &mut cx), BinaryOp::Mod);
    assert_eq!(convert_binop(HirBinaryOp::Eq, &mut cx), BinaryOp::Eq);
    assert_eq!(convert_binop(HirBinaryOp::Ne, &mut cx), BinaryOp::Ne);
    assert_eq!(convert_binop(HirBinaryOp::Lt, &mut cx), BinaryOp::Lt);
    assert_eq!(convert_binop(HirBinaryOp::Le, &mut cx), BinaryOp::Le);
    assert_eq!(convert_binop(HirBinaryOp::Gt, &mut cx), BinaryOp::Gt);
    assert_eq!(convert_binop(HirBinaryOp::Ge, &mut cx), BinaryOp::Ge);
    assert_eq!(convert_binop(HirBinaryOp::And, &mut cx), BinaryOp::And);
    assert_eq!(convert_binop(HirBinaryOp::Or, &mut cx), BinaryOp::Or);
    assert_eq!(
        convert_binop(HirBinaryOp::BitAnd, &mut cx),
        BinaryOp::BitAnd
    );
    assert_eq!(convert_binop(HirBinaryOp::BitOr, &mut cx), BinaryOp::BitOr);
    assert_eq!(
        convert_binop(HirBinaryOp::BitXor, &mut cx),
        BinaryOp::BitXor
    );
    assert_eq!(convert_binop(HirBinaryOp::Shl, &mut cx), BinaryOp::Shl);
    assert_eq!(convert_binop(HirBinaryOp::Shr, &mut cx), BinaryOp::Shr);
    assert_eq!(convert_binop(HirBinaryOp::Usr, &mut cx), BinaryOp::Eq);
    assert_eq!(convert_binop(HirBinaryOp::In, &mut cx), BinaryOp::Eq);
    assert_eq!(
        convert_binop(HirBinaryOp::InstanceOf, &mut cx),
        BinaryOp::Eq
    );
    assert!(
        cx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "P0005" && d.message.contains("Usr")),
        "Usr/In/InstanceOf must emit a P0005 diagnostic from convert_binop"
    );
}

#[test]
fn convert_binop_unsupported_variants_emit_diagnostic_at_call_site() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Binary {
        op: HirBinaryOp::Usr,
        lhs: Box::new(int_lit(1)),
        rhs: Box::new(int_lit(2)),
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
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 for unsupported binary op");
    assert!(diag.message.contains("Usr"));
}

#[test]
fn convert_unaryop_maps_variants() {
    let mut cx = ctx();
    assert_eq!(convert_unaryop(HirUnaryOp::Neg, &mut cx), UnaryOp::Neg);
    assert_eq!(convert_unaryop(HirUnaryOp::Not, &mut cx), UnaryOp::Not);
    assert_eq!(
        convert_unaryop(HirUnaryOp::BitNot, &mut cx),
        UnaryOp::BitNot
    );
    assert_eq!(convert_unaryop(HirUnaryOp::TypeOf, &mut cx), UnaryOp::Not);
    assert_eq!(convert_unaryop(HirUnaryOp::Void, &mut cx), UnaryOp::Not);
    assert_eq!(convert_unaryop(HirUnaryOp::Delete, &mut cx), UnaryOp::Not);
    assert!(
        cx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "P0005" && d.message.contains("TypeOf")),
        "TypeOf/Void/Delete must emit a P0005 diagnostic from convert_unaryop"
    );
}
