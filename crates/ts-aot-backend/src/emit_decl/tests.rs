use super::ident::sanitize_ident;
use super::types::emit_type_id;
use super::{emit_decls, emit_function, emit_global};
use crate::BackendError;
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    BinaryOp, FunctionEffects, FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl,
    MirFunctionDecl, MirGlobalDecl, MirLocalDecl, MirParam, MirPlace, MirPlaceBase, MirProgram,
    MirStmt, MirStructDecl,
};

fn empty_func(name: &str) -> MirFunctionDecl {
    MirFunctionDecl {
        id: FunctionId::from_raw(0),
        name: Atom::from(name),
        export_name: None,
        params: Vec::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: MirBody::default(),
        kind: FunctionKind::Plain,
        effects: FunctionEffects::default(),
    }
}

#[test]
fn empty_program_emits_no_decls() {
    let prog = MirProgram::new(ModuleId::from_raw(0));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    assert!(tokens.is_empty());
}

#[test]
fn plain_function_emits_fn_signature() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(empty_func("greet")));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("fn greet"), "got: {s}");
    assert!(s.contains("->"), "expected ret arrow, got: {s}");
}

#[test]
fn exported_function_emits_pub_keyword() {
    let mut f = empty_func("render");
    f.export_name = Some("render".to_owned());
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub "), "expected `pub` prefix, got: {s}");
}

#[test]
fn private_function_omits_pub_keyword() {
    let tokens =
        emit_function(&empty_func("internal"), &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("pub "),
        "private function should not have `pub`, got: {s}"
    );
}

#[test]
fn async_function_emits_async_keyword() {
    let mut f = empty_func("fetch_data");
    f.effects.is_async = true;
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("async fn"), "got: {s}");
}

#[test]
fn function_body_return_emits_expression() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let mut f = empty_func("answer");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 42,
                ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("return 42"), "got: {s}");
    assert!(!s.contains("unimplemented"), "got: {s}");
}

#[test]
fn function_body_let_binary_and_return_emits_statements() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let local = LocalId::from_raw(0);
    let mut f = empty_func("sum");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("total"),
            ty: int_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local,
                    ty: int_ty,
                    init: Some(MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Int {
                            value: 1,
                            ty: int_ty,
                        }),
                        right: Box::new(MirExpr::Int {
                            value: 2,
                            ty: int_ty,
                        }),
                        ty: int_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::Local(local))),
            ],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("let total : i32 = (1 + 2)"), "got: {s}");
    assert!(s.contains("return total"), "got: {s}");
}

#[test]
fn method_kind_emits_self_param() {
    let mut f = empty_func("method");
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: LocalId::from_raw(0),
    };
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("self"), "method must emit `self`, got: {s}");
}

#[test]
fn method_kind_omits_synthetic_this_param() {
    let mut types = TypeTable::new();
    let number_ty = types.intern(&Type::I32);
    let mut f = empty_func("method");
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: LocalId::from_raw(0),
    };
    f.params = vec![
        MirParam {
            id: LocalId::from_raw(0),
            name: Atom::from("this"),
            ty: number_ty,
        },
        MirParam {
            id: LocalId::from_raw(1),
            name: Atom::from("value"),
            ty: number_ty,
        },
    ];
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("self"),
        "method must emit Rust receiver, got: {s}"
    );
    assert!(
        !s.contains("this :"),
        "method signature must hide synthetic receiver param, got: {s}"
    );
    assert!(s.contains("value : i32"), "expected value param, got: {s}");
}

#[test]
fn struct_decl_emits_struct_keyword() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::from("Point"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Point"), "got: {s}");
}

#[test]
fn struct_with_fields_emits_field_list() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: StructId::from_raw(0),
        name: Atom::from("Rect"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("width"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("height"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Private,
            },
        ],
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Rect"), "got: {s}");
    assert!(s.contains("width :"), "expected field width, got: {s}");
    assert!(s.contains("height :"), "expected field height, got: {s}");
    assert!(
        s.contains("pub width"),
        "expected `pub` on public field, got: {s}"
    );
    assert!(
        !s.contains("pub height"),
        "private field must not have `pub`, got: {s}"
    );
}

