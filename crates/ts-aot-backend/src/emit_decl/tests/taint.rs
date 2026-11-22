use super::super::emit_decls;
use super::{
    capture_free_closure, capturing_closure, empty_func, normalize_token_whitespace,
    self_capture_probe, self_capture_probe_with_env,
};
use ts_aot_core::{
    Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, MirStructDecl, RuntimeOp,
};

#[test]
fn self_capture_rebind_to_plain_value_is_not_an_escape() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Assign {
                target: MirPlace::Local { id: f },
                value: MirExpr::Int { value: 1, ty },
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "a local rebound to a value that cannot hold the capture must not be treated as escaping"
    );
}

#[test]
fn self_capture_escape_before_rebind_is_still_refused() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Assign {
                target: MirPlace::Global(Atom::from("leak")),
                value: MirExpr::Local(f),
            },
            MirStmt::Assign {
                target: MirPlace::Local { id: f },
                value: MirExpr::Int { value: 1, ty },
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "an escape observed before the rebind must still be refused"
    );
}

#[test]
fn self_capture_taint_survives_rebind_on_one_branch() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::If {
                cond: MirExpr::Bool(true),
                then_block: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: f },
                        value: MirExpr::Int { value: 1, ty },
                    }],
                },
                else_block: None,
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "a rebind on one branch must not clear taint established before the branch"
    );
}

#[test]
fn self_capture_taint_survives_rebind_inside_loop() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::While {
                cond: MirExpr::Bool(false),
                body: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: f },
                        value: MirExpr::Int { value: 1, ty },
                    }],
                },
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "a rebind inside a loop must not clear taint established before the loop"
    );
}

#[test]
fn self_capture_taint_carried_by_a_loop_back_edge_is_refused() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: None,
                mutable: true,
            },
            MirStmt::While {
                cond: MirExpr::Bool(true),
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Assign {
                            target: MirPlace::Field {
                                base: Box::new(MirPlaceBase::Local(self_param)),
                                field: slot,
                                ty,
                            },
                            value: MirExpr::Local(f),
                        },
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: capturing_closure(self_param, ty),
                        },
                    ],
                },
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "the loop body stores the closure into `f` after reading `f` into the receiver's field, so \
         on the second iteration the capture reaches the field and escapes; a single pass with the \
         pre-loop taint misses it"
    );
}

#[test]
fn self_capture_rebind_inside_loop_clears_taint_before_the_escape() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::While {
                cond: MirExpr::Bool(true),
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: MirExpr::Int { value: 1, ty },
                        },
                        MirStmt::Assign {
                            target: MirPlace::Field {
                                base: Box::new(MirPlaceBase::Local(self_param)),
                                field: slot,
                                ty,
                            },
                            value: MirExpr::Local(f),
                        },
                    ],
                },
            },
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "the rebind at the top of every iteration clears `f` before the field store reads it, so \
         no iteration ever carries the capture into the receiver's field; the loop fixpoint must \
         keep applying the kill instead of unioning the pre-loop taint into every statement"
    );
}

#[test]
fn self_capture_taint_returns_on_a_loop_back_edge_past_a_rebind() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let g = LocalId::from_raw(2);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Let {
                local: g,
                ty,
                init: None,
                mutable: true,
            },
            MirStmt::While {
                cond: MirExpr::Bool(true),
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Assign {
                            target: MirPlace::Field {
                                base: Box::new(MirPlaceBase::Local(self_param)),
                                field: slot,
                                ty,
                            },
                            value: MirExpr::Local(g),
                        },
                        MirStmt::Assign {
                            target: MirPlace::Local { id: g },
                            value: MirExpr::Local(f),
                        },
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: MirExpr::Int { value: 1, ty },
                        },
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: capturing_closure(self_param, ty),
                        },
                    ],
                },
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "the first iteration copies the tainted `f` into `g` before `f` is rebound, and the back \
         edge carries that taint into the next iteration's field store; the rebind inside the body \
         must not stop the fixpoint from feeding the body's taint back into the loop header, so a \
         single pass over the body misses the escape the fixpoint finds"
    );
}

