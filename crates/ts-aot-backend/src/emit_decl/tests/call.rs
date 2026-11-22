use super::super::{emit_decls, emit_function};
use super::{empty_func, norm, signature_fragment};
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam,
    MirPlace, MirPlaceBase, MirProgram, MirStmt, MirStructDecl,
};

#[test]
fn optional_fn_value_call_dispatches_through_the_fn_call_helper() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });
    let opt_fn_ty = types.intern(&Type::Optional {
        inner: fn_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let opt_i64_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });

    let mut target = empty_func("target_fn");
    target.id = FunctionId::from_raw(0);
    target.ret = i64_ty;
    target.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let callee_local = LocalId::from_raw(0);
    let result = LocalId::from_raw(1);
    let arg = LocalId::from_raw(2);
    let mut caller = empty_func("caller");
    caller.id = FunctionId::from_raw(1);
    caller.ret = opt_i64_ty;
    caller.params = vec![MirParam {
        id: callee_local,
        name: Atom::from("f"),
        ty: opt_fn_ty,
    }];
    caller.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: result,
                name: Atom::from("r"),
                ty: opt_i64_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: arg,
                name: Atom::from("a"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: result,
                    ty: opt_i64_ty,
                    init: Some(MirExpr::IndirectCall {
                        callee: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(callee_local)),
                            ty: opt_fn_ty,
                        }),
                        args: vec![MirExpr::Local(arg)],
                        ty: i64_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::Local(result))),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(target));
    prog.push_decl(MirDecl::Function(caller));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn caller").nth(1).unwrap_or(""));
    assert!(
        body.contains("f.as_ref().map(|__ts_recv|__ts_fn_call_0(__ctx,__ts_recv,a))"),
        "an optional call on a function value must go through the generated `__ts_fn_call_N` \
         helper, because a `__TsFnValueN` enum is not a Rust function; got: {body}"
    );
    assert!(
        !body.contains("Some(f.as_ref()"),
        "the `map` arm already yields the optional result, so the surrounding coercion must not \
         wrap it in a second `Some`; got: {body}"
    );
}

#[test]
fn optional_fn_value_call_coerces_and_pads_its_arguments() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });
    let opt_fn_ty = types.intern(&Type::Optional {
        inner: fn_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let opt_i64_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });

    let mut target = empty_func("target_fn");
    target.id = FunctionId::from_raw(0);
    target.ret = i64_ty;
    target.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let callee_local = LocalId::from_raw(0);
    let result = LocalId::from_raw(1);
    let mut caller = empty_func("caller");
    caller.id = FunctionId::from_raw(1);
    caller.ret = opt_i64_ty;
    caller.params = vec![MirParam {
        id: callee_local,
        name: Atom::from("f"),
        ty: opt_fn_ty,
    }];
    let int_arg = MirExpr::Int {
        value: 7,
        ty: i64_ty,
    };
    caller.body = MirBody {
        locals: vec![MirLocalDecl {
            id: result,
            name: Atom::from("r"),
            ty: opt_i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: result,
                    ty: opt_i64_ty,
                    init: Some(MirExpr::IndirectCall {
                        callee: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(callee_local)),
                            ty: opt_fn_ty,
                        }),
                        args: vec![int_arg],
                        ty: i64_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::Local(result))),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(target));
    prog.push_decl(MirDecl::Function(caller));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn caller").nth(1).unwrap_or(""));
    assert!(
        body.contains("map(|__ts_recv|__ts_fn_call_0(__ctx,__ts_recv,7))"),
        "the helper call must carry the argument inside the closure arm, using the same coercion \
         the direct indirect-call path applies; got: {body}"
    );
}

