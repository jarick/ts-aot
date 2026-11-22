use super::super::ctx::{EmitCtx, EmitEnv};
use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirGlobalDecl, MirLocalDecl, MirParam,
    MirPlace, MirProgram, MirStmt, MirStructDecl, RuntimeOp,
};
use ts_aot_test_support::check_generated_crate;

#[test]
fn float_nan_emits_f64_nan_literal() {
    let mut func = empty_func("nan_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::NAN,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: NAN"),
        "NaN literal must emit f64::NAN, got: {s}"
    );
}

#[test]
fn float_positive_infinity_emits_f64_infinity_literal() {
    let mut func = empty_func("pos_inf_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::INFINITY,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: INFINITY"),
        "+Infinity must emit f64::INFINITY, got: {s}"
    );
}

#[test]
fn float_negative_infinity_emits_f64_neg_infinity_literal() {
    let mut func = empty_func("neg_inf_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: f64::NEG_INFINITY,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("f64 :: NEG_INFINITY"),
        "-Infinity must emit f64::NEG_INFINITY, got: {s}"
    );
}

#[test]
fn float_finite_still_uses_unsuffixed_literal() {
    let mut func = empty_func("finite_test");
    let f64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: 3.5,
                ty: f64_ty,
            }))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("3.5") && !s.contains("f64 ::"),
        "finite float 3.5 must emit literal 3.5 (not f64::NAN/INFINITY), got: {s}"
    );
}

#[test]
fn const_default_array_in_function_context_uses_arena_vec() {
    let mut types = TypeTable::new();
    let elem = types.intern(&Type::I64);
    let arr = types.intern(&Type::Array { element: elem });
    let prog = MirProgram::new(ModuleId::from_raw(0));
    let ctx = EmitCtx::build(&prog, &types);
    let emit_env = EmitEnv {
        types: &types,
        program: &prog,
    };
    let mut budget = 64;
    let tokens = super::super::emit_const_default(
        arr,
        &ctx,
        &emit_env,
        0,
        &mut budget,
        super::super::types::LifetimeMode::Full,
    )
    .expect("arena array const default must emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains("ts_aot_runtime::bumpalo::collections::Vec::new_in(__ctx.arena)"),
        "an array const default in a function context must allocate in the arena \
         (`ts_aot_runtime::bumpalo::collections::Vec::new_in(__ctx.arena)`), not a std Vec; got: {norm}"
    );
    let source = format!(
        "pub fn array_default<'a>(__ctx: &ts_aot_runtime::Ctx<'a>) \
         -> ts_aot_runtime::bumpalo::collections::Vec<'a, i64> {{ {tokens} }}"
    );
    check_generated_crate("generated_check_const_default", &source);
}

#[test]
fn array_typed_global_is_stored_owned() {
    let mut types = TypeTable::new();
    let elem = types.intern(&Type::I64);
    let arr = types.intern(&Type::Array { element: elem });
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("xs"),
        ty: arr,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let tokens = emit_decls(&prog, &types)
        .expect("an array-typed global is stored in its owned form")
        .to_string();
    assert!(
        tokens.contains(":: std :: vec :: Vec < i64 >")
            && tokens.contains(":: std :: vec :: Vec :: new ()"),
        "the array global must be declared as an owned `std::vec::Vec`; got: {tokens}"
    );
}

#[test]
fn string_index_passes_ctx_to_char_at() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let i64_ty = types.intern(&Type::I64);
    let param = LocalId::from_raw(0);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let mut f = empty_func("at");
    f.ret = string_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("s"),
        ty: string_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Index {
                base: Box::new(MirExpr::Local(param)),
                index: Box::new(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                }),
                ty: string_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("string indexing must emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains("__ts_aot_string_char_at(__ctx,&s,0asi64"),
        "string indexing must pass `__ctx` as the first argument, matching the runtime \
         helper signature `__ts_aot_string_char_at(ctx, s, idx)` and the \
         RuntimeOp::StringCharAt path; got: {norm}"
    );
}

#[test]
fn object_keys_struct_literal_emits_arena_array_with_ctx() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let struct_id = StructId::from_raw(0);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let keys_ty = types.intern(&Type::Array { element: string_ty });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("y"),
                ty: i64_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
        ],
        methods: Vec::new(),
    }));

    let param = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("keys_of");
    f.ret = keys_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("p"),
        ty: struct_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("keys"),
            ty: keys_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectKeys,
                args: vec![MirExpr::Local(param)],
                dest: Some(dest),
                ty: keys_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("Object.keys on a struct literal must emit");
    let norm: String = tokens
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        norm.contains(
            "__ts_aot_array_create::<ts_aot_runtime::JsString>(__ctx,ts_aot_runtime::bumpalo::vec![in__ctx.arena;"
        ),
        "Object.keys over a struct literal must build the keys array with `__ctx` and an \
         arena-allocated `ts_aot_runtime::bumpalo::vec![in __ctx.arena; ...]`, matching the canonical \
         ArrayCreate emitter (not a std `vec!`); got: {norm}"
    );
}

