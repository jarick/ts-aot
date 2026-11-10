use super::common::*;

#[test]
fn converter_starts_with_empty_state() {
    let c = ExprConverter::new();
    assert_eq!(c.peek_next_local(), 0);
}

#[test]
fn default_matches_new() {
    let a = ExprConverter::default();
    let b = ExprConverter::new();
    assert_eq!(a.peek_next_local(), b.peek_next_local());
}

#[test]
fn fresh_local_increments_counter() {
    let mut c = ExprConverter::new();
    let l0 = c.map_local_id(LocalId::from_raw(0));
    let l1 = c.map_local_id(LocalId::from_raw(1));
    assert_ne!(l0, l1);
    assert_eq!(c.peek_next_local(), 2);
}

#[test]
fn with_function_remap_and_offset_starts_past_offset() {
    let c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 5);
    assert_eq!(c.peek_next_local(), 5);
    let c2 = ExprConverter::with_function_remap(HashMap::new());
    assert_eq!(c2.peek_next_local(), 0);
}

#[test]
fn seed_params_advances_next_local_past_param_count() {
    let mut c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 0);
    c.seed_params(&[
        LocalId::from_raw(0),
        LocalId::from_raw(1),
        LocalId::from_raw(2),
    ]);
    assert_eq!(c.peek_next_local(), 3);
    let fresh = c.map_local_id(LocalId::from_raw(99));
    assert_eq!(fresh, LocalId::from_raw(3));
}

#[test]
fn map_local_returns_same_id_for_same_old() {
    let mut c = ExprConverter::new();
    let src = LocalId::from_raw(42);
    let a = c.map_local(src);
    let b = c.map_local(src);
    assert_eq!(a, b);
    assert_eq!(c.peek_next_local(), 1);
}

#[test]
fn map_local_id_returns_local_id() {
    let mut c = ExprConverter::new();
    let old = LocalId::from_raw(7);
    let new = c.map_local_id(old);
    assert_eq!(c.map_local_id(old), new);
}

#[test]
fn register_local_name_does_not_panic() {
    let mut c = ExprConverter::new();
    let id = LocalId::from_raw(0);
    c.register_local_name(id, Atom::new_inline("11"));
    assert_eq!(
        c.local_name(id),
        Some(Atom::new_inline("11")),
        "register_local_name must record the (LocalId, Atom) mapping; the lookup path is the \
         narrowest observable API to verify the registration took effect"
    );
}

#[test]
fn resolve_callee_function_uses_remap() {
    let mut remap = HashMap::new();
    remap.insert(FunctionId::from_raw(3), FunctionId::from_raw(99));
    let mut c = ExprConverter::with_function_remap(remap);
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(3)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(99));
}

#[test]
fn resolve_callee_function_without_remap_returns_input() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(7)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(7));
}

#[test]
fn resolve_callee_indirect_is_placeholder_and_warning() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Indirect(Box::new(int_lit(1))), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(
        !cx.has_errors(),
        "PR 1.2: unresolved indirect callee downgrades P0005 to warning (runtime fallback handles it)"
    );
    let p0005_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .count();
    assert_eq!(
        p0005_count, 1,
        "P0005 must still be emitted as a warning, got {p0005_count} diags"
    );
}

#[test]
fn resolve_callee_closure_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Closure(LocalId::from_raw(0)), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}

#[test]
fn resolve_callee_runtime_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(
        &HirCallee::Runtime {
            name: Atom::new_inline("0"),
            ty: TypeId::from_raw(0),
        },
        &mut cx,
    );
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}

