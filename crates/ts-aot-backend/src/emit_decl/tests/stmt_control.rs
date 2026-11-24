use super::super::{emit_decls, emit_function};
use super::empty_func;
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeId};
use ts_aot_ir_mir::{
    ConstValue, MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirPlace, MirProgram, MirStmt,
    SwitchCase,
};

#[test]
fn switch_with_int_cases_emits_match() {
    let mut func = empty_func("switch_int");
    let i64_ty = TypeId::from_raw(7);
    let disc = LocalId::from_raw(0);
    let a = LocalId::from_raw(1);
    let b = LocalId::from_raw(2);
    func.ret = i64_ty;
    func.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: disc,
                name: Atom::from("x"),
                ty: i64_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: a,
                name: Atom::from("a"),
                ty: i64_ty,
                mutable: true,
                narrowed: false,
            },
            MirLocalDecl {
                id: b,
                name: Atom::from("b"),
                ty: i64_ty,
                mutable: true,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Switch {
                disc: Box::new(MirExpr::Local(disc)),
                cases: vec![
                    SwitchCase {
                        value: ConstValue::Int(1),
                        body: MirBlock {
                            stmts: vec![MirStmt::Assign {
                                target: MirPlace::Local { id: a },
                                value: MirExpr::Int {
                                    value: 10,
                                    ty: i64_ty,
                                },
                            }],
                        },
                    },
                    SwitchCase {
                        value: ConstValue::Int(2),
                        body: MirBlock {
                            stmts: vec![MirStmt::Assign {
                                target: MirPlace::Local { id: b },
                                value: MirExpr::Int {
                                    value: 20,
                                    ty: i64_ty,
                                },
                            }],
                        },
                    },
                ],
                default: Some(MirBlock {
                    stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                        value: 0,
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
        s.contains("match x"),
        "Switch must emit `match <disc>` over disc local, got: {s}"
    );
    assert!(
        s.contains("1 =>") && s.contains("2 =>"),
        "Switch cases must emit numeric arms `1 => ...` and `2 => ...`, got: {s}"
    );
    assert!(
        s.contains("_ =>"),
        "Switch default must emit `_ => ...` arm, got: {s}"
    );
}

#[test]
fn switch_with_string_cases_emits_match() {
    let mut func = empty_func("switch_string");
    let string_ty = TypeId::from_raw(8);
    let disc = LocalId::from_raw(0);
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: disc,
            name: Atom::from("key"),
            ty: string_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Switch {
                disc: Box::new(MirExpr::Local(disc)),
                cases: vec![SwitchCase {
                    value: ConstValue::String(Atom::from("foo")),
                    body: MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::String {
                            id: Atom::from("foo"),
                            ty: string_ty,
                        }))],
                    },
                }],
                default: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("match key"),
        "Switch over string local must emit `match key`, got: {s}"
    );
    assert!(
        s.contains("\"foo\" =>"),
        "Switch string case must emit `\"foo\" =>` arm, got: {s}"
    );
    assert!(
        s.contains("_ => { }"),
        "Switch without default must emit empty `_ => {{}}` arm, got: {s}"
    );
}

#[test]
fn mixed_type_switch_cases_on_string_discriminant_use_equality_chain() {
    let mut types = ts_aot_core::TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let i64_ty = types.intern(&Type::I64);
    let disc = LocalId::from_raw(0);
    let mut func = empty_func("mixed_switch");
    func.ret = i64_ty;
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: disc,
            name: Atom::from("key"),
            ty: string_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: disc,
                    ty: string_ty,
                    init: Some(MirExpr::String {
                        id: Atom::from("foo"),
                        ty: string_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Switch {
                    disc: Box::new(MirExpr::Local(disc)),
                    cases: vec![
                        SwitchCase {
                            value: ConstValue::String(Atom::from("foo")),
                            body: MirBlock {
                                stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                    value: 1,
                                    ty: i64_ty,
                                }))],
                            },
                        },
                        SwitchCase {
                            value: ConstValue::Int(3),
                            body: MirBlock {
                                stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                                    value: 2,
                                    ty: i64_ty,
                                }))],
                            },
                        },
                    ],
                    default: Some(MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                            value: 0,
                            ty: i64_ty,
                        }))],
                    }),
                },
                MirStmt::Return(Some(MirExpr::Int {
                    value: -1,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &types)
        .expect("emit must succeed")
        .to_string();
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !compact.contains("3=>"),
        "a numeric case value on a string discriminant must not become an integer `match` arm \
         (Rust rejects `3 =>` against a `JsString`); the switch must use the equality chain, got:\n{s}"
    );
    assert!(
        compact.contains("elseiffalse"),
        "a numeric case value on a string discriminant must lower to a never-matching equality \
         test (JavaScript strict equality across types is false), got:\n{s}"
    );
    assert!(
        compact.contains("==ts_aot_runtime::JsString::from(\"foo\")"),
        "the string case must still compare against its `JsString` value, got:\n{s}"
    );
    ts_aot_test_support::check_generated_crate("generated_check_mixed_switch", &s);
}

#[test]
fn do_while_emits_loop_with_break_on_negated_cond() {
    let mut types = ts_aot_core::TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let cond = LocalId::from_raw(0);
    let mut func = empty_func("do_while");
    func.ret = bool_ty;
    func.body = MirBody {
        locals: vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: bool_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Local(cond))],
                },
                cond: MirExpr::Local(cond),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &types)
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("loop"),
        "DoWhile must emit Rust `loop` (body executes at least once), got: {s}"
    );
    assert!(
        s.contains("if ! c")
            || s.contains("if  ! c")
            || s.contains("if ! (c")
            || s.contains("if ! c ;"),
        "DoWhile must break on negated cond `if !c {{ break; }}`, got: {s}"
    );
    assert!(
        s.contains("break"),
        "DoWhile must emit `break` for exit, got: {s}"
    );
}

