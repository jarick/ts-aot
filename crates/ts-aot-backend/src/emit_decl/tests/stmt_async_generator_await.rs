use super::super::emit_function;
use super::empty_func;
use ts_aot_core::{Atom, FunctionId, LocalId, NullishKinds, Type};
use ts_aot_ir_mir::{FunctionKind, MirBlock, MirBody, MirExpr, MirLocalDecl, MirStmt};

#[test]
fn async_generator_nested_finally_throw_uses_raw_path_for_outer_catch() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let str_ty = types.intern(&Type::String);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    let inner = MirStmt::Try {
        body: MirBlock::new(),
        catch_param: None,
        catch: None,
        finally: Some(MirBlock {
            stmts: vec![MirStmt::Throw {
                error: MirExpr::String {
                    id: Atom::from("boom"),
                    ty: str_ty,
                },
                error_ty: str_ty,
            }],
        }),
    };
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: str_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock { stmts: vec![inner] },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    assert!(
        normalized.contains("__ts_aot_catch_string"),
        "the enclosing catch must be emitted so it can run for the nested-finally throw, got:\n{s}"
    );
    assert!(
        normalized.contains("__ts_aot_throw("),
        "a throw inside a nested finally must use the raw throw path so the enclosing catch catches it, got:\n{s}"
    );
    assert!(
        !normalized.contains("__ts_aot_async_generator_throw("),
        "a nested-finally throw must not be wrapped as an async-generator throw (the enclosing catch would miss it), got:\n{s}"
    );
}

#[test]
fn async_generator_await_inside_try_uses_raw_helper_for_catch() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Await {
                        expr: Box::new(MirExpr::Int {
                            value: 0,
                            ty: promise_ty,
                        }),
                        ty: i64_ty,
                    })],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    assert!(
        normalized.contains("__ts_aot_await_value_raw"),
        "an await inside a try must use the raw helper so the enclosing catch catches the rejection, got:\n{s}"
    );
    assert!(
        !normalized.contains("__ts_aot_await_value_gen"),
        "inside a try an async-generator await must not reject through next(), got:\n{s}"
    );
}

#[test]
fn async_generator_inner_try_inside_outer_finally_wraps_rethrow() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let str_ty = types.intern(&Type::String);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    let inner = MirStmt::Try {
        body: MirBlock {
            stmts: vec![MirStmt::Throw {
                error: MirExpr::String {
                    id: Atom::from("boom"),
                    ty: str_ty,
                },
                error_ty: str_ty,
            }],
        },
        catch_param: None,
        catch: None,
        finally: Some(MirBlock::new()),
    };
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock::new(),
                catch_param: None,
                catch: None,
                finally: Some(MirBlock { stmts: vec![inner] }),
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert_eq!(
        s.matches("__ts_aot_async_generator_rethrow").count(),
        2,
        "the outer try and the inner try (emitted inside the outer finally) must each wrap their escape payload, got:\n{s}"
    );
}

#[test]
fn return_after_nested_try_still_slots_into_outer() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Try {
                            body: MirBlock::new(),
                            catch_param: None,
                            catch: None,
                            finally: None,
                        },
                        MirStmt::Return(Some(MirExpr::Int {
                            value: 5,
                            ty: i64_ty,
                        })),
                    ],
                },
                catch_param: None,
                catch: None,
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    assert!(
        normalized.contains("__return_slot_0=Some(5)"),
        "a return after a completed nested try must still be slotted into the enclosing try's \
         return slot (a bare `return 5` would be ill-typed inside the enclosing catch_unwind \
         closure and fail rustc with E0308), got:\n{s}"
    );
    assert!(
        normalized.contains("return__ReturnSignal::Break"),
        "the return after a nested try must propagate as a Break signal, got:\n{s}"
    );
    assert!(
        !normalized.contains("return5;"),
        "the return must not be emitted as a bare `return 5`, got:\n{s}"
    );
}