#[test]
fn async_mutating_method_call_is_run_or_polled_in_the_staged_block() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let void_ty = types.intern(&Type::Void);
    let promise_ty = types.intern(&Type::Promise {
        ok: void_ty,
        err: None,
    });
    let owner = StructId::from_raw(1);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let method_id = FunctionId::from_raw(40);

    let mut method = empty_func("add");
    method.id = method_id;
    method.kind = FunctionKind::Method { owner, self_param };
    method.ret = promise_ty;
    method.effects.is_async = true;
    method.params = vec![
        MirParam {
            id: self_param,
            name: Atom::from("self"),
            ty: struct_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: f64_ty,
        },
    ];
    method.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: field_id,
                    ty: f64_ty,
                },
                value: MirExpr::Local(value_param),
            }],
        },
    };

    let recv_param = LocalId::from_raw(0);
    let mut run = empty_func("run");
    run.id = FunctionId::from_raw(41);
    run.ret = f64_ty;
    run.params = vec![MirParam {
        id: recv_param,
        name: Atom::from("c"),
        ty: struct_ty,
    }];
    run.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Call {
                    callee: method_id,
                    args: vec![
                        MirExpr::Local(recv_param),
                        MirExpr::Int {
                            value: 5,
                            ty: f64_ty,
                        },
                    ],
                    ty: promise_ty,
                }),
                MirStmt::Return(Some(MirExpr::Float {
                    value: 0.0,
                    ty: f64_ty,
                })),
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
            ty: f64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![method],
    }));
    prog.push_decl(MirDecl::Function(run));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn run").nth(1).unwrap_or(""));
    assert!(
        body.contains("ts_aot_runtime::__ts_aot_run_or_poll(Counter::add(&(c),__ctx,"),
        "an async mutating method call must be driven inside the staged block, otherwise the \
         discarded `Future` never runs and the generated value is not the backend's promise; got: \
         {body}"
    );
}

#[test]
fn sync_mutating_method_call_stays_a_direct_call() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let owner = StructId::from_raw(1);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let method_id = FunctionId::from_raw(50);

    let mut method = empty_func("add");
    method.id = method_id;
    method.kind = FunctionKind::Method { owner, self_param };
    method.ret = f64_ty;
    method.params = vec![
        MirParam {
            id: self_param,
            name: Atom::from("self"),
            ty: struct_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: f64_ty,
        },
    ];
    method.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: field_id,
                    ty: f64_ty,
                },
                value: MirExpr::Local(value_param),
            }],
        },
    };

    let recv_param = LocalId::from_raw(0);
    let mut run = empty_func("run");
    run.id = FunctionId::from_raw(51);
    run.ret = f64_ty;
    run.params = vec![MirParam {
        id: recv_param,
        name: Atom::from("c"),
        ty: struct_ty,
    }];
    run.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Call {
                    callee: method_id,
                    args: vec![
                        MirExpr::Local(recv_param),
                        MirExpr::Int {
                            value: 5,
                            ty: f64_ty,
                        },
                    ],
                    ty: f64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Float {
                    value: 0.0,
                    ty: f64_ty,
                })),
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
            ty: f64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![method],
    }));
    prog.push_decl(MirDecl::Function(run));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn run").nth(1).unwrap_or(""));
    assert!(
        body.contains("Counter::add(&(c),__ctx,"),
        "a synchronous mutating method call keeps its direct form; got: {body}"
    );
    assert!(
        !body.contains("__ts_aot_run_or_poll"),
        "a synchronous mutating method call must not be routed through the promise helper; got: \
         {body}"
    );
}

