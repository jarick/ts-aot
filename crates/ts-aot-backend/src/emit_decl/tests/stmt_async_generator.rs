use super::super::emit_function;
use super::empty_func;
use ts_aot_core::{Atom, LocalId, Type};
use ts_aot_ir_mir::{FunctionKind, MirBlock, MirBody, MirExpr, MirLocalDecl, MirStmt};

#[test]
fn async_generator_return_inside_try_uses_inner_slot_and_wraps_finished() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock::new()),
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__return_slot_0 : Option < Option < i64 >>"),
        "generator try slot must use the inner return type (i64), got:\n{s}"
    );
    assert!(
        s.contains("AsyncGenYield :: finished (__v)"),
        "async generator replay must wrap the slot value in AsyncGenYield::finished, got:\n{s}"
    );
}

#[test]
fn async_generator_try_finally_wraps_rethrow_for_promise_rejection() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock::new(),
                catch_param: None,
                catch: None,
                finally: Some(MirBlock::new()),
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        s.contains("__ts_aot_async_generator_rethrow"),
        "async generator try/finally must wrap the rethrow so next() rejects, got:\n{s}"
    );
}

#[test]
fn async_generator_nested_try_wraps_rethrow_only_at_outermost() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: true,
    });
    let mut f = empty_func("g");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.effects.is_async = true;
    let inner = MirStmt::Try {
        body: MirBlock::new(),
        catch_param: None,
        catch: None,
        finally: Some(MirBlock::new()),
    };
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock { stmts: vec![inner] },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock::new()),
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    let mut sites = normalized
        .split("__pending_throw{std::panic::resume_unwind(")
        .skip(1);
    let inner = sites
        .next()
        .expect("the inner (nested) try must rethrow the escaping payload");
    let outer = sites
        .next()
        .expect("the outermost try must rethrow the escaping payload");
    assert!(
        sites.next().is_none(),
        "only the two try scopes may rethrow the escaping payload, got:\n{s}"
    );
    assert!(
        inner.starts_with("__e);"),
        "the inner (nested) try must pass the raw payload to resume_unwind so the outermost \
         scope wraps it exactly once, got:\n{s}"
    );
    assert!(
        outer.starts_with("ts_aot_runtime::__ts_aot_async_generator_rethrow(__e));"),
        "the outermost try must wrap the escape payload with \
         __ts_aot_async_generator_rethrow so next() rejects instead of leaking a raw panic, \
         got:\n{s}"
    );
    assert_eq!(
        s.matches("__ts_aot_async_generator_rethrow").count(),
        1,
        "only the outermost try in a nested async-generator try must wrap the escape payload \
         (nested tries keep it raw), got:\n{s}"
    );
}

#[test]
fn async_generator_typed_catch_binding_is_inside_catch_unwind_closure() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
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
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: Some(MirBlock::new()),
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    let catch_pos = normalized
        .find("let__catch_result")
        .expect("the catch handler must bind __catch_result");
    let closure_body = normalized[catch_pos..]
        .split_once("AssertUnwindSafe(||{")
        .expect("the catch handler must run inside catch_unwind's closure")
        .1
        .split_once("));")
        .expect("the catch_unwind closure must be closed after the handler body")
        .0;
    assert!(
        closure_body.contains("__ts_aot_catch_typed"),
        "the typed catch binding (`__ts_aot_catch_typed`, which performs the downcast) must be \
         moved inside the catch_unwind closure so payload-mismatch panics are caught and the \
         finally flow runs, got:\n{s}"
    );
}

#[test]
fn async_generator_catch_mismatch_wraps_rethrow_for_promise_rejection() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
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
                body: MirBlock::new(),
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
        normalized.contains("resume_unwind(ts_aot_runtime::__ts_aot_async_generator_rethrow("),
        "async generator catch-parameter mismatch must wrap the rethrow so next() rejects, got:\n{s}"
    );
}