#[test]
fn struct_type_reference_uses_declared_struct_name() {
    let struct_id = StructId::from_raw(7);
    let mut types = TypeTable::new();
    let point_ty = types.intern(&Type::Struct { id: struct_id });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    let mut f = empty_func("make_point");
    f.ret = point_ty;
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("pub struct Point"), "got: {s}");
    assert!(s.contains("-> Point"), "got: {s}");
    assert!(!s.contains("__struct7"), "got: {s}");
}

#[test]
fn global_without_init_returns_not_implemented() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("counter"),
        ty: TypeId::from_raw(0),
        mutable: false,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let err = emit_decls(&prog, &TypeTable::new()).expect_err("global init is required");
    assert_eq!(err, BackendError::NotImplemented);
}

#[test]
fn global_with_non_const_init_returns_not_implemented() {
    let err = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Global(Atom::from("other"))),
        },
        &TypeTable::new(),
    )
    .expect_err("non-const global init must not emit invalid static initializer");
    assert_eq!(err, BackendError::NotImplemented);
}

#[test]
fn global_with_const_int_init_emits_initializer() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 42,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.contains("= 42"), "got: {s}");
    assert!(!s.contains("Default :: default"), "got: {s}");
    assert!(!s.contains("unimplemented"), "got: {s}");
}

#[test]
fn public_global_emits_pub_from_visibility_without_export_name() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static counter"), "got: {s}");
}

#[test]
fn private_global_omits_pub_from_visibility() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("secret"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Private,
            export_name: Some("secret".to_owned()),
            init: Some(MirExpr::Bool(true)),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("static secret"), "got: {s}");
}

#[test]
fn mutable_global_emits_mut_from_flag() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: true,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static mut counter"), "got: {s}");
}

#[test]
fn immutable_global_omits_mut_from_flag() {
    let tokens = emit_global(
        &MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: Some(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
        },
        &TypeTable::new(),
    )
    .expect("const global init should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub static counter"), "got: {s}");
    assert!(!s.contains("static mut"), "got: {s}");
}

#[test]
fn sanitize_ident_replaces_dash_with_underscore() {
    assert_eq!(sanitize_ident("foo-bar"), "foo_bar");
}

#[test]
fn sanitize_ident_prefixes_digit_start() {
    assert_eq!(sanitize_ident("7greet"), "_7greet");
}

#[test]
fn sanitize_ident_appends_underscore_to_keyword() {
    assert_eq!(sanitize_ident("type"), "type_");
    assert_eq!(sanitize_ident("fn"), "fn_");
    assert_eq!(sanitize_ident("try"), "try_");
    assert_eq!(sanitize_ident("gen"), "gen_");
}

#[test]
fn emit_type_resolves_primitives() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let bool_id = types.intern(&Type::Bool);
    let tokens = emit_type_id(i32_id, &types);
    assert_eq!(tokens.to_string(), "i32");
    let tokens = emit_type_id(bool_id, &types);
    assert_eq!(tokens.to_string(), "bool");
}

#[test]
fn emit_type_for_unknown_id_emits_placeholder() {
    let types = TypeTable::new();
    let tokens = emit_type_id(TypeId::from_raw(42), &types);
    assert!(tokens.to_string().contains("__ty42"), "got: {tokens}");
}

#[test]
fn emit_type_optional_resolves_inner_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let opt_id = types.intern(&Type::Optional { inner: i32_id });
    let tokens = emit_type_id(opt_id, &types);
    assert_eq!(tokens.to_string(), "Option < i32 >");
}

#[test]
fn emit_type_array_resolves_element_via_table() {
    let mut types = TypeTable::new();
    let str_id = types.intern(&Type::String);
    let arr_id = types.intern(&Type::Array { element: str_id });
    let tokens = emit_type_id(arr_id, &types);
    assert_eq!(tokens.to_string(), "Vec < String >");
}

#[test]
fn emit_type_result_resolves_ok_and_err_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let str_id = types.intern(&Type::String);
    let res_id = types.intern(&Type::Result {
        ok: i32_id,
        err: str_id,
    });
    let tokens = emit_type_id(res_id, &types);
    assert_eq!(tokens.to_string(), "Result < i32 , String >");
}