#[test]
fn optional_method_call_closure_parameter_avoids_source_binding_names() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let void_ty = types.intern(&Type::Void);
    let owner = StructId::from_raw(1);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let opt_struct_ty = types.intern(&Type::Optional {
        inner: struct_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let method_id = FunctionId::from_raw(60);

    let mut method = empty_func("set");
    method.id = method_id;
    method.kind = FunctionKind::Method { owner, self_param };
    method.ret = void_ty;
    method.params = vec![
        MirParam {
            id: self_param,
            name: Atom::from("self"),
            ty: struct_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: f64_ty,
        },
    ];
    method.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: field_id,
                    ty: f64_ty,
                },
                value: MirExpr::Local(value_param),
            }],
        },
    };

    let recv_param = LocalId::from_raw(0);
    let source_local = LocalId::from_raw(1);
    let mut caller = empty_func("got");
    caller.id = FunctionId::from_raw(61);
    caller.ret = f64_ty;
    caller.params = vec![MirParam {
        id: recv_param,
        name: Atom::from("e"),
        ty: opt_struct_ty,
    }];
    caller.body = MirBody {
        locals: vec![MirLocalDecl {
            id: source_local,
            name: Atom::from("__ts_recv"),
            ty: f64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Call {
                    callee: method_id,
                    args: vec![
                        MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(recv_param)),
                            ty: opt_struct_ty,
                        },
                        MirExpr::Local(source_local),
                    ],
                    ty: void_ty,
                }),
                MirStmt::Return(Some(MirExpr::Local(source_local))),
            ],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("E"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("v"),
            ty: f64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![method],
    }));
    prog.push_decl(MirDecl::Function(caller));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn got").nth(1).unwrap_or(""));
    assert!(
        body.contains("map(|__ts_recv_|E::set(__ts_recv_,__ctx,__ts_recv))"),
        "the unwrapped receiver must be bound to a generated name that cannot collide with a \
         source binding, so the source argument `__ts_recv` keeps its own value; got: {body}"
    );
    assert!(
        !body.contains("map(|__ts_recv|E::set(__ts_recv,__ctx,__ts_recv))"),
        "the closure parameter must not reuse a source binding's name; got: {body}"
    );
}

#[test]
fn indirect_call_emits_callee_args_for_known_callee() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let callee_local = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("caller");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: callee_local,
                name: Atom::from("callee_ref"),
                ty: int_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: dest,
                name: Atom::from("result"),
                ty: int_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::IndirectCall {
                callee: Box::new(MirExpr::Local(callee_local)),
                args: vec![
                    MirExpr::Int {
                        value: 1,
                        ty: int_ty,
                    },
                    MirExpr::Int {
                        value: 2,
                        ty: int_ty,
                    },
                ],
                ty: int_ty,
            }))],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();

    assert!(
        s.contains("callee_ref (1 , 2)"),
        "MirExpr::IndirectCall with non-Optional callee must emit a direct Rust call `callee(args)`. \
         PR 1.4 expansion replaces the old `Runtime::CallIndirect` path that emitted \
         `__ts_aot_call_indirect(callee_str, args_slice, dispatch_table)`. \
         Got: {s}"
    );
}

#[test]
fn indirect_call_with_optional_chain_callee_emits_as_ref_map_call() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I64);
    let opt_int = types.intern(&Type::Optional {
        inner: int_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let obj = LocalId::from_raw(0);
    let mut f = empty_func("caller");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: obj,
            name: Atom::from("obj"),
            ty: opt_int,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::IndirectCall {
                callee: Box::new(MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(obj)),
                    ty: opt_int,
                }),
                args: vec![MirExpr::Int {
                    value: 7,
                    ty: int_ty,
                }],
                ty: int_ty,
            }))],
        },
    };

    let tokens = emit_function(&f, &types).expect("obj?.() must emit");
    let s = tokens.to_string();

    assert!(
        s.contains("obj . as_ref () . map (| __ts_recv | __ts_recv (7))"),
        "MirExpr::IndirectCall with OptionalChain callee (obj?.() pattern) must emit \
         `obj.as_ref().map(|recv| recv(args))` — Option-aware short-circuit, with the unwrapped \
         callee bound to a generated name so a source argument cannot shadow it. \
         Got: {s}"
    );
}

