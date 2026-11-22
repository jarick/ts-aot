use super::super::emit_decls;
use super::empty_func;
use crate::BackendError;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeId};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirProgram, MirStmt,
};
use ts_aot_test_support::check_generated_crate;

#[test]
fn yield_with_value_emits_co_yield_await() {
    let mut func = empty_func("yield_test");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    let local = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("x"),
            ty: i64_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Local(local))),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__gen_co_") && s.contains(" . yield_ (x) . await"),
        "Yield(Some) must emit `co.yield_(x).await` inside the producer, got: {s}"
    );
}

#[test]
fn yield_without_value_emits_unit_yield() {
    let mut func = empty_func("yield_unit_test");
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Yield {
                expr: None,
                ty: TypeId::from_raw(0),
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__gen_co_") && s.contains(" . yield_ (()) . await"),
        "Yield(None) must emit `co.yield_(()).await`, got: {s}"
    );
}

#[test]
fn yield_in_non_generator_body_returns_internal_error() {
    let mut func = empty_func("yield_plain_test");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Yield {
                expr: None,
                ty: TypeId::from_raw(0),
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let err = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect_err("non-generator Yield must be rejected with an error");
    let BackendError::Internal(msg) = err else {
        panic!("expected BackendError::Internal, got other error variant");
    };
    assert!(
        msg.contains("MirExpr::Yield"),
        "Internal error must mention the MirExpr::Yield context, got: {msg:?}"
    );
    assert!(
        msg.contains("non-generator body"),
        "Internal error must explain the non-generator body invariant, got: {msg:?}"
    );
}

#[test]
fn yield_inside_try_finally_is_allowed() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let mut func = empty_func("yield_in_finally");
    func.ret = gen_ty;
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Try {
                    body: MirBlock::new(),
                    catch_param: None,
                    catch: None,
                    finally: Some(MirBlock {
                        stmts: vec![MirStmt::Expr(MirExpr::Yield {
                            expr: Some(Box::new(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            })),
                            ty: i64_ty,
                        })],
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
    prog.push_decl(MirDecl::Function(func));
    let rust = emit_decls(&prog, &types)
        .expect("yield in finally must remain valid (finally is outside the catch_unwind closure)")
        .to_string();

    let yield_pos = rust.find("yield_ (1) . await").expect(
        "the finally must emit the generator yield as `co.yield_(1).await`; losing it would \
         silently skip the yield",
    );
    let closure_marker = "AssertUnwindSafe (|| {";
    let closure_start = rust
        .find(closure_marker)
        .expect("the try body must be wrapped in a synchronous `AssertUnwindSafe` closure");
    let open = closure_start + closure_marker.len() - 1;
    let mut depth = 0i32;
    let mut closure_end = None;
    for (i, ch) in rust[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    closure_end = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let closure_end = closure_end.expect("the synchronous `catch_unwind` closure must be closed");
    assert!(
        yield_pos > closure_end,
        "the `finally` yield must be emitted OUTSIDE the synchronous `catch_unwind` closure: \
         `.await` inside a non-async closure is invalid Rust. Got:\n{rust}"
    );
    check_generated_crate("generated_check_yield_in_finally", &rust);
}

#[test]
fn generator_return_inside_try_preserves_completion_shape() {
    let mut func = empty_func("gen_try");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    })),
                    ty: i64_ty,
                }),
                MirStmt::Try {
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                            value: 2,
                            ty: i64_ty,
                        }))],
                    },
                    catch_param: None,
                    catch: None,
                    finally: None,
                },
                MirStmt::Try {
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(None)],
                    },
                    catch_param: None,
                    catch: None,
                    finally: None,
                },
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__return_slot_0 = Some (Some (2))"),
        "valued return inside a generator try block must set runtime slot to `Some(Some(2))`, got: {s}"
    );
    assert!(
        s.contains("__return_slot_1 = Some (None)"),
        "bare return inside a generator try block must set runtime slot to `Some(None)`, got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_1"),
        "generator try replay must read runtime slot via `if let Some(__v) = slot {{ return __v; }}`, \
         got: {s}"
    );
}

#[test]
fn generator_fn_body_emits_producer_async_block() {
    let mut func = empty_func("gen");
    func.kind = FunctionKind::Generator;
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Yield {
                    expr: Some(Box::new(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    })),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 2,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_generator_new (| __gen_co_") && s.contains(" | async move {"),
        "generator body must be wrapped in __ts_aot_generator_new(|co| async move {{..}}), got: {s}"
    );
    assert!(
        s.contains("return Some (2) ;"),
        "generator return must map to `return Some(v)` (completion value), got: {s}"
    );
}