#[test]
fn global_decl_type_resolves_through_namespace_paths() {
    let root_ty = TypeId::from_raw(11);
    let ns_ty = TypeId::from_raw(22);
    let shared_ty = TypeId::from_raw(33);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("counter"),
        ty: root_ty,
        init: None,
    });
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("shared"),
        ty: shared_ty,
        init: None,
    });
    prog.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns"),
        members: vec![HirDecl::Global {
            name: Atom::new_inline("counter"),
            ty: ns_ty,
            init: None,
        }],
    });

    let mut c = ExprConverter::new();
    c.set_program(Arc::new(prog));

    c.set_namespace_path(&[]);
    assert_eq!(
        c.global_decl_type(&Atom::new_inline("counter")),
        Some(root_ty),
        "at the root, `counter` must resolve to the top-level global"
    );

    c.set_namespace_path(&["ns".to_owned()]);
    assert_eq!(
        c.global_decl_type(&Atom::new_inline("counter")),
        Some(ns_ty),
        "inside `ns`, `counter` must resolve to the namespace global, not the top-level one"
    );
    assert_eq!(
        c.global_decl_type(&Atom::new_inline("shared")),
        Some(shared_ty),
        "a root-level global must still resolve from inside a namespace (probe inward to root)"
    );
    assert_eq!(
        c.global_decl_type(&Atom::new_inline("missing")),
        None,
        "an unknown name must resolve to None"
    );
}

#[test]
fn value_static_ty_prefers_namespace_qualified_function() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let i64_ty = types.intern(&Type::I64);
    let error_ty = types.intern(&Type::Error);

    let root = HirFunction {
        name: Atom::new_inline("g"),
        ret: string_ty,
        ..Default::default()
    };
    let inner = HirFunction {
        name: Atom::new_inline("g"),
        ret: i64_ty,
        ..Default::default()
    };

    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(root));
    prog.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns"),
        members: vec![HirDecl::Function(inner)],
    });

    let call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::new_inline("g"),
            ty: error_ty,
            span: Span::new(0, 0),
        })),
        args: vec![],
        type_args: vec![],
        ty: error_ty,
        span: Span::new(0, 0),
    };

    let mut c = ExprConverter::new();
    c.set_program(Arc::new(prog));
    c.set_namespace_path(&["ns".to_owned()]);
    assert_eq!(
        c.value_static_ty(&call, &mut types),
        Some(i64_ty),
        "inside `ns` the call must resolve to the namespace function, not the top-level `g`"
    );

    c.set_namespace_path(&[]);
    assert_eq!(
        c.value_static_ty(&call, &mut types),
        Some(string_ty),
        "at the root the call must resolve to the top-level `g`"
    );
}

#[test]
fn expr_type_recovers_a_literal() {
    let mut types = TypeTable::new();
    let c = ExprConverter::new();
    assert_eq!(
        c.expr_type(&HirExpr::Int(1, Span::new(0, 0)), &mut types),
        Some(types.intern(&Type::I64)),
        "an integer literal must recover as i64"
    );
}

#[test]
fn expr_type_returns_none_for_unit() {
    let mut types = TypeTable::new();
    let c = ExprConverter::new();
    assert_eq!(
        c.expr_type(&HirExpr::Unit(Span::new(0, 0)), &mut types),
        None
    );
}

#[test]
fn expr_type_does_not_recover_an_error_typed_local() {
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let c = ExprConverter::new();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: error_ty,
        span: Span::new(0, 0),
    };
    assert_eq!(
        c.expr_type(&local, &mut types),
        None,
        "an Error-typed local must not be reported as a recovered type"
    );
}

#[test]
fn expr_type_recovers_a_type_id_zero_local_that_is_bool() {
    let mut types = TypeTable::new();
    let boolean_ty = types.intern(&Type::Bool);
    assert_eq!(
        boolean_ty,
        TypeId::from_raw(0),
        "precondition: bool interns first in a fresh table"
    );
    let c = ExprConverter::new();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: boolean_ty,
        span: Span::new(0, 0),
    };
    assert_eq!(
        c.expr_type(&local, &mut types),
        Some(boolean_ty),
        "a local whose type is legitimately TypeId(0) must still recover; only the literal \
         sentinel is excluded, and that only via has_static_type()"
    );
}