#[test]
fn for_await_of_item_tainted_from_the_iterable_is_an_escape() {
    let self_param = LocalId::from_raw(0);
    let source = LocalId::from_raw(1);
    let item = LocalId::from_raw(2);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: source,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: false,
            },
            MirStmt::ForAwaitOf {
                item,
                iterable: MirExpr::Local(source),
                iter_ty: ty,
                body: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Field {
                            base: Box::new(MirPlaceBase::Local(self_param)),
                            field: slot,
                            ty,
                        },
                        value: MirExpr::Local(item),
                    }],
                },
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "an async iterable that may hold the self-capture makes every `for await` item hold it \
         too, so storing the item into the receiver's field escapes; `for await` must taint the \
         item from the iterable exactly like `for of` does"
    );
}

#[test]
fn an_escape_inside_catch_sees_the_taint_the_try_body_produced() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: None,
                mutable: true,
            },
            MirStmt::Try {
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: capturing_closure(self_param, ty),
                        },
                        MirStmt::Throw {
                            error: MirExpr::Int { value: 1, ty },
                            error_ty: ty,
                        },
                    ],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Field {
                            base: Box::new(MirPlaceBase::Local(self_param)),
                            field: slot,
                            ty,
                        },
                        value: MirExpr::Local(f),
                    }],
                }),
                finally: None,
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "the try body may throw after binding the capture into `f`, so the catch handler must be \
         scanned with the taint the try body produced as well as the taint from before the try; \
         scanning the catch with the pre-try taint alone misses the escape"
    );
}

#[test]
fn self_capture_escape_at_a_try_throw_point_survives_a_later_rebind() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: None,
                mutable: true,
            },
            MirStmt::Try {
                body: MirBlock {
                    stmts: vec![
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: capturing_closure(self_param, ty),
                        },
                        MirStmt::Throw {
                            error: MirExpr::Int { value: 1, ty },
                            error_ty: ty,
                        },
                        MirStmt::Assign {
                            target: MirPlace::Local { id: f },
                            value: MirExpr::Int { value: 2, ty },
                        },
                    ],
                },
                catch_param: None,
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Field {
                            base: Box::new(MirPlaceBase::Local(self_param)),
                            field: slot,
                            ty,
                        },
                        value: MirExpr::Local(f),
                    }],
                }),
                finally: None,
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "the throwing statement in the try body runs while `f` still holds the capture, and the \
         catch then stores `f` into the receiver's field; the rebind that follows the throw on the \
         normal path empties the taint at the end of the body, so scanning the catch with the \
         end-of-body state alone misses the escape"
    );
}

#[test]
fn weakmap_liveness_refresh_on_reassignment_is_guarded_by_key_identity() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let key_struct_id = StructId::from_raw(7);
    let key_field_id = FieldId::from_raw(0);
    let key_struct_ty = types.intern(&Type::Struct { id: key_struct_id });
    let wm_handle_ty = types.intern(&Type::WeakMap {
        key: key_struct_ty,
        value: i64_ty,
    });
    let wm_param = LocalId::from_raw(0);
    let key_param = LocalId::from_raw(1);
    let mut f = empty_func("reassign_key");
    f.ret = i64_ty;
    f.params = vec![
        MirParam {
            id: wm_param,
            name: Atom::from("wm"),
            ty: wm_handle_ty,
        },
        MirParam {
            id: key_param,
            name: Atom::from("new_k"),
            ty: key_struct_ty,
        },
    ];
    let key_local = LocalId::from_raw(2);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: key_local,
            name: Atom::from("k"),
            ty: key_struct_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: key_local,
                    ty: key_struct_ty,
                    init: Some(MirExpr::Local(key_param)),
                    mutable: true,
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
                    value: MirExpr::Local(key_param),
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
            id: key_field_id,
            name: Atom::from("v"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let fn_body = normalize_token_whitespace(
        s.split("fn reassign_key")
            .nth(1)
            .and_then(|rest| rest.split("return").next())
            .unwrap_or(""),
    );
    assert!(
        fn_body.contains("let mut __wm_liveness_0 : std :: rc :: Rc < () >"),
        "WeakMap key local must be backed by a let mut liveness binding (identity must be refreshable on reassignment). Got body fragment: {fn_body}"
    );
    let compact: String = fn_body.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("Rc::as_ptr(&k.v)"),
        "the refresh decision must compare the key's object identity (the shared field cell of the \
         struct value), got body fragment: {fn_body}"
    );
    assert!(
        compact.contains("if__ts_aot_weakmap_key_"),
        "the liveness token must be refreshed conditionally, only when the assigned object's \
         identity differs, got body fragment: {fn_body}"
    );
    let probe_pos = compact.find("let__ts_aot_weakmap_key_");
    let assign_pos = compact.find("k=new_k");
    let reset_pos = compact.find("__wm_liveness_0=std::rc::Rc::new(())");
    let first_set_pos = compact.find("__ts_aot_weak_map_set");
    let second_set_pos = compact.rfind("__ts_aot_weak_map_set");
    assert!(
        first_set_pos.is_some()
            && second_set_pos.is_some()
            && probe_pos.is_some()
            && assign_pos.is_some()
            && reset_pos.is_some(),
        "expected prelude + identity probe + assignment + guarded liveness refresh + two WeakMapSet \
         calls in body; got first_set={first_set_pos:?} second_set={second_set_pos:?} \
         probe={probe_pos:?} assign={assign_pos:?} reset={reset_pos:?}. body={fn_body}"
    );
    let (set1, probe, assign, reset, set2) = (
        first_set_pos.expect("set"),
        probe_pos.expect("probe"),
        assign_pos.expect("assign"),
        reset_pos.expect("reset"),
        second_set_pos.expect("set2"),
    );
    assert!(
        set1 < probe && probe < assign && assign < reset && reset < set2,
        "expected WeakMapSet -> identity probe -> Assign -> guarded refresh -> WeakMapSet ordering \
         so the second set sees the same identity when the object is unchanged; got positions \
         set={set1} probe={probe} assign={assign} reset={reset} set2={set2}. body={fn_body}"
    );
    assert!(
        !compact.contains(";__wm_liveness_0=std::rc::Rc::new(())"),
        "reassigning the key local to the same object must not unconditionally install a fresh \
         liveness token, got body fragment: {fn_body}"
    );
}

