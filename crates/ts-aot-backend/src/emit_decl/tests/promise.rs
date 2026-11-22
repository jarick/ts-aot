use super::super::{emit_decls, emit_function};
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirGlobalDecl, MirLocalDecl, MirParam, MirProgram,
    MirStmt, RuntimeOp,
};
use ts_aot_test_support::check_generated_crate;

#[test]
fn promise_combinator_is_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let promise_ty = types.intern(&Type::Promise {
        ok: i64_ty,
        err: None,
    });
    let array_ty = types.intern(&Type::Array {
        element: promise_ty,
    });
    let promises = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("f");
    f.ret = promise_ty;
    f.params = vec![MirParam {
        id: promises,
        name: Atom::from("promises"),
        ty: array_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseRace,
                    args: vec![MirExpr::Local(promises)],
                    dest: Some(dest),
                    ty: promise_ty,
                    target_ty: Some(i64_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };
    let err = emit_function(&f, &types)
        .expect_err("promise combinators cannot be reconciled with arena-backed arrays");
    assert!(
        err.to_string().contains("promise combinator"),
        "the rejection must name the promise combinator, got: {err}"
    );
}

#[test]
fn promise_resolving_to_arena_array_is_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });
    let promise_ty = types.intern(&Type::Promise {
        ok: array_ty,
        err: None,
    });
    let arr = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = promise_ty;
    f.params = vec![MirParam {
        id: arr,
        name: Atom::from("arr"),
        ty: array_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(arr)))],
        },
    };
    let err = emit_function(&f, &types).expect_err(
        "resolving an arena-backed array into a promise cannot satisfy the `'static` bound",
    );
    assert!(
        err.to_string().contains("borrows the arena"),
        "the rejection must explain that the resolved value borrows the arena, got: {err}"
    );
}

#[test]
fn promise_passthrough_of_arena_array_is_allowed() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });
    let promise_ty = types.intern(&Type::Promise {
        ok: array_ty,
        err: None,
    });
    let p = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = promise_ty;
    f.params = vec![MirParam {
        id: p,
        name: Atom::from("p"),
        ty: promise_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(p)))],
        },
    };
    let s = emit_function(&f, &types)
        .expect("returning an existing promise unchanged never resolves an arena value")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    assert!(
        normalized.contains("Promise<ts_aot_runtime::bumpalo::collections::Vec<'a,i64>>"),
        "the promise type must keep the arena-backed element type in the signature, got:\n{s}"
    );
    assert!(
        !normalized.contains("__ts_aot_promise_resolve_value"),
        "a passthrough must not resolve the value into a new promise, got:\n{s}"
    );
}