#[test]
fn function_typed_param_and_return_exclude_function_from_dispatch_table() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_param_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });
    let fn_return_ty = types.intern(&Type::Fn {
        params: vec![],
        optional: vec![],
        ret: i64_ty,
        err: None,
    });

    let mut with_fn_param = empty_func("with_fn_param");
    with_fn_param.ret = i64_ty;
    with_fn_param.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: fn_param_ty,
    }];

    let mut with_fn_return = empty_func("with_fn_return");
    with_fn_return.id = FunctionId::from_raw(1);
    with_fn_return.ret = fn_return_ty;
    with_fn_return.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(with_fn_param));
    prog.push_decl(MirDecl::Function(with_fn_return));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    assert!(
        !s.contains("__ts_aot_dispatch_with_fn_param"),
        "function-typed param => Type::Fn not in is_u64_arg_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_dispatch_with_fn_return"),
        "function-typed return => Type::Fn not in is_u64_ret_packable set => no `fn(&[u64]) -> u64` wrapper. Got: {s}"
    );
    assert!(
        !s.contains("__TS_AOT_DISPATCH_TABLE"),
        "no sync dispatchable function (both excluded by function types) => no dispatch table. Got: {s}"
    );
}

#[test]
fn function_typed_param_emits_fn_value_enum() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });

    let mut f = empty_func("take_fn");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: fn_ty,
    }];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "take_fn");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("cb:__TsFnValue0"),
        "param typed `(a: i64) => i64` must emit as the generated Fn-value enum type. \
         Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}

#[test]
fn function_typed_return_emits_fn_value_enum() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });

    let mut f = empty_func("make_fn");
    f.ret = fn_ty;
    f.params = vec![];
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "make_fn");
    let sig_norm: String = sig.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        sig_norm.contains("->__TsFnValue0"),
        "return type `(a: i64) => i64` must emit as the generated Fn-value enum type. \
         Whitespace-insensitive. Got signature fragment: `{sig}`"
    );
}

#[test]
fn fn_value_let_local_emits_enum() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![],
        optional: vec![],
        ret: i64_ty,
        err: None,
    });
    let mut clos = empty_func("__ts_aot_closure_0");
    clos.id = FunctionId::from_raw(1);
    clos.ret = i64_ty;
    let mut run = empty_func("run");
    run.ret = i64_ty;
    run.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(0),
            name: Atom::from("g"),
            ty: fn_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: LocalId::from_raw(0),
                    ty: fn_ty,
                    init: Some(MirExpr::Global(Atom::from("__ts_aot_closure_0"))),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::IndirectCall {
                    callee: Box::new(MirExpr::Local(LocalId::from_raw(0))),
                    args: vec![],
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(clos));
    prog.push_decl(MirDecl::Function(run));
    let tokens = emit_decls(&prog, &types).expect("emit");
    let s = tokens.to_string();
    let run_body = norm(
        s.split("fn run")
            .nth(1)
            .unwrap_or_else(|| panic!("`run` must be emitted, got: {s}")),
    );
    assert!(
        run_body.contains("letg:__TsFnValue0<'a>=__TsFnValue0::TsAotClosure0"),
        "the `g` local in `run` must be initialized with the enum variant standing for \
         `__ts_aot_closure_0`, got run body: {run_body}"
    );
}

#[test]
fn fn_value_enum_and_call_helper_are_emitted() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i64_ty],
        optional: vec![false],
        ret: i64_ty,
        err: None,
    });

    let mut callee = empty_func("add_one");
    callee.id = FunctionId::from_raw(0);
    callee.ret = i64_ty;
    callee.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("x"),
        ty: i64_ty,
    }];

    let mut invoker = empty_func("call_it");
    invoker.id = FunctionId::from_raw(1);
    invoker.ret = i64_ty;
    invoker.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("cb"),
        ty: fn_ty,
    }];

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(callee));
    prog.push_decl(MirDecl::Function(invoker));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    assert!(
        s.contains("enum __TsFnValue0"),
        "a fn-value enum must be emitted for a used function signature, got: {s}"
    );
    assert!(
        s.contains("AddOne"),
        "the enum must carry a variant for every same-signature function, got: {s}"
    );
    assert!(
        s.contains("fn __ts_fn_call_0"),
        "a static-dispatch call helper must be emitted, got: {s}"
    );
}