#[test]
fn do_while_with_empty_body_emits_loop_break_only() {
    let mut func = empty_func("do_while_empty");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock { stmts: Vec::new() },
                cond: MirExpr::Bool(false),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("loop") && s.contains("if !"),
        "DoWhile(empty body, false cond) must still emit `loop {{ if !false {{ break; }} }}`, got: {s}"
    );
}

#[test]
fn do_while_continue_in_body_uses_labeled_continue_for_cond_recheck() {
    let mut func = empty_func("do_while_continue");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::DoWhile {
                body: MirBlock {
                    stmts: vec![MirStmt::Continue],
                },
                cond: MirExpr::Bool(true),
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("__do_while_continue_")
            && s.contains("__do_while_break_")
            && s.contains(": loop"),
        "DoWhile must lower to a labeled loop whose body sits in a labeled block \
         (`'break: loop {{ 'continue: {{ body }} if !cond {{ break 'break; }} }}`) so the cond \
         check fires on every iteration. Got: {s}"
    );
    assert!(
        s.contains("break '__do_while_continue_"),
        "Continue in a DoWhile body must break out of the continue block so it reaches the \
         bottom condition check instead of jumping to the loop top. Got: {s}"
    );
    assert!(
        s.contains("break '__do_while_break_"),
        "the DoWhile condition check must break the surrounding loop so it exits when cond is \
         false. Got: {s}"
    );
    assert!(
        !s.contains("continue ;"),
        "a DoWhile continue must not fall through as a bare `continue;` (that would skip the \
         condition check). Got: {s}"
    );
}

#[test]
fn do_while_break_in_body_targets_do_while_not_enclosing_switch() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Switch {
                disc: Box::new(MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                }),
                cases: vec![SwitchCase {
                    value: ConstValue::Int(1),
                    body: MirBlock {
                        stmts: vec![MirStmt::DoWhile {
                            body: MirBlock {
                                stmts: vec![MirStmt::Break],
                            },
                            cond: MirExpr::Bool(false),
                        }],
                    },
                }],
                default: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        s.matches("break '__do_while_break_").count() >= 2,
        "a `break` inside a do-while body must target the do-while loop (its own break label), \
         not an enclosing switch, got:\n{s}"
    );
    assert!(
        !s.contains("break '__switch_"),
        "no `break` in a do-while body may target the enclosing switch's label, got:\n{s}"
    );
}

#[test]
fn continue_outside_dowhile_uses_enclosing_loop_label() {
    let mut func = empty_func("bare_while_continue");
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
            stmts: vec![MirStmt::While {
                cond: MirExpr::Local(cond),
                body: MirBlock {
                    stmts: vec![MirStmt::Expr(MirExpr::Local(cond)), MirStmt::Continue],
                },
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let s = emit_decls(&prog, &ts_aot_core::TypeTable::new())
        .expect("emit must succeed")
        .to_string();
    assert!(
        s.contains("'__while_") && s.contains("continue '__while_"),
        "Continue inside a regular While body must target the enclosing while's own label so it \
         stays correct inside nested switches and the try fallback (a bare `continue;` would be \
         ambiguous there). Got: {s}"
    );
    assert!(
        !s.contains("__do_while_"),
        "Continue in a regular While must NOT carry a __do_while_ label, got: {s}"
    );
}

#[test]
fn continue_in_nested_switch_and_try_in_while_targets_while_label() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let cond = LocalId::from_raw(0);
    let locals = || {
        vec![MirLocalDecl {
            id: cond,
            name: Atom::from("c"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }]
    };

    let mut switch_loop = empty_func("switch_in_while");
    switch_loop.ret = i64_ty;
    switch_loop.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::While {
                cond: MirExpr::Local(cond),
                body: MirBlock {
                    stmts: vec![MirStmt::Switch {
                        disc: Box::new(MirExpr::Local(cond)),
                        cases: vec![SwitchCase {
                            value: ConstValue::Int(1),
                            body: MirBlock {
                                stmts: vec![MirStmt::Continue],
                            },
                        }],
                        default: None,
                    }],
                },
            }],
        },
    };
    let switch_s = emit_function(&switch_loop, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        switch_s.contains("continue '__while_"),
        "a `continue` inside a switch nested in a while must target the while's label (switch \
         lowering must not clear the continue target), got:\n{switch_s}"
    );

    let mut try_loop = empty_func("try_in_while");
    try_loop.ret = i64_ty;
    try_loop.body = MirBody {
        locals: locals(),
        block: MirBlock {
            stmts: vec![MirStmt::While {
                cond: MirExpr::Local(cond),
                body: MirBlock {
                    stmts: vec![MirStmt::Try {
                        body: MirBlock {
                            stmts: vec![MirStmt::Continue],
                        },
                        catch_param: None,
                        catch: Some(MirBlock::new()),
                        finally: None,
                    }],
                },
            }],
        },
    };
    let try_s = emit_function(&try_loop, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        try_s.contains("continue '__while_"),
        "the try fallback for a `continue` escaping a try inside a while must target the while's \
         label, got:\n{try_s}"
    );
}
