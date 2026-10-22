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