#[test]
fn function_returning_result_emits_result_type_with_ok_and_err_stmts() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let res_ty = types.intern(&Type::Result {
        ok: int_ty,
        err: str_ty,
    });

    let mut f = empty_func("try_get");
    f.ret = res_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::ReturnResultErr {
                    error: MirExpr::String {
                        id: Atom::from("oops"),
                        ty: str_ty,
                    },
                    err_ty: str_ty,
                },
                MirStmt::Return(Some(MirExpr::ResultOk {
                    value: Box::new(MirExpr::Int {
                        value: 42,
                        ty: int_ty,
                    }),
                    ty: res_ty,
                })),
            ],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 , String >"),
        "ret type must be Result<i32, String>, got: {s}"
    );
    assert!(
        s.contains("Err (") && s.contains("\"oops\""),
        "ReturnResultErr must emit `Err(...)` containing the error, got: {s}"
    );
    assert!(
        s.contains("Ok (42)"),
        "ResultOk must emit `Ok(42)`, got: {s}"
    );
    assert!(
        !s.contains("-> ()"),
        "Result ret must not fall back to unit, got: {s}"
    );
}

#[test]
fn field_access_resolves_name_via_base_struct_id() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);
    let size_id = StructId::from_raw(1);
    let point_ty = types.intern(&Type::Struct { id: point_id });
    let size_ty = types.intern(&Type::Struct { id: size_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: size_id,
        name: Atom::from("Size"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("width"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let p_local = LocalId::from_raw(0);
    let s_local = LocalId::from_raw(1);
    let mut f = empty_func("read");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: p_local,
                name: Atom::from("p"),
                ty: point_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Binary {
                op: BinaryOp::Add,
                left: Box::new(MirExpr::Field {
                    base: Box::new(MirExpr::Local(p_local)),
                    field: FieldId::from_raw(0),
                    ty: int_ty,
                }),
                right: Box::new(MirExpr::Field {
                    base: Box::new(MirExpr::Local(s_local)),
                    field: FieldId::from_raw(0),
                    ty: int_ty,
                }),
                ty: int_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("p . x"), "expected `p.x`, got: {s}");
    assert!(s.contains("s . width"), "expected `s.width`, got: {s}");
    assert!(
        !s.contains("__field0"),
        "FieldId(0) must resolve to its struct's real name, got: {s}"
    );
}

#[test]
fn field_assign_resolves_name_via_base_struct_id() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);
    let size_id = StructId::from_raw(1);
    let point_ty = types.intern(&Type::Struct { id: point_id });
    let size_ty = types.intern(&Type::Struct { id: size_id });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: size_id,
        name: Atom::from("Size"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("width"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let p_local = LocalId::from_raw(0);
    let s_local = LocalId::from_raw(1);
    let mut f = empty_func("touch");
    f.ret = TypeId::from_raw(0);
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: p_local,
                name: Atom::from("p"),
                ty: point_ty,
                mutable: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
            },
        ],
        block: MirBlock {
            stmts: vec![
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(p_local)),
                        field: FieldId::from_raw(0),
                        ty: int_ty,
                    },
                    value: MirExpr::Int {
                        value: 1,
                        ty: int_ty,
                    },
                },
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(s_local)),
                        field: FieldId::from_raw(0),
                        ty: int_ty,
                    },
                    value: MirExpr::Int {
                        value: 2,
                        ty: int_ty,
                    },
                },
                MirStmt::Return(None),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("p . x = 1"), "expected `p.x = 1`, got: {s}");
    assert!(
        s.contains("s . width = 2"),
        "expected `s.width = 2`, got: {s}"
    );
    assert!(
        !s.contains("__field0"),
        "FieldId(0) must resolve to its struct's real name, got: {s}"
    );
}

#[test]
fn field_access_on_non_struct_returns_not_implemented() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);

    let point_id = StructId::from_raw(0);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: int_ty,
            mutable: false,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let local = LocalId::from_raw(0);
    let mut f = empty_func("bad");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("n"),
            ty: int_ty,
            mutable: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Field {
                base: Box::new(MirExpr::Local(local)),
                field: FieldId::from_raw(0),
                ty: int_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let err = emit_decls(&prog, &types).expect_err("field access on non-struct must fail");
    assert_eq!(err, BackendError::NotImplemented);
}
