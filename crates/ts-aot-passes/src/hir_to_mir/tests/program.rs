use super::common::*;

#[test]
fn convert_program_empty_keeps_module() {
    let hir = empty_hir();
    let mut cx = ctx();
    let mir = convert_program(&hir, &mut empty_types(), &mut cx);
    assert_eq!(mir.module, hir.module);
    assert_eq!(mir.decl_count(), 0);
}

#[test]
fn convert_program_assigns_distinct_function_ids() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    for i in 0..3 {
        prog.push_decl(HirDecl::Function(HirFunction {
            name: Atom::from(format!("fn{}", i)),
            params: Vec::new(),
            ret: unit_ty(),
            throws: None,
            body: vec![HirStmt::Return { value: None }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }));
    }
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let functions: Vec<_> = mir.functions().collect();
    assert_eq!(functions.len(), 3);
    let ids: std::collections::HashSet<_> = functions.iter().map(|f| f.id).collect();
    assert_eq!(
        ids.len(),
        3,
        "FunctionIds must be distinct across top-level decls"
    );
}

#[test]
fn convert_program_resolves_indirect_global_callee_to_function_id() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("callee"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("caller"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline("callee"),
                    ty: unit_ty(),

                    span: Span::default(),
                })),
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
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let p0005: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .collect();
    assert!(
        p0005.is_empty(),
        "expected no P0005 (indirect callee) errors, got {}: {:?}",
        p0005.len(),
        p0005
    );
    let caller = mir
        .functions()
        .find(|f| f.name == Atom::new_inline("caller"))
        .expect("caller function present in MIR");
    let stmt = caller
        .body
        .block
        .stmts
        .first()
        .expect("caller has at least one stmt");
    let MirStmt::Expr(MirExpr::Call { callee, .. }) = stmt else {
        panic!("expected MirStmt::Expr(MirExpr::Call), got {stmt:?}");
    };
    assert_eq!(
        *callee,
        FunctionId::from_raw(0),
        "caller's call to global 'callee' must resolve to FunctionId::from_raw(0); got {callee:?}"
    );
}

#[test]
fn convert_program_assigns_distinct_struct_ids() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    for i in 0..2 {
        prog.push_decl(HirDecl::Class(HirClass {
            name: Atom::from(format!("cls{}", i)),
            ty: TypeId::from_raw(100 + i),
            fields: vec![HirField {
                name: Atom::from(format!("f{}", i)),
                ty: unit_ty(),
            }],
            methods: Vec::new(),
            extends: None,
            type_params: Vec::new(),
        }));
    }
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let structs: Vec<_> = mir.structs().collect();
    assert_eq!(structs.len(), 2);
    let ids: std::collections::HashSet<_> = structs.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 2, "StructIds must be distinct across classes");
}