#[test]
fn async_return_of_promise_value_is_not_wrapped() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let p = LocalId::from_raw(0);
    let locals = || {
        vec![MirLocalDecl {
            id: p,
            name: Atom::from("p"),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }]
    };

    let mut plain = empty_func("plain");
    plain.ret = promise_ty;
    plain.effects.is_async = true;
    plain.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(p)))],
        },
    };
    let plain_s = emit_function(&plain, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        !plain_s.contains("__ts_aot_promise_resolve_value"),
        "an async function returning an already-Promise<T> value must not resolve it as T \
         (that would fail rustc with E0308), got:\n{plain_s}"
    );

    let mut in_try = empty_func("in_try");
    in_try.ret = promise_ty;
    in_try.effects.is_async = true;
    in_try.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(p)))],
                },
                catch_param: None,
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let try_s = emit_function(&in_try, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        !try_s.contains("__ts_aot_promise_resolve_value"),
        "the same distinction must hold for a return inside a try, got:\n{try_s}"
    );

    let mut value = empty_func("value");
    value.ret = promise_ty;
    value.effects.is_async = true;
    value.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 5,
                ty: i64_ty,
            }))],
        },
    };
    let value_s = emit_function(&value, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        value_s.contains("__ts_aot_promise_resolve_value"),
        "an async function returning a resolved non-promise value must still be wrapped, got:\n{value_s}"
    );

    let error_ty = types.intern(&Type::Error);
    let mut untyped_call = empty_func("untyped_call");
    untyped_call.ret = promise_ty;
    untyped_call.effects.is_async = true;
    untyped_call.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Call {
                callee: FunctionId::from_raw(7),
                args: Vec::new(),
                ty: error_ty,
            }))],
        },
    };
    let untyped_s = emit_function(&untyped_call, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        untyped_s.contains("__ts_aot_promise_resolve_value"),
        "a return whose expression the backend types as `Error` (e.g. a call or member access \
         the frontend could not resolve to a `Promise`) must be wrapped conservatively, got:\n{untyped_s}"
    );
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn async_bare_return_resolves_declared_optional_payload() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let optional_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: NullishKinds::UNDEFINED_ONLY,
    });
    let promise_ty = types.intern(&Type::Promise {
        ok: optional_ty,
        err: None,
    });

    let mut plain = empty_func("plain");
    plain.ret = promise_ty;
    plain.effects.is_async = true;
    plain.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(None)],
        },
    };
    let plain_s = emit_function(&plain, &types)
        .expect("function should emit")
        .to_string();
    let plain_c = compact(&plain_s);
    assert!(
        plain_c.contains(
            "__ts_aot_promise_resolve_value::<Option<i64>>(::core::option::Option::None)"
        ),
        "a bare `return;` in an async function whose declared payload is `number | undefined` \
         must resolve the payload's absent value (`None`), not `()`, got:\n{plain_s}"
    );
    assert!(
        !plain_c.contains("__ts_aot_promise_resolve_value::<()>(())"),
        "a bare `return;` must not resolve `()` when the declared payload is `number | undefined` \
         (the generated `Promise<()>` does not match the signature and fails rustc), got:\n{plain_s}"
    );

    let mut in_try = empty_func("in_try");
    in_try.ret = promise_ty;
    in_try.effects.is_async = true;
    in_try.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(None)],
                },
                catch_param: None,
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let try_s = emit_function(&in_try, &types)
        .expect("function should emit")
        .to_string();
    let try_c = compact(&try_s);
    assert!(
        try_c.contains(
            "__ts_aot_promise_resolve_value::<Option<i64>>(::core::option::Option::None)"
        ) && !try_c.contains("__ts_aot_promise_resolve_value::<()>(())"),
        "a bare `return;` inside a `try` must resolve the declared optional payload's absent \
         value too, got:\n{try_s}"
    );

    let void_ty = types.intern(&Type::Void);
    let void_promise_ty = types.intern(&Type::Promise {
        ok: void_ty,
        err: None,
    });
    let mut void_fn = empty_func("void_fn");
    void_fn.ret = void_promise_ty;
    void_fn.effects.is_async = true;
    void_fn.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(None)],
        },
    };
    let void_s = emit_function(&void_fn, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        compact(&void_s).contains("__ts_aot_promise_resolve_value::<()>(())"),
        "a bare `return;` in an async function declared `Promise<void>` must keep resolving `()`, \
         got:\n{void_s}"
    );
}

#[test]
fn async_bare_return_with_non_optional_payload_is_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let mut f = empty_func("plain");
    f.ret = promise_ty;
    f.effects.is_async = true;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(None)],
        },
    };
    let err = emit_function(&f, &types).expect_err(
        "a bare `return;` cannot produce a value for a payload that cannot hold `undefined`",
    );
    assert!(
        err.to_string().contains("cannot hold `undefined`"),
        "the rejection must explain that the payload cannot hold `undefined`, got: {err}"
    );
}

#[test]
fn async_return_of_optional_promise_value_is_not_wrapped() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let optional_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: NullishKinds::UNDEFINED_ONLY,
    });
    let promise_ty = types.intern(&Type::Promise {
        ok: optional_ty,
        err: None,
    });
    let p = LocalId::from_raw(0);
    let locals = || {
        vec![MirLocalDecl {
            id: p,
            name: Atom::from("p"),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }]
    };

    let mut plain = empty_func("plain");
    plain.ret = promise_ty;
    plain.effects.is_async = true;
    plain.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(p)))],
        },
    };
    let plain_s = emit_function(&plain, &types)
        .expect("function should emit")
        .to_string();
    let plain_c = compact(&plain_s);
    assert!(
        plain_c.contains(
            "asyncfnplain<'a>(__ctx:&ts_aot_runtime::Ctx<'a>,)->ts_aot_runtime::Promise<Option<i64>>{returnp;}"
        ),
        "returning an existing `Promise<number | undefined>` must return the local unchanged from \
         a function whose declared return type is that same promise type, got:\n{plain_s}"
    );
    assert!(
        !plain_c.contains("Some"),
        "returning an existing `Promise<number | undefined>` must not wrap it in `Some(..)`; the \
         returned value already has the promise type, got:\n{plain_s}"
    );
    assert!(
        !plain_c.contains("__ts_aot_promise_resolve_value"),
        "returning an existing promise must not resolve the value into a new promise, got:\n{plain_s}"
    );

    let mut in_try = empty_func("in_try");
    in_try.ret = promise_ty;
    in_try.effects.is_async = true;
    in_try.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(p)))],
                },
                catch_param: None,
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let try_s = emit_function(&in_try, &types)
        .expect("function should emit")
        .to_string();
    let try_c = compact(&try_s);
    assert!(
        try_c.contains("__return_slot_0=Some(p);return__ReturnSignal::Break;"),
        "a return of an existing promise inside a `try` must store `p` in the return slot exactly \
         once, got:\n{try_s}"
    );
    assert!(
        !try_c.contains("Some(Some(p))") && !try_c.contains("__ts_aot_promise_resolve_value"),
        "the same passthrough must hold for a return inside a `try` (the return slot itself wraps \
         the value in `Some`, but the promise value must not be wrapped again), got:\n{try_s}"
    );
}
