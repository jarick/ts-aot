use super::common::*;

fn make_throw_inference_function(body: Vec<HirStmt>, throws: Option<TypeId>) -> HirFunction {
    HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }
}

#[test]
fn body_can_throw_propagates_through_struct_literal_fields() {
    let throwing_call_ty = TypeId::from_raw(0);
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: throwing_call_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::StructLiteral {
            ty: throwing_call_ty,
            fields: vec![(FieldId::from_raw(0), call)],

            span: Span::default(),
        }),
    }];
    let f = make_throw_inference_function(body, None);
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "function returning a struct literal whose field calls a throwing callee must be can_throw"
    );
}

#[test]
fn body_can_throw_stays_false_for_plain_struct_literal() {
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::StructLiteral {
            ty: unit_ty(),
            fields: vec![(FieldId::from_raw(0), int_lit(1))],

            span: Span::default(),
        }),
    }];
    let f = make_throw_inference_function(body, None);
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !mir.effects.can_throw,
        "struct literal with non-throwing fields must not propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_assignment_target() {
    let throwing_call_ty = TypeId::from_raw(0);
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: throwing_call_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: throwing_call_ty,

        span: Span::default(),
    };
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(field_target),
            value: Box::new(int_lit(1)),
            ty: throwing_call_ty,

            span: Span::default(),
        },
    }];
    let f = make_throw_inference_function(body, None);
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "assignment with throwing call on LHS (e.g. obj().x = 1) must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_assignment_target_index() {
    let throwing_call_ty = TypeId::from_raw(0);
    let arr_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(77)),
        args: Vec::new(),
        ty: throwing_call_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let idx_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(78)),
        args: Vec::new(),
        ty: throwing_call_ty,
        type_args: vec![],

        span: Span::default(),
    };
    let index_lhs = HirExpr::Index {
        owner: Box::new(arr_target),
        index: Box::new(idx_target),
        ty: throwing_call_ty,

        span: Span::default(),
    };
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(index_lhs),
            value: Box::new(int_lit(1)),
            ty: throwing_call_ty,

            span: Span::default(),
        },
    }];
    let f = make_throw_inference_function(body, None);
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "assignment with throwing calls in arr()[idx()] LHS must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_if_condition_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::If {
            cond: call,
            then: Box::new(HirStmt::Return { value: None }),
            otherwise: None,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "If with throwing cond must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_ternary_branches() {
    let throwing_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
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
            value: Some(HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(true, Span::default())),
                then_branch: Box::new(throwing_call),
                else_branch: Box::new(HirExpr::Int(0, Span::default())),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Ternary with throwing then_branch must propagate can_throw (without this arm, function is mis-analyzed as Plain)"
    );
}

#[test]
fn body_can_throw_propagates_through_while_condition_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::While {
            cond: call,
            body: Box::new(HirStmt::Return { value: None }),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "While with throwing cond must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_for_of_iter_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::ForOf {
            binding: LocalId::from_raw(0),
            iter: call,
            body: Box::new(HirStmt::Return { value: None }),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "ForOf with throwing iter must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_switch_discriminant_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::Switch {
            disc: call,
            cases: vec![HirSwitchCase::new(
                Some(int_lit(1)),
                vec![HirStmt::Return { value: None }],
            )],
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Switch with throwing discriminant must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_catch_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: Some(ts_aot_ir_hir::HirCatchClause::new(
                None,
                Box::new(HirStmt::Expr { expr: call }),
            )),
            finally: None,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Try with throwing catch body must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_finally_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: None,
            finally: Some(Box::new(HirStmt::Expr { expr: call })),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Try with throwing finally body must propagate can_throw"
    );
}

#[test]
fn body_can_throw_await_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Await {
                expr: Box::new(int_lit(0)),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Await must be considered throwing (rejection)"
    );
}

#[test]
fn body_can_throw_new_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Ctor"),
                    ty: unit_ty(),

                    span: Span::default(),
                }),
                args: Vec::new(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "New (constructor invocation) must be considered throwing"
    );
}

#[test]
fn body_can_throw_yield_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(int_lit(0))),
                ty: unit_ty(),

                span: Span::default(),
            },
        }],
        is_async: false,
        is_generator: true,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Yield must be considered throwing (delegated generator may throw)"
    );
}

#[test]
fn infer_throws_is_none_for_call_only_function() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
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
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "function with a Call expr must surface can_throw=true (call may throw at runtime)"
    );
    assert!(
        mir.throws.is_none(),
        "function without a Throw statement must NOT be a throwing function; got throws={:?}",
        mir.throws
    );
}

#[test]
fn infer_throws_is_none_for_if_with_throwing_cond_only() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
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
        body: vec![HirStmt::If {
            cond: call,
            then: Box::new(HirStmt::Return { value: None }),
            otherwise: None,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(mir.effects.can_throw);
    assert!(
        mir.throws.is_none(),
        "If with throwing cond (no Throw) must NOT be a throwing function"
    );
}

#[test]
fn infer_throws_uses_real_source_when_throwing_typed_expr() {
    let custom_err_ty = TypeId::from_raw(99);
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: custom_err_ty,

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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(custom_err_ty),
        "throws must be derived from the thrown expression's type, not a sentinel"
    );
}

#[test]
fn infer_throws_uses_ternary_ty_not_sentinel() {
    let custom_err_ty = TypeId::from_raw(77);
    let then_ty = TypeId::from_raw(123);
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(true, Span::default())),
                then_branch: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: then_ty,

                    span: Span::default(),
                }),
                else_branch: Box::new(HirExpr::Int(0, Span::default())),
                ty: custom_err_ty,
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
    assert_eq!(
        mir.throws,
        Some(custom_err_ty),
        "throw_expr_type must use the Ternary's `ty` (real type), not the TypeId::from_raw(0) \
         sentinel; the then_branch.ty ({then_ty:?}) was deliberately set DIFFERENT from the \
         Ternary.ty ({custom_err_ty:?}) so the assertion can only pass if inference reads the \
         Ternary node's type, not the inner expression's type"
    );
}

#[test]
fn infer_throws_respects_declared_over_inferred() {
    let declared_ty = TypeId::from_raw(7);
    let f = make_throw_inference_function(vec![HirStmt::Return { value: None }], Some(declared_ty));
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(declared_ty),
        "declared throws must win over inferred (TS spec: explicit annotation wins)"
    );
}

#[test]
fn infer_throws_uses_sentinel_for_primitive_thrown_expr() {
    let f = make_throw_inference_function(vec![HirStmt::Throw { expr: int_lit(0) }], None);
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(TypeId::from_raw(0)),
        "primitive throw (no real source type) must fall back to TypeId(0) sentinel"
    );
}