#[test]
fn weakmap_liveness_keeps_a_self_assigned_field_less_key() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let key_struct_id = StructId::from_raw(9);
    let key_struct_ty = types.intern(&Type::Struct { id: key_struct_id });
    let wm_handle_ty = types.intern(&Type::WeakMap {
        key: key_struct_ty,
        value: i64_ty,
    });
    let wm_param = LocalId::from_raw(0);
    let key_param = LocalId::from_raw(1);
    let mut f = empty_func("reassign_empty_key");
    f.ret = i64_ty;
    f.params = vec![
        MirParam {
            id: wm_param,
            name: Atom::from("wm"),
            ty: wm_handle_ty,
        },
        MirParam {
            id: key_param,
            name: Atom::from("new_k"),
            ty: key_struct_ty,
        },
    ];
    let key_local = LocalId::from_raw(2);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: key_local,
            name: Atom::from("k"),
            ty: key_struct_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: key_local,
                    ty: key_struct_ty,
                    init: Some(MirExpr::Local(key_param)),
                    mutable: true,
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
                    value: MirExpr::Local(key_local),
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
                MirStmt::Assign {
                    target: MirPlace::Local { id: key_local },
                    value: MirExpr::Local(key_param),
                },
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapSet,
                    args: vec![
                        MirExpr::Local(wm_param),
                        MirExpr::Local(key_local),
                        MirExpr::Int {
                            value: 3,
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
        name: Atom::from("Empty"),
        fields: Vec::new(),
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let fn_body = normalize_token_whitespace(
        s.split("fn reassign_empty_key")
            .nth(1)
            .and_then(|rest| rest.split("return").next())
            .unwrap_or(""),
    );
    assert!(
        fn_body.contains("let mut __wm_liveness_0 : std :: rc :: Rc < () >"),
        "the field-less key local must still be backed by a liveness binding; got body fragment: \
         {fn_body}"
    );
    let compact: String = fn_body.chars().filter(|c| !c.is_whitespace()).collect();
    let reset = "__wm_liveness_0=std::rc::Rc::new(())";
    let self_assign = compact
        .find("k=k.clone();")
        .unwrap_or_else(|| panic!("the self-assignment must still emit the write; body={fn_body}"));
    let foreign_assign = compact.find("k=new_k.clone();").unwrap_or_else(|| {
        panic!("binding the key to a different object must emit the write; body={fn_body}")
    });
    let reset_pos = compact
        .find(reset)
        .unwrap_or_else(|| panic!("a genuinely different key object must refresh the token"));
    assert!(
        self_assign < foreign_assign && foreign_assign < reset_pos,
        "expected self-assignment, then the different-object assignment, then a single refresh; \
         got self={self_assign} foreign={foreign_assign} reset={reset_pos}. body={fn_body}"
    );
    assert!(
        !compact[self_assign..foreign_assign].contains(reset),
        "`k = k` cannot change which object the key holds, so it must keep the liveness token (and \
         the entry stored under it) instead of purging it; got body fragment: {fn_body}"
    );
    assert_eq!(
        &compact[foreign_assign..reset_pos + reset.len()],
        "k=new_k.clone();__wm_liveness_0=std::rc::Rc::new(())",
        "binding the key to a different object still has to refresh the token unconditionally, \
         because a field-less struct carries no identity to compare; got body fragment: {fn_body}"
    );
    assert!(
        !compact.contains("Rc::as_ptr(&k"),
        "a key type without an identity probe (a struct with no fields) must not emit a key \
         identity comparison between two values it cannot distinguish; got body fragment: {fn_body}"
    );
}

#[test]
fn weakmap_liveness_unaffected_by_field_assignment_on_key_local() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let owner = StructId::from_raw(3);
    let field_id = FieldId::from_raw(0);
    let inner_ty = types.intern(&Type::Struct { id: owner });
    let wm_handle_ty = types.intern(&Type::WeakMap {
        key: inner_ty,
        value: i64_ty,
    });
    let wm_param = LocalId::from_raw(0);
    let key_local = LocalId::from_raw(1);
    let mut f = empty_func("field_assign_key");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: wm_param,
        name: Atom::from("wm"),
        ty: wm_handle_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: key_local,
            name: Atom::from("k"),
            ty: inner_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local: key_local,
                    ty: inner_ty,
                    init: None,
                    mutable: true,
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
                    target: MirPlace::Field {
                        base: Box::new(MirPlaceBase::Local(key_local)),
                        field: field_id,
                        ty: i64_ty,
                    },
                    value: MirExpr::Int {
                        value: 7,
                        ty: i64_ty,
                    },
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
        id: owner,
        name: Atom::from("K"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("v"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let fn_body = normalize_token_whitespace(
        s.split("fn field_assign_key")
            .nth(1)
            .and_then(|rest| rest.split("return").next())
            .unwrap_or(""),
    );
    assert!(
        fn_body.contains("let mut __wm_liveness_0 : std :: rc :: Rc < () >"),
        "prelude must declare the liveness binding for the WeakMap key local. Got body fragment: {fn_body}"
    );
    assert!(
        !fn_body.contains("__wm_liveness_0 = std :: rc :: Rc :: new (()) ;"),
        "field assignment on the key local must NOT refresh the liveness (the local identity is unchanged); got body fragment: {fn_body}"
    );
}

#[test]
fn store_through_a_receiver_alias_is_an_escape() {
    let self_param = LocalId::from_raw(0);
    let alias = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: alias,
                ty,
                init: Some(MirExpr::Local(self_param)),
                mutable: false,
            },
            MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(alias)),
                    field: slot,
                    ty,
                },
                value: capturing_closure(self_param, ty),
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "a local proven to alias the receiver (`const alias = this`) is a receiver root, so \
         storing a closure that captures `this` into a field reached through it must be reported \
         as an escape"
    );
}

