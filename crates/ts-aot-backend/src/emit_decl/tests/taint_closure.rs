use super::super::{emit_decls, emit_function};
use super::empty_func;
use ts_aot_core::{
    Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, MirStructDecl,
};

#[test]
fn closure_param_for_of_generator_iterable_emits_mut() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let outer_param = LocalId::from_raw(0);
    let closure_param = LocalId::from_raw(10);
    let item = LocalId::from_raw(11);
    let mut f = empty_func("drain_via_closure");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: outer_param,
        name: Atom::from("g"),
        ty: gen_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: vec![MirParam {
                    id: closure_param,
                    name: Atom::from("g"),
                    ty: gen_ty,
                }],
                captures: Vec::new(),
                locals: vec![MirLocalDecl {
                    id: item,
                    name: Atom::from("x"),
                    ty: i64_ty,
                    mutable: false,
                    narrowed: false,
                }],
                body: MirBlock {
                    stmts: vec![MirStmt::ForOf {
                        item,
                        iterable: MirExpr::Local(closure_param),
                        iter_ty: gen_ty,
                        body: MirBlock {
                            stmts: vec![MirStmt::Expr(MirExpr::Local(item))],
                        },
                    }],
                },
                ret_ty: i64_ty,
                fn_ty: gen_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let closure_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        closure_sig.trim_start().starts_with("mut "),
        "closure param that is the iterable of a `for-of` over a Generator must emit \
         `mut g` in the move-closure signature (regression: closure body walk did not mark \
         the closure-local iterable as mutated when the closure was emitted); got \
         closure signature fragment: `{closure_sig}`"
    );
}

#[test]
fn closure_param_assigned_in_if_body_emits_mut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("x"),
                    ty: int_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![
                        MirStmt::If {
                            cond: MirExpr::Bool(true),
                            then_block: MirBlock {
                                stmts: vec![MirStmt::Assign {
                                    target: ts_aot_ir_mir::MirPlace::Local { id: param_id },
                                    value: MirExpr::Int {
                                        value: 1,
                                        ty: int_ty,
                                    },
                                }],
                            },
                            else_block: None,
                        },
                        MirStmt::Return(Some(MirExpr::Local(param_id))),
                    ],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("mut x :"),
        "closure param assigned inside an `if` then-block must emit `mut x : i32`; got: {s}"
    );
}

#[test]
fn closure_param_unused_emits_immut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure_pure");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("x"),
                    ty: int_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(param_id)))],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("")
        .trim();
    assert!(
        param_sig.contains("x : i32") && !param_sig.starts_with("mut "),
        "closure param that is not assigned must remain `x : i32` (no `mut`); got: `{param_sig}`"
    );
}

#[test]
fn closure_param_passed_to_array_push_emits_mut() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: int_ty });
    let param_id = LocalId::from_raw(0);
    let mut f = empty_func("make_closure_push");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("arr"),
                    ty: arr_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: ts_aot_ir_mir::RuntimeOp::ArrayPush,
                        args: vec![
                            MirExpr::Local(param_id),
                            MirExpr::Int {
                                value: 42,
                                ty: int_ty,
                            },
                        ],
                        dest: None,
                        ty: TypeId::from_raw(0),
                        target_ty: None,
                    }],
                },
                ret_ty: int_ty,
                fn_ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        param_sig.trim_start().starts_with("mut "),
        "closure param passed as `args[0]` to RuntimeOp::ArrayPush must emit `mut arr : ...` \
         (runtime requires &mut Vec); got: `{param_sig}`"
    );
}

#[test]
fn closure_param_assigned_via_optional_chain_place_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_ty = types.intern(&Type::Optional {
        inner: obj_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let param_id = LocalId::from_raw(0);

    let mut f = empty_func("make_closure_optional_chain");
    f.ret = i32_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: vec![MirParam {
                    id: param_id,
                    name: Atom::from("obj"),
                    ty: obj_ty,
                }],
                captures: Vec::new(),
                locals: Vec::new(),
                body: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Field {
                            base: Box::new(MirPlaceBase::Chain {
                                base: Box::new(MirExpr::OptionalChain {
                                    base: Box::new(MirExpr::Local(param_id)),
                                    ty: opt_ty,
                                }),
                                ty: opt_ty,
                            }),
                            field: FieldId::from_raw(0),
                            ty: i32_ty,
                        },
                        value: MirExpr::Int {
                            value: 0,
                            ty: i32_ty,
                        },
                    }],
                },
                ret_ty: i32_ty,
                fn_ty: i32_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let param_sig = s
        .split("move |")
        .nth(1)
        .and_then(|after_opener| after_opener.split('|').next())
        .unwrap_or("");
    assert!(
        param_sig.trim_start().starts_with("mut "),
        "closure param assigned through MirPlaceBase::Chain over OptionalChain over Local \
         must emit `mut obj : ...` (regression: visit_base for Chain in body.rs lacked the \
         early-insert that collect_written_locals in ctx.rs has via place_base_root_local_id); \
         got: `{param_sig}`"
    );
}