#[test]
fn promise_resolve_and_reject_emit_compilable_crate() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let string_ty = types.intern(&Type::String);
    let promise_ty = types.intern(&Type::Promise {
        ok: f64_ty,
        err: None,
    });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));

    let resolved = LocalId::from_raw(0);
    let mut resolve = empty_func("resolve_value");
    resolve.export_name = Some("resolve_value".to_owned());
    resolve.ret = promise_ty;
    resolve.body = MirBody {
        locals: vec![MirLocalDecl {
            id: resolved,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseResolveStatic,
                    args: vec![MirExpr::Int {
                        value: 42,
                        ty: f64_ty,
                    }],
                    dest: Some(resolved),
                    ty: promise_ty,
                    target_ty: Some(f64_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(resolved))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(resolve));

    let rejected = LocalId::from_raw(0);
    let mut reject = empty_func("reject_value");
    reject.export_name = Some("reject_value".to_owned());
    reject.ret = promise_ty;
    reject.body = MirBody {
        locals: vec![MirLocalDecl {
            id: rejected,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseRejectStatic,
                    args: vec![MirExpr::String {
                        id: Atom::from("boom"),
                        ty: string_ty,
                    }],
                    dest: Some(rejected),
                    ty: promise_ty,
                    target_ty: Some(f64_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(rejected))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(reject));

    let rust = emit_decls(&prog, &types)
        .expect("resolve/reject must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("__ts_aot_promise_resolve_value::<f64>(42.0)"),
        "Promise.resolve must coerce the integer literal to the f64 value type, got:\n{rust}"
    );
    assert!(
        compact.contains("__ts_aot_promise_reject_value::<f64>("),
        "Promise.reject must use the promise's ok type as the turbofish, got:\n{rust}"
    );
    assert!(
        !compact.contains("into_owned"),
        "`JsString::to_string_lossy` already returns `String`; `.into_owned()` does not compile, \
         got:\n{rust}"
    );
    assert!(
        compact.contains(":ts_aot_runtime::Promise<f64>=__ts_aot_promise"),
        "the resolve/reject temp must be annotated with the concrete promise type, not `()`, \
         got:\n{rust}"
    );
    check_generated_crate("generated_check_promise_resolve_reject", &rust);
}

#[test]
fn promise_instances_of_arena_promise_are_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let promise_ty = types.intern(&Type::Promise {
        ok: arr_ty,
        err: None,
    });
    let p = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("f");
    f.ret = promise_ty;
    f.params = vec![MirParam {
        id: p,
        name: Atom::from("p"),
        ty: promise_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseThenInstance,
                    args: vec![MirExpr::Local(p), MirExpr::Unit],
                    dest: Some(dest),
                    ty: promise_ty,
                    target_ty: Some(arr_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };
    let err = emit_function(&f, &types).expect_err(
        "a promise whose value borrows the arena cannot use the `'static`-bound instance helpers",
    );
    assert!(
        err.to_string().contains("borrows the arena"),
        "the rejection must explain that the resolved value borrows the arena, got: {err}"
    );
}

#[test]
fn promise_resolve_widens_integer_width_to_target() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let i32_ty = types.intern(&Type::I32);
    let promise_ty = types.intern(&Type::Promise {
        ok: i32_ty,
        err: None,
    });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let src = LocalId::from_raw(0);
    let dest = LocalId::from_raw(1);
    let mut f = empty_func("f");
    f.export_name = Some("f".to_owned());
    f.ret = promise_ty;
    f.params = vec![MirParam {
        id: src,
        name: Atom::from("src"),
        ty: i64_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseResolveStatic,
                    args: vec![MirExpr::Local(src)],
                    dest: Some(dest),
                    ty: promise_ty,
                    target_ty: Some(i32_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let rust = emit_decls(&prog, &types)
        .expect("resolve must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("asi32"),
        "resolving an i64 into a Promise<i32> must cast with `as i32`, got:\n{rust}"
    );
    check_generated_crate("generated_check_promise_resolve_int_width", &rust);
}

#[test]
fn promise_resolve_int_literal_into_i32_target_compiles() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let i32_ty = types.intern(&Type::I32);
    let promise_ty = types.intern(&Type::Promise {
        ok: i32_ty,
        err: None,
    });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let dest = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.export_name = Some("f".to_owned());
    f.ret = promise_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseResolveStatic,
                    args: vec![MirExpr::Int {
                        value: 7,
                        ty: i64_ty,
                    }],
                    dest: Some(dest),
                    ty: promise_ty,
                    target_ty: Some(i32_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let rust = emit_decls(&prog, &types)
        .expect("resolve must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("__ts_aot_promise_resolve_value::<i32>(7)"),
        "an int literal resolved into a Promise<i32> must stay a bare literal, got:\n{rust}"
    );
    check_generated_crate("generated_check_promise_resolve_int_literal", &rust);
}

#[test]
fn promise_reject_module_level_string_reason_uses_string_conversion() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let string_ty = types.intern(&Type::String);
    let promise_ty = types.intern(&Type::Promise {
        ok: f64_ty,
        err: None,
    });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("reason"),
        ty: string_ty,
        mutable: false,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));

    let dest = LocalId::from_raw(0);
    let mut f = empty_func("reject_reason");
    f.ret = promise_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from(""),
            ty: promise_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::PromiseRejectStatic,
                    args: vec![MirExpr::Global(Atom::from("reason"))],
                    dest: Some(dest),
                    ty: promise_ty,
                    target_ty: Some(f64_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(dest))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let rust = emit_decls(&prog, &types)
        .expect("Promise.reject with a module-level string reason must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("__ts_aot_promise_reject_value::<f64>("),
        "a module-level string reason must reach the Promise.reject runtime call; got:\n{rust}"
    );
    assert!(
        compact.contains("to_string_lossy"),
        "a module-level string reason must convert through `JsString::to_string_lossy`, not the \
         unsupported-reason fallback; got:\n{rust}"
    );
}
