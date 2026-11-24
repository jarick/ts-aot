use super::super::emit_decls;
use super::{empty_func, norm};
use crate::BackendError;
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    BinaryOp, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, MirStructDecl,
};

#[test]
fn place_root_global_name_recurses_through_field_index_and_chain() {
    let root =
        |p: &MirPlace| super::super::ctx::place_root_global_name(p).map(|a| a.as_str().to_owned());
    let field_base = MirPlaceBase::Field {
        base: Box::new(MirPlaceBase::Global(Atom::from("g"))),
        field: FieldId::from_raw(0),
        ty: TypeId::from_raw(0),
    };
    let index_base = MirPlaceBase::Index {
        base: Box::new(MirExpr::Global(Atom::from("g"))),
        index: Box::new(MirExpr::Int {
            value: 0,
            ty: TypeId::from_raw(0),
        }),
        ty: TypeId::from_raw(0),
    };
    let chain_base = MirPlaceBase::Chain {
        base: Box::new(MirExpr::OptionalChain {
            base: Box::new(MirExpr::Global(Atom::from("g"))),
            ty: TypeId::from_raw(0),
        }),
        ty: TypeId::from_raw(0),
    };
    for place in [
        MirPlace::Global(Atom::from("g")),
        MirPlace::Field {
            base: Box::new(field_base),
            field: FieldId::from_raw(0),
            ty: TypeId::from_raw(0),
        },
        MirPlace::Index {
            base: Box::new(MirExpr::Global(Atom::from("g"))),
            index: Box::new(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            }),
            ty: TypeId::from_raw(0),
        },
        MirPlace::Field {
            base: Box::new(index_base),
            field: FieldId::from_raw(0),
            ty: TypeId::from_raw(0),
        },
        MirPlace::Field {
            base: Box::new(chain_base),
            field: FieldId::from_raw(0),
            ty: TypeId::from_raw(0),
        },
    ] {
        assert_eq!(
            root(&place),
            Some("g".to_owned()),
            "a place rooted at a global (incl. field/index/optional-chain bases) must be \
             detected so its assignment can be wrapped in `unsafe`, got place: {place:?}"
        );
    }

    let local = MirPlace::Local {
        id: LocalId::from_raw(0),
    };
    assert_eq!(
        root(&local),
        None,
        "a local-rooted place must not be reported as global-rooted"
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
                narrowed: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
                narrowed: false,
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
                narrowed: false,
            },
            MirLocalDecl {
                id: s_local,
                name: Atom::from("s"),
                ty: size_ty,
                mutable: false,
                narrowed: false,
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
    assert!(
        s.contains("(* p . x . borrow_mut ()) = 1"),
        "expected the `p.x` place through its RefCell, got: {s}"
    );
    assert!(
        s.contains("(* s . width . borrow_mut ()) = 2"),
        "expected the `s.width` place through its RefCell, got: {s}"
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
            narrowed: false,
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
    assert_eq!(
        err,
        BackendError::NotImplemented("field access: base is not a struct")
    );
}

#[test]
fn assignment_to_optional_chain_field_emits_is_some_branch() {
    let mut types = TypeTable::new();
    let point = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point });
    let opt = types.intern(&Type::Optional {
        inner: point_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: types.intern(&Type::I64),
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let obj = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = opt;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: obj,
            name: Atom::from("obj"),
            ty: opt,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Chain {
                        base: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(obj)),
                            ty: opt,
                        }),
                        ty: opt,
                    }),
                    field: FieldId::from_raw(0),
                    ty: types.intern(&Type::I64),
                },
                value: MirExpr::Int {
                    value: 42,
                    ty: types.intern(&Type::I64),
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("obj?.x = y must emit");
    let s = tokens.to_string();

    let normalized = norm(&s);
    let guarded = normalized
        .split_once("ifobj.is_some(){")
        .expect("obj?.x = y must emit an `if obj.is_some()` guard")
        .1
        .split_once('}')
        .expect("the `if obj.is_some()` guard must have a body")
        .0;
    assert!(
        guarded.contains("(*obj.as_mut().unwrap().x.borrow_mut())=42;"),
        "MirPlace::Field with MirPlaceBase::Chain + OptionalChain base must unwrap and assign \
         the field through its RefCell inside the `if obj.is_some()` block, not after it (a \
         write after the guard would unwrap `None`). Got: {s}"
    );
}

#[test]
fn nested_field_target_captures_its_base_before_the_value_runs() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let inner_id = StructId::from_raw(1);
    let holder_id = StructId::from_raw(2);
    let inner_ty = types.intern(&Type::Struct { id: inner_id });
    let holder_ty = types.intern(&Type::Struct { id: holder_id });
    let n_field = FieldId::from_raw(0);
    let child_field = FieldId::from_raw(0);
    let replace_id = FunctionId::from_raw(40);
    let holder_param = LocalId::from_raw(0);
    let inner_param = LocalId::from_raw(1);

    let mut replace = empty_func("replace_child");
    replace.id = replace_id;
    replace.ret = i64_ty;
    replace.params = vec![
        MirParam {
            id: holder_param,
            name: Atom::from("h"),
            ty: holder_ty,
        },
        MirParam {
            id: inner_param,
            name: Atom::from("i"),
            ty: inner_ty,
        },
    ];
    replace.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(holder_param)),
                        field: child_field,
                        ty: inner_ty,
                    },
                    value: MirExpr::Local(inner_param),
                },
                MirStmt::Return(Some(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                })),
            ],
        },
    };

    let mut touch = empty_func("touch");
    touch.ret = i64_ty;
    touch.params = vec![
        MirParam {
            id: holder_param,
            name: Atom::from("h"),
            ty: holder_ty,
        },
        MirParam {
            id: inner_param,
            name: Atom::from("i"),
            ty: inner_ty,
        },
    ];
    touch.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Field {
                            base: Box::new(MirPlaceBase::Local(holder_param)),
                            field: child_field,
                            ty: inner_ty,
                        }),
                        field: n_field,
                        ty: i64_ty,
                    },
                    value: MirExpr::Call {
                        callee: replace_id,
                        args: vec![MirExpr::Local(holder_param), MirExpr::Local(inner_param)],
                        ty: i64_ty,
                    },
                },
                MirStmt::Return(Some(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                })),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: inner_id,
        name: Atom::from("Inner"),
        fields: vec![MirFieldDecl {
            id: n_field,
            name: Atom::from("n"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: holder_id,
        name: Atom::from("Holder"),
        fields: vec![MirFieldDecl {
            id: child_field,
            name: Atom::from("child"),
            ty: inner_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(replace));
    prog.push_decl(MirDecl::Function(touch));

    let tokens = emit_decls(&prog, &types).expect("a nested field target must emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn touch").nth(1).unwrap_or(""));
    let capture = body.find("let__ts_field_base=").expect(
        "`h.child.n = f()` must capture the nested base before the right-hand side runs, so the \
             store keeps the child the left-hand side selected",
    );
    let value = body
        .find("replace_child(__ctx")
        .expect("the right-hand side call must be emitted");
    assert!(
        capture < value,
        "the nested field base must be captured before the right-hand side expression runs; got: \
         {body}"
    );
    assert!(
        body.contains("(*__ts_field_base.n.borrow_mut())="),
        "the store must write through the captured base; got: {body}"
    );
    ts_aot_test_support::check_generated_crate("nested_field_base_capture", &s);
}
