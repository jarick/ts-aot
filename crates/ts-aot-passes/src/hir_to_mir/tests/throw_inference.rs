use super::common::*;

/// A call to a free function by name, as the body walker emits it.
fn call_free(name: &str, ty: TypeId) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::new_inline(name),
            ty,
            span: Span::default(),
        })),
        args: Vec::new(),
        type_args: Vec::new(),
        ty,
        span: Span::default(),
    }
}

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
        &mut struct_id_map,
        &mut next_struct_id,
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut struct_id_map,
        &mut next_struct_id,
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut struct_id_map,
        &mut next_struct_id,
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut struct_id_map,
        &mut next_struct_id,
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut empty_types(),
        &mut cx,
        &[],
        &f.name,
    );
    assert_eq!(
        mir.throws,
        Some(declared_ty),
        "declared throws must win over inferred (TS spec: explicit annotation wins)"
    );
}

#[test]
fn infer_throws_types_a_primitive_thrown_expr_by_its_value() {
    let f = make_throw_inference_function(vec![HirStmt::Throw { expr: int_lit(0) }], None);
    let mut cx = ctx();
    let mut types = empty_types();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut ExprConverter::new(),
        &mut types,
        &mut cx,
        &[],
        &f.name,
    );
    let throws = mir
        .throws
        .expect("a `throw` statement must infer an error type");
    assert_eq!(
        types.resolve(throws),
        Some(&Type::F64),
        "a thrown literal is a `number`, so the error type is `f64` - never the raw \
         `TypeId::from_raw(0)` sentinel, which collided with whatever type was interned first \
         (for a method that is the enclosing class)"
    );
}

/// The fixpoint records the free-function declaration the call *selects*, so a
/// same-named declaration the call does not select cannot supply its error type.
/// Inside `ns`, `foo()` is `ns::foo`; the module-level `foo` is a different
/// callee even though it throws.
#[test]
fn module_throws_resolves_only_the_selected_free_function() {
    let mut types = TypeTable::new();
    let err = types.intern(&Type::F64);
    let error_ty = types.intern(&Type::Error);

    let throwing_foo = HirFunction {
        name: Atom::new_inline("foo"),
        ret: unit_ty(),
        throws: Some(err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };
    let quiet_foo = HirFunction {
        name: Atom::new_inline("foo"),
        ret: unit_ty(),
        ..Default::default()
    };
    let caller = HirFunction {
        name: Atom::new_inline("caller"),
        ret: unit_ty(),
        body: vec![HirStmt::Expr {
            expr: call_free("foo", error_ty),
        }],
        ..Default::default()
    };

    let mut prog = empty_hir();
    prog.push_decl(HirDecl::Function(throwing_foo));
    prog.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns"),
        members: vec![HirDecl::Function(quiet_foo), HirDecl::Function(caller)],
    });

    let throws = module_throws(&prog, &mut types);
    assert!(
        throws.contains_key(&Atom::new_inline("foo")),
        "the module-level `foo` throws; got {throws:?}"
    );
    assert!(
        !throws.contains_key(&Atom::new_inline("ns::caller")),
        "`ns::caller` calls `ns::foo`, which cannot throw: the module-level `foo` is a different \
         callee and must not supply its error type; got {throws:?}"
    );
}

/// A class-qualified `::m` suffix is the fallback for a *method* whose receiver
/// class is not known statically. A free function must never match it: `m()` is
/// the module-level `m`, not `C::m`.
#[test]
fn module_throws_does_not_match_a_method_suffix_for_a_free_function() {
    let mut types = TypeTable::new();
    let err = types.intern(&Type::F64);
    let error_ty = types.intern(&Type::Error);
    let class_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(0),
    });

    let throwing_method = HirFunction {
        name: Atom::new_inline("m"),
        ret: unit_ty(),
        throws: Some(err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };
    let class = ts_aot_ir_hir::HirClass {
        name: Atom::new_inline("C"),
        ty: class_ty,
        pre_pass_ty: None,
        fields: Vec::new(),
        methods: vec![throwing_method],
        extends: None,
        type_params: Vec::new(),
    };
    let free_m = HirFunction {
        name: Atom::new_inline("m"),
        ret: unit_ty(),
        ..Default::default()
    };
    let caller = HirFunction {
        name: Atom::new_inline("caller"),
        ret: unit_ty(),
        body: vec![HirStmt::Expr {
            expr: call_free("m", error_ty),
        }],
        ..Default::default()
    };

    let mut prog = empty_hir();
    prog.push_decl(HirDecl::Class(class));
    prog.push_decl(HirDecl::Function(free_m));
    prog.push_decl(HirDecl::Function(caller));

    let throws = module_throws(&prog, &mut types);
    assert!(
        throws.contains_key(&Atom::new_inline("C::m")),
        "the method `C::m` throws; got {throws:?}"
    );
    assert!(
        !throws.contains_key(&Atom::new_inline("caller")),
        "`caller` calls the free `m`, which cannot throw; `C::m` must not supply its error type \
         through the `::m` suffix fallback; got {throws:?}"
    );
}

