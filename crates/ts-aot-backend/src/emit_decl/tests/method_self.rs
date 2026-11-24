use super::super::{emit_decls, emit_function};
use super::{empty_func, signature_fragment};
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam,
    MirPlace, MirPlaceBase, MirProgram, MirStmt, MirStructDecl, RuntimeOp,
};

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
fn method_writing_self_field_emits_shared_self() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(7);
    let self_id = LocalId::from_raw(0);
    let field_id = FieldId::from_raw(0);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("set_x");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner,
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: types.intern(&Type::Struct { id: owner }),
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_id)),
                    field: field_id,
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 1,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "set_x");
    assert!(
        sig.contains("& self ,"),
        "a method that writes to a self field must take a shared receiver, because the write goes \
         through the field's `RefCell`; got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains("mut self"),
        "the receiver must never be borrowed mutably; got: `{sig}`"
    );
}

#[test]
fn method_consuming_self_via_generator_next_emits_shared_self() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let self_id = LocalId::from_raw(0);
    let temp = LocalId::from_raw(1);

    let mut f = empty_func("drive");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: i64_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Local(self_id)],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive");
    assert!(
        sig.contains("& self ,"),
        "a method that mutably borrows its receiver as a whole must still take a shared receiver, \
         because every field is behind its own `RefCell`; got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains("mut self"),
        "the receiver must never be borrowed mutably; got: `{sig}`"
    );
}

#[test]
fn method_reading_self_only_emits_immutable_self() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let self_id = LocalId::from_raw(0);
    let result = LocalId::from_raw(1);
    let mut f = empty_func("get_x");
    f.ret = TypeId::from_raw(0);
    f.kind = FunctionKind::Method {
        owner: StructId::from_raw(0),
        self_param: self_id,
    };
    f.params = vec![MirParam {
        id: self_id,
        name: Atom::from("self"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: result,
            name: Atom::from("result"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local { id: result },
                value: MirExpr::Local(self_id),
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "get_x");
    assert!(
        sig.contains("self ,") && !sig.contains("mut self"),
        "method that only reads self must emit `self,` (no `mut`), got signature fragment: `{sig}`"
    );
}

#[test]
fn class_fields_and_initializers_use_shared_ref_cell_storage() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(9);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let x_field = FieldId::from_raw(0);
    let y_field = FieldId::from_raw(1);

    let mut build = empty_func("build");
    build.ret = struct_ty;
    build.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::StructLiteral {
                struct_id: owner,
                fields: vec![(
                    x_field,
                    MirExpr::Int {
                        value: 1,
                        ty: i32_ty,
                    },
                )],
                ty: struct_ty,
            }))],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Point"),
        fields: vec![
            MirFieldDecl {
                id: x_field,
                name: Atom::from("x"),
                ty: i32_ty,
                mutable: true,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: y_field,
                name: Atom::from("y"),
                ty: i32_ty,
                mutable: true,
                visibility: Visibility::Public,
            },
        ],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(build));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains("pubx:::std::rc::Rc<::core::cell::RefCell<i32>>")
            && norm.contains("puby:::std::rc::Rc<::core::cell::RefCell<i32>>"),
        "every class field must be stored as `Rc<RefCell<..>>` so a clone of the instance shares \
         the field storage; got: {norm}"
    );
    assert!(
        norm.contains("Point{x:::std::rc::Rc::new(::core::cell::RefCell::new(1))"),
        "a listed field initializer must wrap the value in `Rc::new(RefCell::new(..))`; got: \
         {norm}"
    );
    assert!(
        norm.contains("y:::std::rc::Rc::new(::core::cell::RefCell::new(0))"),
        "a field left out of the literal must still be initialized through \
         `Rc::new(RefCell::new(..))`; got: {norm}"
    );
}