#[test]
fn convert_program_struct_id_consistent_across_functions_for_same_type() {
    let shared_ty = TypeId::from_raw(99);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    let make_fn = |name: u32, ty: TypeId| HirFunction {
        name: Atom::from(format!("f{}", name)),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::StructLiteral {
                ty,
                fields: Vec::new(),

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    prog.push_decl(HirDecl::Function(make_fn(1, shared_ty)));
    prog.push_decl(HirDecl::Function(make_fn(2, shared_ty)));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let mut struct_literal_ids: Vec<ts_aot_core::StructId> = Vec::new();
    for func in mir.functions() {
        for s in &func.body.block.stmts {
            let sl = match s {
                MirStmt::Return(Some(MirExpr::StructLiteral { struct_id, .. })) => Some(*struct_id),
                MirStmt::Expr(MirExpr::StructLiteral { struct_id, .. }) => Some(*struct_id),
                _ => None,
            };
            if let Some(id) = sl {
                struct_literal_ids.push(id);
            }
        }
    }
    assert_eq!(
        struct_literal_ids.len(),
        2,
        "expected 2 StructLiteral exprs, got {struct_literal_ids:?}"
    );
    assert_eq!(
        struct_literal_ids[0], struct_literal_ids[1],
        "same HIR TypeId must yield same MIR StructId across functions (got {:?})",
        struct_literal_ids
    );
}

#[test]
fn convert_program_class_methods_use_method_function_kind() {
    use ts_aot_ir_hir::{HirClass, HirParam};
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("42"),
        ty: TypeId::from_raw(4242),
        fields: Vec::new(),
        methods: vec![HirFunction {
            name: Atom::new_inline("100"),
            params: vec![HirParam {
                name: Atom::new_inline("200"),
                ty: unit_ty(),
            }],
            ret: unit_ty(),
            throws: None,
            body: vec![HirStmt::Return { value: None }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }],
        extends: None,
        type_params: Vec::new(),
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let expected_owner = struct_decl.id;
    assert_eq!(struct_decl.methods.len(), 1);
    let method = &struct_decl.methods[0];
    let (owner, self_param) = match method.kind {
        FunctionKind::Method { owner, self_param } => (owner, self_param),
        ref other => panic!("expected FunctionKind::Method, got {other:?}"),
    };
    assert_eq!(
        owner, expected_owner,
        "Method.owner must match owning struct"
    );
    assert_eq!(
        self_param, method.params[0].id,
        "Method.self_param must be the first param's LocalId"
    );
}

#[test]
fn convert_program_class_struct_id_shared_with_new_and_struct_literal() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(7777);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("2"),
        params: Vec::new(),
        ret: class_ty,
        throws: None,
        body: vec![
            HirStmt::Expr {
                expr: HirExpr::New {
                    callee: Box::new(HirExpr::Global {
                        name: Atom::new_inline("1"),
                        ty: class_ty,

                        span: Span::default(),
                    }),
                    args: Vec::new(),
                    ty: class_ty,

                    span: Span::default(),
                },
            },
            HirStmt::Return {
                value: Some(HirExpr::StructLiteral {
                    ty: class_ty,
                    fields: vec![(FieldId::from_raw(0), int_lit(1))],

                    span: Span::default(),
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let mut new_id: Option<ts_aot_core::StructId> = None;
    let mut literal_id: Option<ts_aot_core::StructId> = None;
    let mut new_seen = false;
    for func in mir.functions() {
        for s in &func.body.block.stmts {
            if let MirStmt::Let {
                init: Some(MirExpr::StructLiteral { struct_id, .. }),
                ..
            } = s
                && !new_seen
            {
                new_id = Some(*struct_id);
                new_seen = true;
            }
            if let MirStmt::Return(Some(MirExpr::StructLiteral { struct_id, .. })) = s {
                literal_id = Some(*struct_id);
            }
        }
    }
    let new_id = new_id.expect("expected New expression to lower");
    let literal_id = literal_id.expect("expected StructLiteral expression to lower");
    assert_eq!(
        new_id, class_struct_id,
        "new Foo() must use class's StructId"
    );
    assert_eq!(
        literal_id, class_struct_id,
        "StructLiteral with class TypeId must use class's StructId"
    );
}

#[test]
fn convert_program_class_struct_id_shared_even_when_function_decl_comes_first() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(8888);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("2"),
        params: Vec::new(),
        ret: class_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("1"),
                    ty: class_ty,

                    span: Span::default(),
                }),
                args: Vec::new(),
                ty: class_ty,

                span: Span::default(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let func = mir.functions().next().expect("expected one function");
    let mut found_new_id: Option<ts_aot_core::StructId> = None;
    for s in &func.body.block.stmts {
        if let MirStmt::Let {
            init: Some(MirExpr::StructLiteral { struct_id, .. }),
            ..
        } = s
        {
            found_new_id = Some(*struct_id);
        }
    }
    let new_id = found_new_id.expect("expected New expression to lower");
    assert_eq!(
        new_id, class_struct_id,
        "new Foo() must use class's StructId even when class decl follows function decl"
    );
}

#[test]
fn convert_program_preserves_import_module_path_from_atom() {
    use ts_aot_ir_hir::{HirExport, HirImport};
    let module_id = Atom::new_inline("./other");
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.imports.push(HirImport {
        module: module_id,
        name: Atom::new_inline("7"),
        alias: None,
    });
    prog.exports.push(HirExport {
        name: Atom::new_inline("9"),
        alias: None,
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    assert_eq!(mir.imports.len(), 1);
    assert_eq!(mir.imports[0].module, "./other");
    assert_eq!(mir.imports[0].symbol, Atom::new_inline("7"));
    assert_eq!(mir.exports.len(), 1);
    assert_eq!(mir.exports[0].symbol, Atom::new_inline("9"));
}

#[test]
fn convert_program_class_method_with_no_params_is_skipped() {
    use ts_aot_ir_hir::HirClass;
    let class_ty = TypeId::from_raw(5555);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: Vec::new(),
        methods: vec![HirFunction {
            name: Atom::new_inline("100"),
            params: Vec::new(),
            ret: unit_ty(),
            throws: None,
            body: vec![HirStmt::Return { value: None }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }],
        extends: None,
        type_params: Vec::new(),
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    assert!(
        struct_decl.methods.is_empty(),
        "method without receiver parameter must be dropped from the struct, not converted to Method {{ self_param: LocalId(0) }}"
    );
}

#[test]
fn convert_program_exported_function_uses_atom_name_as_export_name() {
    let name_id = Atom::new_inline("render");
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: name_id,
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: true,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    assert_eq!(
        func.export_name.as_deref(),
        Some("render"),
        "export_name must come from the function name (Atom), not FunctionId"
    );
}

#[test]
fn convert_program_resolves_field_id_for_non_first_field() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(7777);
    let field_a_ty = TypeId::from_raw(8888);
    let field_b_ty = TypeId::from_raw(8889);
    let field_c_ty = TypeId::from_raw(8890);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Foo"),
        ty: class_ty,
        fields: vec![
            HirField {
                name: Atom::new_inline("a"),
                ty: field_a_ty,
            },
            HirField {
                name: Atom::new_inline("b"),
                ty: field_b_ty,
            },
            HirField {
                name: Atom::new_inline("c"),
                ty: field_c_ty,
            },
        ],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getB"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: class_ty,
        }],
        ret: field_b_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: class_ty,

                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("b"),
                ty: field_b_ty,

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(1),
        "field `b` must resolve to its post-flatten index in the class, not the placeholder 0"
    );
}

#[test]
fn convert_program_resolves_field_id_after_lower_classes_flatten() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let parent_ty = TypeId::from_raw(100);
    let child_ty = TypeId::from_raw(200);
    let parent_field_ty = TypeId::from_raw(101);
    let child_field_ty = TypeId::from_raw(201);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Parent"),
        ty: parent_ty,
        fields: vec![HirField {
            name: Atom::new_inline("p"),
            ty: parent_field_ty,
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Child"),
        ty: child_ty,
        fields: vec![HirField {
            name: Atom::new_inline("c"),
            ty: child_field_ty,
        }],
        methods: Vec::new(),
        extends: Some(Atom::new_inline("Parent")),
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getC"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: child_ty,
        }],
        ret: child_field_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: child_ty,

                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("c"),
                ty: child_field_ty,

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut cx = ctx();
    let mut types = TypeTable::new();
    lower_classes(&mut prog, &mut types, &mut cx);
    let mir = convert_program(&prog, &mut types, &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(1),
        "post-lower_classes, Child's `c` lives at index 1 (after inherited `p`)"
    );
}

#[test]
fn convert_program_resolves_field_id_preserves_placeholder_for_unknown_field() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(300);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("OnlyA"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("a"),
            ty: TypeId::from_raw(0),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getMissing"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: class_ty,
        }],
        ret: TypeId::from_raw(0),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: class_ty,

                    span: Span::default(),
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("missing"),
                ty: TypeId::from_raw(0),

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(0),
        "unknown field keeps the placeholder and a diagnostic is emitted, not a wrong resolve"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0010"),
        "P0010 must be reported for an unknown field, diagnostics: {:?}",
        cx.diagnostics()
    );
}