#[test]
fn closure_local_array_assigned_via_array_set_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let arr_local = LocalId::from_raw(10);
    let mut f = empty_func("make_closure_array_set");
    f.ret = i32_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Closure {
                id: 0,
                params: Vec::new(),
                captures: Vec::new(),
                locals: vec![MirLocalDecl {
                    id: arr_local,
                    name: Atom::from("arr"),
                    ty: arr_ty,
                    mutable: false,
                    narrowed: false,
                }],
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Let {
                            local: arr_local,
                            ty: arr_ty,
                            init: None,
                            mutable: false,
                        },
                        MirStmt::Runtime {
                            op: ts_aot_ir_mir::RuntimeOp::ArraySet,
                            args: vec![
                                MirExpr::Local(arr_local),
                                MirExpr::Int {
                                    value: 0,
                                    ty: i32_ty,
                                },
                                MirExpr::Int {
                                    value: 7,
                                    ty: i32_ty,
                                },
                            ],
                            dest: None,
                            ty: TypeId::from_raw(0),
                            target_ty: None,
                        },
                    ],
                },
                ret_ty: i32_ty,
                fn_ty: i32_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("let mut arr"),
        "closure local array passed as `args[0]` to RuntimeOp::ArraySet must be re-registered \
         with `mutable=true` (regression: collect_assigned_locals in body.rs was filtering \
         candidates to closure params only, so mutably-borrowed closure locals stayed \
         `let arr = ...` and the generated Rust required `&mut Vec<T>` on an immutable \
         binding); got tokens: `{s}`"
    );
}

#[test]
fn doubly_nested_closure_reads_the_captured_shadowed_binding() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: Vec::new(),
        optional: Vec::new(),
        ret: i64_ty,
        err: None,
    });
    let outer_x = LocalId::from_raw(0);
    let inner_x = LocalId::from_raw(1);
    let sibling = LocalId::from_raw(2);
    let outer = LocalId::from_raw(3);
    let inner = LocalId::from_raw(4);

    let mut f = empty_func("run");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: outer_x,
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: inner_x,
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: sibling,
                name: Atom::from("sibling"),
                ty: fn_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: outer,
                name: Atom::from("outer"),
                ty: fn_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: outer_x,
                    ty: i64_ty,
                    mutable: false,
                    init: Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }),
                },
                MirStmt::Let {
                    local: inner_x,
                    ty: i64_ty,
                    mutable: false,
                    init: Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }),
                },
                MirStmt::Let {
                    local: sibling,
                    ty: fn_ty,
                    mutable: false,
                    init: Some(MirExpr::Closure {
                        id: 0,
                        params: Vec::new(),
                        captures: vec![inner_x],
                        locals: Vec::new(),
                        body: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 0,
                                ty: i64_ty,
                            }))],
                        },
                        ret_ty: i64_ty,
                        fn_ty,
                    }),
                },
                MirStmt::Let {
                    local: outer,
                    ty: fn_ty,
                    mutable: false,
                    init: Some(MirExpr::Closure {
                        id: 1,
                        params: Vec::new(),
                        captures: vec![outer_x],
                        locals: vec![MirLocalDecl {
                            id: inner,
                            name: Atom::from("inner"),
                            ty: fn_ty,
                            mutable: false,
                            narrowed: false,
                        }],
                        body: MirBlock {
                            stmts: vec![
                                MirStmt::Let {
                                    local: inner,
                                    ty: fn_ty,
                                    mutable: false,
                                    init: Some(MirExpr::Closure {
                                        id: 2,
                                        params: Vec::new(),
                                        captures: Vec::new(),
                                        locals: Vec::new(),
                                        body: MirBlock {
                                            stmts: vec![MirStmt::Return(Some(MirExpr::Global(
                                                Atom::from("x"),
                                            )))],
                                        },
                                        ret_ty: i64_ty,
                                        fn_ty,
                                    }),
                                },
                                MirStmt::Return(Some(MirExpr::Int {
                                    value: 0,
                                    ty: i64_ty,
                                })),
                            ],
                        },
                        ret_ty: i64_ty,
                        fn_ty,
                    }),
                },
                MirStmt::Return(Some(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        s.contains("x_0.get()"),
        "a name-based capture read must resolve to the binding the enclosing closure actually \
         captured, not to the same-named sibling inserted last into the name map; got: {s}"
    );
    assert!(
        !s.contains("x__1.get()"),
        "the doubly nested closure must not read the same-named binding (`x` id 1) it never \
         captured; got: {s}"
    );
}