/// A method call whose receiver class is not known statically carries only the
/// bare method name, so the fixpoint matches the class-qualified `::name` keys.
/// A same-named *free* function must not supply the type instead (the `Method`
/// variant deliberately skips the direct lookup).
#[test]
fn module_throws_matches_an_unresolved_method_by_suffix() {
    let mut types = TypeTable::new();
    let err = types.intern(&Type::F64);
    let error_ty = types.intern(&Type::Error);
    let class_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(0),
    });

    let throwing_method = HirFunction {
        name: Atom::new_inline("m"),
        ret: unit_ty(),
        throws: Some(err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };
    let class = ts_aot_ir_hir::HirClass {
        name: Atom::new_inline("C"),
        ty: class_ty,
        pre_pass_ty: None,
        fields: Vec::new(),
        methods: vec![throwing_method],
        extends: None,
        type_params: Vec::new(),
    };
    // `owner.m()` where the owner's type is unknown (the HIR local is `Error`), so
    // the call records only the bare method name.
    let caller = HirFunction {
        name: Atom::new_inline("caller"),
        ret: unit_ty(),
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(1),
                        ty: error_ty,
                        span: Span::default(),
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("m"),
                    ty: error_ty,
                    span: Span::default(),
                })),
                args: Vec::new(),
                type_args: Vec::new(),
                ty: error_ty,
                span: Span::default(),
            },
        }],
        ..Default::default()
    };

    // A *free* function with the same name throws a different error type: the
    // unresolved method call must take `C::m`'s, not this one's.
    let free_err = types.intern(&Type::String);
    let free_m = HirFunction {
        name: Atom::new_inline("m"),
        ret: unit_ty(),
        throws: Some(free_err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };

    let mut prog = empty_hir();
    prog.push_decl(HirDecl::Class(class));
    prog.push_decl(HirDecl::Function(free_m));
    prog.push_decl(HirDecl::Function(caller));

    let throws = module_throws(&prog, &mut types);
    assert!(
        throws.contains_key(&Atom::new_inline("C::m")),
        "the method `C::m` throws; got {throws:?}"
    );
    assert_eq!(
        throws.get(&Atom::new_inline("caller")).copied(),
        Some(err),
        "an unresolved method call matches `C::m` by the `::name` suffix, not the bare \
         free-function name; got {throws:?}"
    );
}

/// The `throw <call>` seed resolves its callee the way `resolve_callee` does: the
/// innermost namespace that declares the function wins. `N::Inner::f` throwing
/// `boom()` selects `N::boom`; looking the fully qualified name up and then the
/// *bare* one selects the module-level `boom` instead, and the seed is what the
/// fixpoint starts from - so the wrong function would supply the error type.
#[test]
fn a_thrown_call_seeds_the_return_type_of_its_enclosing_namespace() {
    let mut types = TypeTable::new();
    let err = types.intern(&Type::F64);
    let other = types.intern(&Type::String);
    let error_ty = types.intern(&Type::Error);

    // The module-level `boom` the bare-name lookup would find, with a *different*
    // declared return type.
    let module_boom = HirFunction {
        name: Atom::new_inline("boom"),
        ret: other,
        throws: Some(other),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };
    let ns_boom = HirFunction {
        // The declared return type is what the seed copies into the throws map.
        ret: err,
        throws: Some(err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        name: Atom::new_inline("boom"),
        ..Default::default()
    };
    let inner_caller = HirFunction {
        name: Atom::new_inline("f"),
        ret: unit_ty(),
        body: vec![HirStmt::Throw {
            expr: call_free("boom", error_ty),
        }],
        ..Default::default()
    };

    let mut prog = empty_hir();
    prog.push_decl(HirDecl::Function(module_boom));
    prog.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("N"),
        members: vec![
            HirDecl::Function(ns_boom),
            HirDecl::Namespace {
                name: Atom::new_inline("Inner"),
                members: vec![HirDecl::Function(inner_caller)],
            },
        ],
    });

    let throws = module_throws(&prog, &mut types);
    assert_eq!(
        throws.get(&Atom::new_inline("N::Inner::f")).copied(),
        Some(err),
        "`N::Inner::f` throws `boom()`, which resolves to `N::boom` - not to the module-level \
         `boom`; got {throws:?}"
    );
}

/// The `::name` fallback belongs to a *method* whose receiver class is unknown, so
/// its prefix has to name a class of this program. A namespace-qualified free
/// function (`A::m`) ends with the same suffix and must not supply the type.
#[test]
fn module_throws_ignores_a_namespace_function_for_an_unresolved_method() {
    let mut types = TypeTable::new();
    let err = types.intern(&Type::F64);
    let error_ty = types.intern(&Type::Error);

    let namespaced = HirFunction {
        name: Atom::new_inline("m"),
        ret: unit_ty(),
        throws: Some(err),
        body: vec![HirStmt::Throw { expr: int_lit(1) }],
        ..Default::default()
    };
    // `owner.m()` where the owner's type is unknown, so the call records only the
    // bare method name (`RecordedCallee::Method`).
    let caller = HirFunction {
        name: Atom::new_inline("caller"),
        ret: unit_ty(),
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(1),
                        ty: error_ty,
                        span: Span::default(),
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("m"),
                    ty: error_ty,
                    span: Span::default(),
                })),
                args: Vec::new(),
                type_args: Vec::new(),
                ty: error_ty,
                span: Span::default(),
            },
        }],
        ..Default::default()
    };

    let mut prog = empty_hir();
    prog.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("A"),
        members: vec![HirDecl::Function(namespaced)],
    });
    prog.push_decl(HirDecl::Function(caller));

    let throws = module_throws(&prog, &mut types);
    assert!(
        throws.contains_key(&Atom::new_inline("A::m")),
        "the namespaced function throws; got {throws:?}"
    );
    assert!(
        !throws.contains_key(&Atom::new_inline("caller")),
        "`A::m` is a free function, not the method this call reaches; got {throws:?}"
    );
}
