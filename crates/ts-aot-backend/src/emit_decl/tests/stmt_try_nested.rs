use super::super::{emit_decls, emit_function};
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeId};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirPlace, MirProgram, MirStmt,
};
use ts_aot_test_support::run_generated_crate;

#[test]
fn result_void_fallthrough_returns_ok_unit() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let void_ty = types.intern(&Type::Void);
    let result_void = types.intern(&Type::Result {
        ok: void_ty,
        err: i64_ty,
    });
    let mut f = empty_func("f");
    f.ret = result_void;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Int {
                value: 1,
                ty: i64_ty,
            })],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let normalized: String = s.split_whitespace().collect();
    assert!(
        normalized.contains("returnOk(());"),
        "a `Result<(), E>` function whose body falls through must return `Ok(())`, got:\n{s}"
    );
    assert!(
        !normalized.contains("unreachable"),
        "a `Result<(), E>` fallthrough must not emit `unreachable!()`, got:\n{s}"
    );
}

#[test]
fn try_err_arm_breaks_loop_after_handling() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
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
    let prefix = "match __try_result {";
    let match_pos = s
        .find(prefix)
        .expect("the try loop must match on __try_result");
    let open = match_pos + prefix.len();
    let mut depth = 1i32;
    let mut end = open;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let arms = &s[open..end];
    let err_start = arms
        .find("Err (__e) =>")
        .expect("the try loop's match must have an Err (__e) arm");
    let err_arm = &arms[err_start..];
    let arm_brace = err_arm
        .find('{')
        .expect("the Err (__e) arm must have a block body");
    let mut arm_depth = 0i32;
    let mut arm_close = None;
    for (i, ch) in err_arm[arm_brace..].char_indices() {
        match ch {
            '{' => arm_depth += 1,
            '}' => {
                arm_depth -= 1;
                if arm_depth == 0 {
                    arm_close = Some(arm_brace + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let arm_body = &err_arm[arm_brace + 1..arm_close.expect("the Err arm block must be closed")];
    let compact_arm: String = arm_body.chars().filter(|c| !c.is_whitespace()).collect();
    let last_break = compact_arm
        .rfind("break'__try_")
        .expect("the Err (__e) arm must break the try loop");
    let tail = &compact_arm[last_break..];
    let semi = tail
        .find(';')
        .expect("the loop break must be terminated by `;`");
    assert!(
        tail[..semi].starts_with("break'__try_") && tail[semi + 1..].is_empty(),
        "the Err (__e) arm of the try loop's match must exit the loop UNCONDITIONALLY: the break \
         must be the arm's last statement, not nested in a branch that can fall through, \
         otherwise the try body can run a second time. Got:\n{s}"
    );
}

#[test]
fn nested_try_restores_in_try_state_for_catch_body() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    println!("{}", nested_try_outer_catch::nested_try_catch(&ctx));
}
"#;
    let mut types = ts_aot_core::TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let outer_e = LocalId::from_raw(0);
    let mut func = empty_func("nested_try_catch");
    func.export_name = Some("nested_try_catch".to_owned());
    func.ret = i32_ty;
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: outer_e,
            name: Atom::from("outer"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Throw {
                                error: MirExpr::Int {
                                    value: 1,
                                    ty: i32_ty,
                                },
                                error_ty: i32_ty,
                            }],
                        },
                        catch_param: None,
                        catch: Some(MirBlock {
                            stmts: vec![MirStmt::Throw {
                                error: MirExpr::Int {
                                    value: 2,
                                    ty: i32_ty,
                                },
                                error_ty: i32_ty,
                            }],
                        }),
                        finally: None,
                    }],
                },
                catch_param: Some(outer_e),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Local(outer_e)))],
                }),
                finally: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &types)
        .expect("emit must succeed")
        .to_string();
    let inner_throw = s
        .find("__ts_aot_throw (1)")
        .expect("the inner try body's throw of 1 must emit `__ts_aot_throw(1)`");
    let inner_catch_throw = s
        .find("__ts_aot_throw (2)")
        .expect("the inner catch body's throw of 2 must also emit `__ts_aot_throw(2)`");
    let inner_rethrow = s.find("resume_unwind (__e)").expect(
        "the inner catch's own catch_unwind must store 2 in __pending_throw and rethrow it with \
         resume_unwind",
    );
    let outer_handler = s.find("__ts_aot_catch_typed :: < i32 >").expect(
        "the enclosing handler must bind the rethrown payload via `__ts_aot_catch_typed::<i32>`",
    );
    let throw_count = s.matches("__ts_aot_throw").count();
    assert!(
        throw_count >= 2,
        "Inner try body throw and catch-body throw must both emit `__ts_aot_throw` (panic path), \
         got {throw_count} occurrences in: {s}"
    );
    assert!(
        inner_throw < inner_catch_throw
            && inner_catch_throw < inner_rethrow
            && inner_rethrow < outer_handler,
        "The enclosing handler must be reached by the inner catch's rethrow of its own throw of 2, \
         not by the try body's throw of 1: `__ts_aot_throw(1)` -> inner catch `__ts_aot_throw(2)` \
         -> inner `resume_unwind(__e)` -> outer `__ts_aot_catch_typed::<i32>(__e)`. Got: {s}"
    );

    let stdout = run_generated_crate("nested_try_outer_catch", &s, MAIN);
    assert_eq!(
        stdout.trim(),
        "2",
        "the enclosing handler must observe the inner catch's throw of 2 (not the original 1), \
         got stdout: {stdout}"
    );
}

