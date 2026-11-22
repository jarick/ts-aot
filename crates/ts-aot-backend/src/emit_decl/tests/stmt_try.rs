use super::super::emit_decls;
use super::empty_func;
use super::norm;
use ts_aot_core::{Atom, LocalId, ModuleId, TypeId};
use ts_aot_ir_mir::{MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirProgram, MirStmt};

#[test]
fn try_with_catch_emits_catch_unwind() {
    let mut func = empty_func("try_catch");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
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
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Catch must wrap body in std::panic::catch_unwind, got: {s}"
    );
    assert!(
        s.contains("AssertUnwindSafe"),
        "Try/Catch closure must use AssertUnwindSafe to bypass UnwindSafe bound, got: {s}"
    );
    assert!(
        s.contains("match __catch_result"),
        "Try/Catch must wrap catch arm in `if let Err(...)` check, got: {s}"
    );
}

#[test]
fn try_with_finally_emits_finally_after_try() {
    let mut func = empty_func("try_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
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
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Finally must wrap body in catch_unwind, got: {s}"
    );
    assert!(
        s.contains("__pending_throw"),
        "Try/Finally without catch must allocate a __pending_throw slot so a throw inside try \
         can be re-raised after finally runs (otherwise finally would silently swallow errors). \
         Got: {s}"
    );
    assert!(
        s.contains("resume_unwind"),
        "Try/Finally must rethrow via resume_unwind after finally so uncaught throw still \
         propagates up. Got: {s}"
    );
    assert!(
        s.contains("return 2"),
        "Try/Finally must emit finally body after try block, got: {s}"
    );
}

#[test]
fn try_with_catch_and_finally_emits_both() {
    let mut func = empty_func("try_catch_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: Some(LocalId::from_raw(0)),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 9,
                        ty: i64_ty,
                    })],
                }),
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 3,
                        ty: i64_ty,
                    }))],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("catch_unwind"),
        "Try/Catch/Finally must use catch_unwind, got: {s}"
    );
    assert!(
        s.contains("match __catch_result"),
        "Try/Catch/Finally must have Err branch with catch body, got: {s}"
    );
    assert!(
        s.contains("return 3"),
        "Try/Catch/Finally must have finally body after catch, got: {s}"
    );
}

#[test]
fn try_catch_param_is_bound_for_catch_body_references() {
    let mut func = empty_func("try_catch_param");
    let i64_ty = TypeId::from_raw(7);
    let err_local = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: err_local,
            name: Atom::from("err"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                },
                catch_param: Some(err_local),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(err_local)))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("downcast ::<") || s.contains("downcast :: <"),
        "Try/Catch with catch_param must downcast __e to the parameter type, got: {s}"
    );
    assert!(
        s.contains("downcast"),
        "Try/Catch with catch_param must downcast `__e` to the catch_param's local type so \
         `let err: T = match __e.downcast::<T>() ...` produces a typed binding that \
         round-trips the thrown value (e.g. `let err: i64 = 42`). Without downcast, the \
         binding would be `Box<dyn Any>` and the catch body couldn't use it as `i64`. Got: {s}"
    );
    assert!(
        s.contains("err : i64") || s.contains("err : __ty") || s.contains("err : i64"),
        "Try/Catch with catch_param must type-annotate the binding so the catch body sees \
         the downcast type, not Box<dyn Any>. Got: {s}"
    );
    assert!(
        !s.contains("let _ = __e"),
        "Try/Catch with catch_param must NOT drop the payload via `let _ = __e;` (would leave \
         catch_param undeclared for catch body). Got: {s}"
    );
}

#[test]
fn try_catch_without_param_does_not_bind_param() {
    let mut func = empty_func("try_catch_no_param");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
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
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("let __catch_result = std :: panic :: catch_unwind"),
        "Try/Catch without catch_param must wrap catch body in catch_unwind so the catch arm's \
         own panic does not propagate as an unhandled throw. Got: {s}"
    );
    assert!(
        !norm(&s).contains("__e.downcast::<"),
        "Try/Catch without catch_param must NOT downcast __e (no parameter type), in either \
         `::<` or `:: <` token spacing. Got: {s}"
    );
}

#[test]
fn throw_inside_try_emits_panic_throw_helper() {
    let mut func = empty_func("try_throw");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 42,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 1,
                        ty: i64_ty,
                    }))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_throw"),
        "Throw inside try must emit `__ts_aot_throw(...)` (panic-based, catchable by \
         catch_unwind). If we emitted `return Err(...)` instead, catch_unwind would never see \
         the throw (Result::Err is normal control flow, not a panic). Got: {s}"
    );
    assert!(
        !s.contains("return Err"),
        "Throw inside try must NOT emit `return Err(...)` (would bypass catch_unwind), got: {s}"
    );
}

#[test]
fn throw_outside_try_still_emits_return_err() {
    let mut func = empty_func("bare_throw");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Throw {
                error: MirExpr::Int {
                    value: 7,
                    ty: i64_ty,
                },
                error_ty: i64_ty,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("return Err"),
        "Throw outside try must still emit `return Err(...)` (Result-based, propagates up to \
         function return), got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_throw"),
        "Throw outside try must NOT emit `__ts_aot_throw(...)` (would panic the whole process \
         instead of returning Result::Err to caller), got: {s}"
    );
}