#[test]
fn store_into_an_unrelated_local_object_is_not_an_escape() {
    let self_param = LocalId::from_raw(0);
    let object = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: object,
                ty,
                init: None,
                mutable: false,
            },
            MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(object)),
                    field: slot,
                    ty,
                },
                value: capturing_closure(self_param, ty),
            },
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "a local that is not proven to alias the receiver must stay distinct, so storing the \
         callback into it does not escape the method frame"
    );
}

#[test]
fn if_else_both_rebinding_to_capture_free_clears_capture_taint() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::If {
                cond: MirExpr::Bool(true),
                then_block: MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: f },
                        value: capture_free_closure(ty),
                    }],
                },
                else_block: Some(MirBlock {
                    stmts: vec![MirStmt::Assign {
                        target: MirPlace::Local { id: f },
                        value: capture_free_closure(ty),
                    }],
                }),
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "when both branches of an `if/else` replace the capturing callback with a capture-free \
         one, the incoming taint must be discarded rather than unioned with the branch exits, so \
         the returned callback cannot escape"
    );
}

#[test]
fn self_assignment_preserves_capture_taint() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Assign {
                target: MirPlace::Local { id: f },
                value: MirExpr::Local(f),
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "`f = f` must be a no-op, so the capturing closure bound to `f` is still returned and the \
         escape must be reported"
    );
}