#[test]
fn object_keys_orders_canonical_integer_keys_before_string_keys() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let string_ty = types.intern(&Type::String);
    let struct_id = StructId::from_raw(0);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let keys_ty = types.intern(&Type::Array { element: string_ty });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Bag"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("b"),
                ty: i64_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("10"),
                ty: i64_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(2),
                name: Atom::from("2"),
                ty: i64_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
        ],
        methods: Vec::new(),
    }));

    let param = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("keys_of");
    f.ret = keys_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("bag"),
        ty: struct_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("keys"),
            ty: keys_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectKeys,
                args: vec![MirExpr::Local(param)],
                dest: Some(dest),
                ty: keys_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("Object.keys over a struct must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let two = norm
        .find("JsString::from(\"2\")")
        .expect("the canonical index key 2 must be emitted");
    let ten = norm
        .find("JsString::from(\"10\")")
        .expect("the canonical index key 10 must be emitted");
    let b = norm
        .find("JsString::from(\"b\")")
        .expect("the string key b must be emitted");
    assert!(
        two < ten && ten < b,
        "Object.keys must emit canonical array-index keys in ascending numeric order before \
         string keys in declaration order; got: {s}"
    );
}

#[test]
fn weak_map_set_evaluates_a_call_valued_receiver_once() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let key_struct_id = StructId::from_raw(4);
    let key_ty = types.intern(&Type::Struct { id: key_struct_id });
    let weak_map_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: i64_ty,
    });
    let get_map_id = FunctionId::from_raw(20);
    let get_map_local = LocalId::from_raw(0);
    let key_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);

    let mut get_map = empty_func("get_map");
    get_map.id = get_map_id;
    get_map.ret = weak_map_ty;
    get_map.body = MirBody {
        locals: vec![MirLocalDecl {
            id: get_map_local,
            name: Atom::from(""),
            ty: weak_map_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapNew,
                    args: Vec::new(),
                    dest: Some(get_map_local),
                    ty: weak_map_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(get_map_local))),
            ],
        },
    };

    let mut store = empty_func("store");
    store.ret = weak_map_ty;
    store.params = vec![
        MirParam {
            id: key_param,
            name: Atom::from("k"),
            ty: key_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: i64_ty,
        },
    ];
    store.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: weak_map_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapSet,
                    args: vec![
                        MirExpr::Call {
                            callee: get_map_id,
                            args: Vec::new(),
                            ty: weak_map_ty,
                        },
                        MirExpr::Local(key_param),
                        MirExpr::Local(value_param),
                    ],
                    dest: Some(dest),
                    ty: weak_map_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: key_struct_id,
        name: Atom::from("K"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("v"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(get_map));
    prog.push_decl(MirDecl::Function(store));

    let tokens = emit_decls(&prog, &types).expect("a call-valued WeakMap.set receiver must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(
        norm.matches("get_map(__ctx)").count(),
        1,
        "`get_map().set(k, v)` evaluates its receiver once: the receiver must be bound before the \
         runtime call and the returned handle must be that same binding, so `get_map(__ctx)` must \
         be emitted exactly once; got: {s}"
    );
    let bind = norm
        .find("let__ts_aot_weakmap_receiver=")
        .expect("the receiver must be bound to a hoisted local");
    let set = norm
        .find("__ts_aot_weak_map_set")
        .expect("the runtime set call must be emitted");
    let returned = norm
        .rfind("__ts_aot_weakmap_receiver.clone()")
        .expect("the returned handle must be the bound receiver");
    assert!(
        bind < set && set < returned,
        "the receiver binding must precede the runtime call and the returned handle must reuse it; \
         got: {s}"
    );
    check_generated_crate("weakmap_receiver_once", &s);
}