#[test]
fn expr_type_recovers_a_ternary_of_equal_branches() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let promise_ty = types.intern(&Type::Promise {
        ok: f64_ty,
        err: None,
    });
    let lhs = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_ty,
        span: Span::new(0, 0),
    };
    let rhs = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: promise_ty,
        span: Span::new(0, 0),
    };
    let ternary = HirExpr::Ternary {
        cond: Box::new(HirExpr::Bool(true, Span::new(0, 0))),
        then_branch: Box::new(lhs),
        else_branch: Box::new(rhs),
        ty: types.intern(&Type::Error),
        span: Span::new(0, 0),
    };
    let c = ExprConverter::new();
    assert_eq!(c.expr_type(&ternary, &mut types), Some(promise_ty));
}

#[test]
fn expr_type_joins_numeric_ternary_branches_to_f64() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    assert_ne!(i64_ty, f64_ty);
    let ternary = HirExpr::Ternary {
        cond: Box::new(HirExpr::Bool(true, Span::new(0, 0))),
        then_branch: Box::new(HirExpr::Int(1, Span::new(0, 0))),
        else_branch: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: f64_ty,
            span: Span::new(0, 0),
        }),
        ty: types.intern(&Type::Error),
        span: Span::new(0, 0),
    };
    let c = ExprConverter::new();
    assert_eq!(c.expr_type(&ternary, &mut types), Some(f64_ty));
}

#[test]
fn expr_type_recovers_the_ok_type_of_an_awaited_promise() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let promise_ty = types.intern(&Type::Promise {
        ok: f64_ty,
        err: None,
    });
    let awaited = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: promise_ty,
        span: Span::new(0, 0),
    };
    let await_expr = HirExpr::Await {
        expr: Box::new(awaited),
        ty: types.intern(&Type::Error),
        span: Span::new(0, 0),
    };
    let c = ExprConverter::new();
    assert_eq!(c.expr_type(&await_expr, &mut types), Some(f64_ty));
}

#[test]
fn expr_type_recovers_a_promise_resolve_call_without_a_context() {
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let call = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Promise"),
                ty: error_ty,
                span: Span::new(0, 0),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("resolve"),
            ty: error_ty,
            span: Span::new(0, 0),
        })),
        args: vec![HirExpr::Int(1, Span::new(0, 0))],
        type_args: vec![],
        ty: error_ty,
        span: Span::new(0, 0),
    };
    let c = ExprConverter::new();
    let i64_ty = types.intern(&Type::I64);
    let expected = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    assert_eq!(
        c.expr_type(&call, &mut types),
        Some(expected),
        "with no enclosing context, Promise.resolve(<int literal>) recovers the literal's type"
    );
}

#[test]
fn resolve_field_id_resolves_global_owner_from_declared_type() {
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let box_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(7),
    });
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("g"),
        ty: box_ty,
        init: None,
    });
    let mut c = ExprConverter::new();
    c.set_program(Arc::new(prog));
    let mut field_id_lookup = HashMap::new();
    field_id_lookup.insert(
        (StructId::from_raw(7), Atom::new_inline("x")),
        FieldId::from_raw(3),
    );
    c.set_field_id_lookup(field_id_lookup);
    let mut shared_ids = HashMap::new();
    shared_ids.insert(box_ty, StructId::from_raw(7));

    let owner = HirExpr::Global {
        name: Atom::new_inline("g"),
        ty: error_ty,
        span: Span::default(),
    };
    let mut cx = ctx();
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &shared_ids,
        &types,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(3),
        "a `HirExpr::Global` owner must resolve its field via the global's declared type; \
         diagnostics: {:?}",
        cx.diagnostics()
    );
    assert!(!cx.has_errors(), "{:?}", cx.diagnostics());
}