#[test]
fn cast_wrapped_self_assignment_preserves_capture_taint() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Assign {
                target: MirPlace::Local { id: f },
                value: MirExpr::Cast {
                    expr: Box::new(MirExpr::Local(f)),
                    ty,
                },
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "a cast-wrapped self-assignment still reads the local being assigned, so it must preserve \
         the capture taint rather than clearing it"
    );
}

#[test]
fn rebinding_to_a_capture_free_closure_clears_capture_taint() {
    let self_param = LocalId::from_raw(0);
    let f = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: f,
                ty,
                init: Some(capturing_closure(self_param, ty)),
                mutable: true,
            },
            MirStmt::Assign {
                target: MirPlace::Local { id: f },
                value: capture_free_closure(ty),
            },
            MirStmt::Return(Some(MirExpr::Local(f))),
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "rebinding `f` to a capture-free closure must still clear its capture taint"
    );
}

#[test]
fn array_from_mapped_mapper_returning_self_capture_taints_the_destination() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let fn_ty = types.intern(&Type::Fn {
        params: vec![i32_ty],
        optional: vec![false],
        ret: i32_ty,
        err: None,
    });
    let result_ty = types.intern(&Type::Array { element: fn_ty });
    let prog = MirProgram::new(ModuleId::from_raw(0));
    let self_param = LocalId::from_raw(0);
    let source = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Runtime {
                op: RuntimeOp::ArrayFromMapped,
                args: vec![MirExpr::Local(source), capturing_closure(self_param, fn_ty)],
                dest: Some(dest),
                ty: result_ty,
                target_ty: None,
            },
            MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: slot,
                    ty: fn_ty,
                },
                value: MirExpr::Local(dest),
            },
        ],
    };
    assert!(
        self_capture_probe_with_env(&block, self_param, &types, &prog),
        "a mapper whose return value is a closure that captures `this` stores that capture into \
         the mapped array, so the destination must be tainted and storing it into the receiver's \
         field must be reported as an escape"
    );
}

#[test]
fn push_through_a_receiver_field_alias_is_an_escape() {
    let self_param = LocalId::from_raw(0);
    let alias = LocalId::from_raw(1);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: alias,
                ty,
                init: Some(MirExpr::Field {
                    base: Box::new(MirExpr::Local(self_param)),
                    field: slot,
                    ty,
                }),
                mutable: false,
            },
            MirStmt::Runtime {
                op: RuntimeOp::ArrayPush,
                args: vec![MirExpr::Local(alias), capturing_closure(self_param, ty)],
                dest: None,
                ty,
                target_ty: None,
            },
        ],
    };
    assert!(
        self_capture_probe(&block, self_param),
        "a local initialized from a receiver field aliases receiver-owned storage, so pushing a \
         closure that captures `this` through it must be reported as an escape"
    );
}

#[test]
fn push_through_an_alias_of_an_unrelated_object_is_not_an_escape() {
    let self_param = LocalId::from_raw(0);
    let other = LocalId::from_raw(1);
    let alias = LocalId::from_raw(2);
    let ty = TypeId::from_raw(0);
    let slot = FieldId::from_raw(0);
    let block = MirBlock {
        stmts: vec![
            MirStmt::Let {
                local: other,
                ty,
                init: None,
                mutable: false,
            },
            MirStmt::Let {
                local: alias,
                ty,
                init: Some(MirExpr::Field {
                    base: Box::new(MirExpr::Local(other)),
                    field: slot,
                    ty,
                }),
                mutable: false,
            },
            MirStmt::Runtime {
                op: RuntimeOp::ArrayPush,
                args: vec![MirExpr::Local(alias), capturing_closure(self_param, ty)],
                dest: None,
                ty,
                target_ty: None,
            },
        ],
    };
    assert!(
        !self_capture_probe(&block, self_param),
        "a field of a local that is not rooted in the receiver is unrelated storage, so pushing \
         the callback through it does not escape the method frame"
    );
}