#[test]
fn weak_map_set_refreshes_liveness_after_a_shared_key_reassignment() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let void_ty = types.intern(&Type::Void);
    let key_struct_id = StructId::from_raw(5);
    let key_ty = types.intern(&Type::Struct { id: key_struct_id });
    let weak_map_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: i64_ty,
    });
    let fn_ty = types.intern(&Type::Fn {
        params: Vec::new(),
        optional: Vec::new(),
        ret: void_ty,
        err: None,
    });
    let wm_param = LocalId::from_raw(0);
    let new_key_param = LocalId::from_raw(1);
    let key_local = LocalId::from_raw(2);
    let closure_local = LocalId::from_raw(3);

    let mut f = empty_func("reassign_shared_key");
    f.ret = i64_ty;
    f.params = vec![
        MirParam {
            id: wm_param,
            name: Atom::from("wm"),
            ty: weak_map_ty,
        },
        MirParam {
            id: new_key_param,
            name: Atom::from("new_k"),
            ty: key_ty,
        },
    ];
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: key_local,
                name: Atom::from("k"),
                ty: key_ty,
                mutable: true,
                narrowed: false,
            },
            MirLocalDecl {
                id: closure_local,
                name: Atom::from("g"),
                ty: fn_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: key_local,
                    ty: key_ty,
                    init: Some(MirExpr::Local(new_key_param)),
                    mutable: true,
                },
                MirStmt::Let {
                    local: closure_local,
                    ty: fn_ty,
                    init: Some(MirExpr::Closure {
                        id: 0,
                        params: Vec::new(),
                        captures: vec![key_local],
                        locals: Vec::new(),
                        body: MirBlock { stmts: Vec::new() },
                        ret_ty: void_ty,
                        fn_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapSet,
                    args: vec![
                        MirExpr::Local(wm_param),
                        MirExpr::Local(key_local),
                        MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                    ],
                    dest: None,
                    ty: i64_ty,
                    target_ty: None,
                },
                MirStmt::Assign {
                    target: MirPlace::Local { id: key_local },
                    value: MirExpr::Local(new_key_param),
                },
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapSet,
                    args: vec![
                        MirExpr::Local(wm_param),
                        MirExpr::Local(key_local),
                        MirExpr::Int {
                            value: 2,
                            ty: i64_ty,
                        },
                    ],
                    dest: None,
                    ty: i64_ty,
                    target_ty: None,
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
        id: key_struct_id,
        name: Atom::from("K"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("v"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("a captured WeakMap key local must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let write = norm
        .find("borrow_mut()=Some(")
        .expect("the captured key local must be written through its shared cell");
    let reset = norm
        .find("*__env_0.__wm_liveness_key_2.borrow_mut()=std::rc::Rc::new(())")
        .expect(
            "reassigning a captured WeakMap key local must install a fresh liveness token in its \
             shared environment cell, because the old key identity no longer names the object the \
             local holds",
        );
    let second_set = norm
        .rfind("__ts_aot_weak_map_set")
        .expect("both runtime set calls must be emitted");
    assert!(
        write < reset && reset < second_set,
        "the liveness refresh must follow the shared write and precede the next WeakMap.set; got: \
         {s}"
    );
    check_generated_crate("weakmap_shared_key_refresh", &s);
}

#[test]
fn array_is_array_false_evaluates_its_operand_for_side_effects() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let mark_id = FunctionId::from_raw(30);
    let dest = LocalId::from_raw(0);

    let mut mark = empty_func("mark");
    mark.id = mark_id;
    mark.ret = i64_ty;
    mark.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 1,
                ty: i64_ty,
            }))],
        },
    };

    let mut f = empty_func("probe");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("v"),
            ty: bool_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayIsArrayFalse,
                    args: vec![MirExpr::Call {
                        callee: mark_id,
                        args: Vec::new(),
                        ty: i64_ty,
                    }],
                    dest: Some(dest),
                    ty: bool_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(mark));
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("Array.isArray(mark()) must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let operand = norm.find("mark(__ctx)").expect(
        "`Array.isArray(mark())` evaluates its argument even when the static result is `false`, so \
         the operand call must still be emitted",
    );
    let helper = norm
        .find("__ts_aot_array_is_array_false")
        .expect("the false-result helper must be emitted");
    assert!(
        operand < helper,
        "the operand must be evaluated before the false-result helper is produced; got: {s}"
    );
    check_generated_crate("array_is_array_false_operand", &s);
}

#[test]
fn array_set_stages_the_index_whenever_the_value_is_staged() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let change_id = FunctionId::from_raw(31);
    let arr_param = LocalId::from_raw(0);
    let idx_param = LocalId::from_raw(1);

    let mut change = empty_func("change_index");
    change.id = change_id;
    change.ret = i64_ty;
    change.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 1,
                ty: i64_ty,
            }))],
        },
    };

    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![
        MirParam {
            id: arr_param,
            name: Atom::from("arr"),
            ty: arr_ty,
        },
        MirParam {
            id: idx_param,
            name: Atom::from("i"),
            ty: i64_ty,
        },
    ];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArraySet,
                args: vec![
                    MirExpr::Local(arr_param),
                    MirExpr::Local(idx_param),
                    MirExpr::Call {
                        callee: change_id,
                        args: Vec::new(),
                        ty: i64_ty,
                    },
                ],
                dest: None,
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(change));
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("ArraySet with a call value must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let index = norm.find("let__ts_index=").expect(
        "the index operand must be snapshotted whenever the value operand is staged, so a value \
         that rewrites the index local cannot retarget the store",
    );
    let value = norm
        .find("let__ts_value=")
        .expect("the staged value binding must be emitted");
    assert!(
        index < value,
        "the index must be bound before the staged value expression runs; got: {s}"
    );
    check_generated_crate("array_set_index_staging", &s);
}