#[test]
fn mutating_method_call_takes_a_shared_receiver_without_a_write_back_guard() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(11);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let fn_ty = types.intern(&Type::Fn {
        params: Vec::new(),
        optional: Vec::new(),
        ret: i32_ty,
        err: None,
    });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let closure_local = LocalId::from_raw(1);
    let caller_param = LocalId::from_raw(0);

    let mut bump = empty_func("bump");
    bump.id = FunctionId::from_raw(1);
    bump.kind = FunctionKind::Method { owner, self_param };
    bump.params = vec![MirParam {
        id: self_param,
        name: Atom::from("self"),
        ty: struct_ty,
    }];
    bump.body = MirBody {
        locals: vec![MirLocalDecl {
            id: closure_local,
            name: Atom::from("f"),
            ty: fn_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: closure_local,
                    ty: fn_ty,
                    mutable: false,
                    init: Some(MirExpr::Closure {
                        id: 0,
                        params: Vec::new(),
                        captures: vec![self_param],
                        locals: Vec::new(),
                        body: MirBlock { stmts: Vec::new() },
                        ret_ty: i32_ty,
                        fn_ty,
                    }),
                },
                MirStmt::Assign {
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(self_param)),
                        field: field_id,
                        ty: i32_ty,
                    },
                    value: MirExpr::Int {
                        value: 1,
                        ty: i32_ty,
                    },
                },
            ],
        },
    };

    let mut got = empty_func("got");
    got.id = FunctionId::from_raw(2);
    got.ret = i32_ty;
    got.params = vec![MirParam {
        id: caller_param,
        name: Atom::from("c"),
        ty: struct_ty,
    }];
    got.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Call {
                callee: FunctionId::from_raw(1),
                args: vec![MirExpr::Local(caller_param)],
                ty: i32_ty,
            })],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Counter"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("n"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![bump],
    }));
    prog.push_decl(MirDecl::Function(got));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let sig = signature_fragment(&s, "bump");
    assert!(
        sig.contains("& self ,") && !sig.contains("mut self"),
        "a mutating method must take `&self`; got signature fragment: `{sig}`"
    );
    assert!(
        norm.contains("Counter::bump(&(c)"),
        "the call site must pass a shared borrow of the receiver instead of `&mut c`; got: {norm}"
    );
    assert!(
        !norm.contains("&mut(c)"),
        "no call site may take `&mut` of a class receiver; got: {norm}"
    );
    assert!(
        !norm.contains("WriteBack") && !norm.contains("__self_write_back"),
        "the snapshot receiver and its write-back guard must be gone; got: {norm}"
    );
    assert!(
        norm.contains("Some(self.clone())"),
        "a captured `this` must be a clone of the instance whose `Rc` fields alias the original \
         storage; got: {norm}"
    );
    assert!(
        norm.contains(".n.borrow_mut())=1"),
        "field mutation must still go through the field's `RefCell`, here through the capture \
         environment; got: {norm}"
    );
}

#[test]
fn escaping_this_capture_reports_the_frame_lifetime_consequence() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let closure_local = LocalId::from_raw(1);
    let field_id = FieldId::from_raw(0);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let mut leak = empty_func("leak_this");
    leak.id = FunctionId::from_raw(1);
    leak.kind = FunctionKind::Method { owner, self_param };
    leak.ret = i32_ty;
    leak.params = vec![MirParam {
        id: self_param,
        name: Atom::from("self"),
        ty: struct_ty,
    }];
    leak.body = MirBody {
        locals: vec![MirLocalDecl {
            id: closure_local,
            name: Atom::from("f"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: closure_local,
                    ty: i32_ty,
                    mutable: false,
                    init: Some(MirExpr::Closure {
                        id: 0,
                        params: Vec::new(),
                        captures: vec![self_param],
                        locals: Vec::new(),
                        body: MirBlock { stmts: Vec::new() },
                        ret_ty: i32_ty,
                        fn_ty: i32_ty,
                    }),
                },
                MirStmt::Assign {
                    target: MirPlace::Global(Atom::from("leak")),
                    value: MirExpr::Local(closure_local),
                },
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Counter"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("n"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![leak],
    }));
    let err = emit_decls(&prog, &types)
        .expect_err("a closure that captures `this` and is stored in a global escapes");
    assert_eq!(err.code(), "E0513");
    let message = err.message();
    assert!(
        message.contains("the capture would outlive the method frame"),
        "the diagnostic must state the frame-lifetime consequence, that the capture outlives the \
         method frame; got: {message}"
    );
    assert!(
        message.contains("sharing the receiver's field cells"),
        "the diagnostic must state the shared-field-cells consequence, that the capture shares the \
         receiver's field cells with the method; got: {message}"
    );
    assert!(
        !message.contains("snapshot"),
        "the captured `this` is a clone that shares the receiver's field cells, so the diagnostic \
         must not claim a snapshot copy; got: {message}"
    );
}
