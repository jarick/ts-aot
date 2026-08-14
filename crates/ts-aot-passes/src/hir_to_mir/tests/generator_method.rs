use super::common::*;

fn generator_instance_call(
    field_name: &str,
    args: Vec<HirExpr>,
    types: &mut TypeTable,
) -> (HirExpr, TypeId) {
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: gen_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args,
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    };
    (expr, i64_ty)
}

fn convert_generator_call(
    field_name: &str,
    args: Vec<HirExpr>,
) -> (MirExpr, Vec<MirStmt>, PassContext) {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let (expr, _i64_ty) = generator_instance_call(field_name, args, &mut types);
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    (mir, std::mem::take(out), cx)
}

fn assert_single_diag_unit_empty(
    field_name: &str,
    args: &[HirExpr],
    expected_code: &str,
    message_fragment: &str,
) {
    let arg_label = if args.is_empty() {
        format!("g.{field_name}()")
    } else {
        format!("g.{field_name}(...)")
    };
    let (mir, out, cx) = convert_generator_call(field_name, args.to_vec());
    assert_eq!(
        cx.diagnostics().len(),
        1,
        "{arg_label} must emit exactly one diagnostic and stop generic dispatch, got {:?}",
        cx.diagnostics()
    );
    let d = cx
        .diagnostics()
        .iter()
        .next()
        .expect("exactly one diagnostic");
    assert_eq!(
        d.code.as_str(),
        expected_code,
        "the single diagnostic must be {expected_code}, got {:?}",
        d
    );
    assert!(
        d.message.contains(message_fragment),
        "{expected_code} must carry the `{message_fragment}` message, got {:?}",
        d.message
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "{arg_label} must return MirExpr::Unit to stop fallback MIR generation, got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "{arg_label} must not lower any MirStmt after the diagnostic, got {out:?}"
    );
}

#[test]
fn generator_next_with_args_emits_single_e0406_and_returns_unit() {
    assert_single_diag_unit_empty("next", &[int_lit(5)], "E0406", "takes no arguments");
}

#[test]
fn generator_next_lowering_emits_single_runtime_call_with_one_owner_arg() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let (expr, i64_ty) = generator_instance_call("next", Vec::new(), &mut types);
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let out = std::mem::take(out);
    assert!(
        !cx.has_errors(),
        "g.next() with no args must compile cleanly, got: {:?}",
        cx.diagnostics()
    );
    assert_eq!(
        out.len(),
        1,
        "g.next() must emit exactly one MirStmt (the Runtime call), got: {:?}",
        out
    );
    let MirStmt::Runtime {
        op,
        args,
        dest,
        ty,
        target_ty,
    } = &out[0]
    else {
        panic!("g.next() must emit a MirStmt::Runtime, got: {:?}", out[0]);
    };
    assert_eq!(
        *op,
        RuntimeOp::GeneratorNext,
        "the emitted Runtime op must be GeneratorNext, got: {:?}",
        op
    );
    assert_eq!(
        args.len(),
        1,
        "RuntimeOp::GeneratorNext must carry exactly one owner arg, got: {:?}",
        args
    );
    assert!(
        matches!(&args[0], MirExpr::Local(id) if *id == LocalId::from_raw(0)),
        "the single owner arg must be a MirExpr::Local referencing the generator local, got: {:?}",
        args[0]
    );
    let dest = dest.expect("GeneratorNext must allocate a destination local for the result");
    let resolved = types
        .resolve(*ty)
        .cloned()
        .expect("the destination ty must resolve in the pass type table");
    assert_eq!(
        resolved,
        Type::GeneratorResult { inner: i64_ty },
        "the destination ty must be `Type::GeneratorResult {{ inner: i64 }}` (got: {:?})",
        resolved
    );
    assert!(
        target_ty.is_none(),
        "GeneratorNext must not carry a target_ty override, got: {:?}",
        target_ty
    );
    assert!(
        matches!(&mir, MirExpr::Local(id) if *id == dest),
        "convert_expr must return MirExpr::Local pointing at the destination, got: {:?}",
        mir
    );
}

#[test]
fn generator_return_method_emits_single_e0502_and_returns_unit() {
    assert_single_diag_unit_empty(
        "return",
        &[int_lit(5)],
        GENERATOR_DIAG_DEFERRED_METHOD,
        "Generator.prototype.return",
    );
}

#[test]
fn generator_throw_method_emits_single_e0502_and_returns_unit() {
    assert_single_diag_unit_empty(
        "throw",
        &[int_lit(5)],
        GENERATOR_DIAG_DEFERRED_METHOD,
        "Generator.prototype.throw",
    );
}

#[test]
fn generator_return_method_with_no_args_still_emits_e0502() {
    assert_single_diag_unit_empty(
        "return",
        &[],
        GENERATOR_DIAG_DEFERRED_METHOD,
        "Generator.prototype.return",
    );
}

#[test]
fn generator_throw_method_with_no_args_still_emits_e0502() {
    assert_single_diag_unit_empty(
        "throw",
        &[],
        GENERATOR_DIAG_DEFERRED_METHOD,
        "Generator.prototype.throw",
    );
}
