use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, TypeId};
use ts_aot_ir_mir::{MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirProgram, MirStmt};

#[test]
fn try_return_in_body_runs_finally_then_replays_return() {
    let mut func = empty_func("try_return_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 42,
                        ty: i64_ty,
                    }))],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 7,
                        ty: i64_ty,
                    })],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    let break_pos = s.find("break '__try_");
    let finally_pos = s.find("7 ;");
    let slot_set_pos = s.find("__return_slot_0 = Some (42)");
    let signal_pos = s.find("return __ReturnSignal :: Break");
    let replay_pos = s.find("if let Some (__v) = __return_slot_0");
    assert!(
        break_pos.is_some()
            && finally_pos.is_some()
            && slot_set_pos.is_some()
            && signal_pos.is_some()
            && replay_pos.is_some(),
        "try-with-return-and-finally must emit: try body sets runtime slot `__return_slot_0 = Some(42);` \
         and signals `break #label;`, then finally body (`7 ;`), then `if let Some(__v) = __return_slot_0 \
         {{ return __v; }}` (replay). Got: {s}"
    );
    assert!(
        slot_set_pos < signal_pos && signal_pos < break_pos,
        "The return path must write the runtime slot BEFORE it signals exit: \
         `__return_slot_0 = Some(42);` -> `return __ReturnSignal::Break;` -> `break #try_label;`. \
         If the exit signal came first, the slot could still be empty when replay reads it. Got: {s}"
    );
    assert!(
        break_pos < finally_pos && finally_pos < replay_pos,
        "Order must be: try body return (slot set + break) -> finally body -> replay. \
         If replay appears before finally, finally is skipped (wrong). Got: {s}"
    );
}

#[test]
fn try_catch_return_in_catch_runs_finally_then_replays_return() {
    let mut func = empty_func("try_catch_return");
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
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(err_local),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 99,
                        ty: i64_ty,
                    }))],
                }),
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 5,
                        ty: i64_ty,
                    })],
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
        s.contains("break '__try_"),
        "catch body's `return 99` must be transformed to `break #try_label` so finally runs. \
         Bare `return 99` inside the catch would exit the function and skip finally. Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (99)"),
        "catch body's return must set runtime slot `__return_slot_0 = Some(99);` so finally can \
         observe and replay it. Got: {s}"
    );
    let break_pos = s.find("break '__try_").expect("break in emit");
    let finally_pos = s.find("5 ;").expect("finally in emit");
    let slot_set_pos = s
        .find("__return_slot_0 = Some (99)")
        .expect("catch slot set in emit");
    let signal_pos = s
        .find("return __ReturnSignal :: Break")
        .expect("exit signal in emit");
    let replay_pos = s
        .find("if let Some (__v) = __return_slot_0")
        .expect("replay in emit");
    assert!(
        slot_set_pos < signal_pos,
        "The catch return path must write the runtime slot BEFORE it signals exit: \
         `__return_slot_0 = Some(99);` -> `return __ReturnSignal::Break;`. If the exit signal came \
         first, the slot could still be empty when replay reads it. Got: {s}"
    );
    assert!(
        break_pos < finally_pos && finally_pos < replay_pos,
        "Order must be: catch return → break -> finally body -> replay (if let Some). \
         Got: {s}"
    );
}

#[test]
fn try_with_conditional_return_emits_closure_safe_signal() {
    let mut func = empty_func("try_cond_return");
    let i64_ty = TypeId::from_raw(7);
    let cond = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::If {
                        cond: MirExpr::Local(cond),
                        then_block: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            }))],
                        },
                        else_block: Some(MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 2,
                                ty: i64_ty,
                            }))],
                        }),
                    }],
                },
                catch_param: None,
                catch: None,
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
        s.contains("__ReturnSignal"),
        "try with conditional return must declare __ReturnSignal enum as closure-safe \
         control signal, got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (1)"),
        "if-branch return must set runtime slot to Some(1) so the executed branch's value \
         is preserved at runtime (not overwritten by codegen-time last-write-wins). Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (2)"),
        "else-branch return must set runtime slot to Some(2) so the executed branch's value \
         is preserved at runtime. Got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_0"),
        "after the try block, replay must use the runtime slot via `if let Some(__v) = slot \
         {{ return __v; }}` so whichever branch executed drives the actual return. Got: {s}"
    );
    assert!(
        s.contains("return __ReturnSignal :: Break"),
        "each return must also signal `return Ok(__ReturnSignal::Break);` so the outer loop \
         can perform the break (no cross-closure `break #label;`). Got: {s}"
    );
    assert!(
        s.contains("match __try_result"),
        "outer try loop must pattern-match on __try_result to perform the break. Got: {s}"
    );
}

#[test]
fn nested_try_finally_return_propagates_through_outer_replay() {
    let mut func = empty_func("nested_try_finally");
    let i64_ty = TypeId::from_raw(7);
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 42,
                                ty: i64_ty,
                            }))],
                        },
                        catch_param: None,
                        catch: None,
                        finally: Some(MirBlock {
                            stmts: vec![MirStmt::Expr(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            })],
                        }),
                    }],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Int {
                        value: 2,
                        ty: i64_ty,
                    })],
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
        s.contains("__return_slot_1 = Some (42)"),
        "inner try's return 42 must set the inner runtime slot (__return_slot_1 since \
         counter is allocated on enter), got: {s}"
    );
    let inner_pos = s
        .find("__return_slot_1 = Some (42)")
        .expect("inner slot position");
    let inner_finally_pos = s.find("1 ;").expect("inner finally position");
    let inner_replay_pos = s
        .find("__return_slot_0 = Some (__v)")
        .or_else(|| s.find("if let Some (__v) = __return_slot_1"))
        .expect("inner replay position");
    assert!(
        inner_pos < inner_finally_pos && inner_finally_pos < inner_replay_pos,
        "Inner try order: set inner slot -> run inner finally (`1 ;`) -> inner replay. Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (__v)"),
        "Inner replay must propagate to the outer slot (__return_slot_0) for nested try, \
         not `return __v;` (which would skip the outer finally). Got: {s}"
    );
    let outer_finally_pos = s.rfind("2 ;").expect("outer finally position");
    let outer_replay_pos = s
        .rfind("if let Some (__v) = __return_slot_0")
        .expect("outer replay position");
    let outer_return_pos = s.rfind("return __v ;").expect("outer return position");
    assert!(
        outer_replay_pos < outer_return_pos,
        "Outermost try's replay must `return __v;` (no parent slot to propagate to). Got: {s}"
    );
    let inner_replay_to_outer_finally = inner_replay_pos < outer_finally_pos;
    let outer_finally_to_outer_replay = outer_finally_pos < outer_replay_pos;
    assert!(
        inner_replay_to_outer_finally && outer_finally_to_outer_replay,
        "Outer try order: inner replay propagates -> outer finally (`2 ;`) -> outer replay. \
         Got: {s}"
    );
}