#[test]
fn resolve_field_id_carries_global_declared_type_through_chains() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let mut types = TypeTable::new();
    let error_ty = types.intern(&Type::Error);
    let inner_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(2),
    });
    let box_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(1),
    });
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Box"),
        ty: box_ty,
        fields: vec![HirField {
            name: Atom::new_inline("sub"),
            ty: inner_ty,
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
        ..Default::default()
    }));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("g"),
        ty: box_ty,
        init: None,
    });
    let mut c = ExprConverter::new();
    c.set_program(Arc::new(prog));
    let mut field_id_lookup = HashMap::new();
    field_id_lookup.insert(
        (StructId::from_raw(2), Atom::new_inline("x")),
        FieldId::from_raw(5),
    );
    c.set_field_id_lookup(field_id_lookup);
    let mut shared_ids = HashMap::new();
    shared_ids.insert(inner_ty, StructId::from_raw(2));

    let owner = HirExpr::Field {
        owner: Box::new(HirExpr::Global {
            name: Atom::new_inline("g"),
            ty: error_ty,
            span: Span::default(),
        }),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("sub"),
        ty: error_ty,
        span: Span::default(),
    };
    let mut cx = ctx();
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &shared_ids,
        &types,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(5),
        "the global's declared type must be carried through chained field resolution (g.sub.x); \
         diagnostics: {:?}",
        cx.diagnostics()
    );
    assert!(!cx.has_errors(), "{:?}", cx.diagnostics());
}

#[test]
fn lookup_class_struct_by_name_walks_namespace_depths() {
    let mut c = ExprConverter::new();
    c.set_namespace_path(&["Outer".to_owned(), "Inner".to_owned()]);
    c.class_struct_by_name = HashMap::from([
        (Atom::new_inline("Outer::Inner::C"), StructId::from_raw(7)),
        (Atom::new_inline("Outer::D"), StructId::from_raw(8)),
        (Atom::new_inline("Root"), StructId::from_raw(9)),
        (Atom::new_inline("C"), StructId::from_raw(10)),
    ]);
    assert_eq!(
        c.lookup_class_struct_by_name(&Atom::new_inline("C")),
        Some(StructId::from_raw(7)),
        "a namespace-local class must shadow a top-level class with the same name"
    );
    assert_eq!(
        c.lookup_class_struct_by_name(&Atom::new_inline("D")),
        Some(StructId::from_raw(8)),
        "a class declared in an enclosing namespace must resolve by its leaf name"
    );
    assert_eq!(
        c.lookup_class_struct_by_name(&Atom::new_inline("Root")),
        Some(StructId::from_raw(9)),
        "a top-level class must resolve by its leaf name"
    );
    assert_eq!(
        c.lookup_class_struct_by_name(&Atom::new_inline("Missing")),
        None
    );
}

#[test]
fn value_static_ty_recovers_unresolved_indirect_calls() {
    let mut types = TypeTable::new();
    let number = types.intern(&Type::F64);
    let error = types.intern(&Type::Error);
    let c = ExprConverter::new();
    for callee in [
        HirExpr::Global {
            name: Atom::new_inline("missing"),
            ty: error,
            span: Span::new(0, 0),
        },
        HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: error,
            span: Span::new(0, 0),
        },
    ] {
        for stored in [number, error, TypeId::from_raw(u32::MAX)] {
            let call = HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(callee.clone())),
                args: vec![],
                type_args: vec![],
                ty: stored,
                span: Span::new(0, 0),
            };
            assert_eq!(
                c.value_static_ty(&call, &mut types),
                (stored == number).then_some(number)
            );
        }
    }
}

#[test]
fn expr_type_filters_unknown_function_returns_at_root_and_in_namespace() {
    let mut types = TypeTable::new();
    let number = types.intern(&Type::F64);
    let error = types.intern(&Type::Error);
    for ret in [number, error, TypeId::from_raw(u32::MAX)] {
        for nested in [false, true] {
            let mut prog = HirProgram::new(ModuleId::from_raw(0));
            let function = HirDecl::Function(HirFunction {
                name: Atom::new_inline("g"),
                ret,
                ..Default::default()
            });
            prog.push_decl(if nested {
                HirDecl::Namespace {
                    name: Atom::new_inline("other"),
                    members: vec![function],
                }
            } else {
                function
            });
            let mut c = ExprConverter::new();
            c.set_program(Arc::new(prog));
            if nested {
                c.set_namespace_path(&["other".to_owned()]);
            }
            let call = HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline("g"),
                    ty: error,
                    span: Span::new(0, 0),
                })),
                args: vec![],
                type_args: vec![],
                ty: number,
                span: Span::new(0, 0),
            };
            assert_eq!(
                c.expr_type(&call, &mut types),
                (ret == number).then_some(number)
            );
        }
    }
}