#[test]
fn try_throw_with_finally_runs_finally_then_rethrows() {
    let mut func = empty_func("try_throw_finally");
    let i64_ty = TypeId::from_raw(7);
    let finally_flag = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: finally_flag,
            name: Atom::from("finally_ran"),
            ty: i64_ty,
            mutable: true,
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
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: finally_flag },
                        value: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                    }],
                }),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    let finally_pos = s.find("finally_ran =");
    let replay_pos = s.find("if let Some (__e) = __pending_throw");
    assert!(
        finally_pos.is_some() && replay_pos.is_some() && finally_pos < replay_pos,
        "Finally body must be emitted BEFORE the pending-throw replay block, otherwise the \
         throw would bypass finally. The first `resume_unwind` inside the sentinel gate is \
         structural (part of the try body's catch_unwind wrapper) and appears before finally; \
         the pending-throw replay after finally is the correctness check. \
         Expected `finally_ran = ...` before `if let Some(__e) = __pending_throw`. Got: {s}"
    );
}

#[test]
fn nested_try_in_generator_propagates_through_outer_replay() {
    let mut func = empty_func("nested_try_gen");
    let i64_ty = TypeId::from_raw(7);
    let finally_flag = LocalId::from_raw(0);
    func.kind = FunctionKind::Generator;
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: finally_flag,
            name: Atom::from("finally_ran"),
            ty: i64_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                value: 7,
                                ty: i64_ty,
                            }))],
                        },
                        catch_param: None,
                        catch: None,
                        finally: None,
                    }],
                },
                catch_param: None,
                catch: None,
                finally: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: finally_flag },
                        value: MirExpr::Int {
                            value: 9,
                            ty: i64_ty,
                        },
                    }],
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
        s.contains("__return_slot_1 = Some (Some (7))"),
        "Inner generator try return 7 must set inner slot to Some(Some(7)). Got: {s}"
    );
    assert!(
        s.contains("__return_slot_0 = Some (__v)"),
        "Inner generator replay must propagate Some(Some(7)) to outer slot via \
         `__return_slot_0 = Some(__v);` (not `return __v;` which would exit the generator). Got: {s}"
    );
    assert!(
        s.contains("if let Some (__v) = __return_slot_0 { return __v ; }")
            || s.contains("if let Some (__v) = __return_slot_0 { return __v;}"),
        "Outermost generator try's replay must `return __v;`. Got: {s}"
    );
    let inner_replay_pos = s
        .find("__return_slot_0 = Some (__v)")
        .expect("inner generator replay must reach the outer slot");
    let outer_finally_pos = s
        .find("finally_ran = 9")
        .expect("the outer finally must emit its distinct assignment `finally_ran = 9;`");
    let outer_replay_pos = s
        .find("if let Some (__v) = __return_slot_0")
        .expect("outermost generator replay must read the outer slot");
    assert!(
        inner_replay_pos < outer_finally_pos && outer_finally_pos < outer_replay_pos,
        "The outer finally must run after the inner return reaches `__return_slot_0` and before \
         the outer replay returns: inner replay (`__return_slot_0 = Some(__v)`) -> outer finally \
         (`finally_ran = 9`) -> outer replay (`if let Some(__v) = __return_slot_0 {{ return __v; \
         }}`). Dropping or reordering the outer finally would skip it on the generator return \
         path. Got: {s}"
    );
}
