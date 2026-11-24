use super::super::emit_decls;
use super::{empty_func, norm};
use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, Type, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, MirStructDecl,
};

#[test]
fn mutating_method_receiver_index_precedes_staged_arguments() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let owner = StructId::from_raw(1);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let arr_ty = types.intern(&Type::Array { element: struct_ty });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let arr_param = LocalId::from_raw(2);
    let bump_id = FunctionId::from_raw(10);
    let next_id = FunctionId::from_raw(11);
    let arg_id = FunctionId::from_raw(12);

    let mut bump = empty_func("bump");
    bump.id = bump_id;
    bump.kind = FunctionKind::Method { owner, self_param };
    bump.params = vec![
        MirParam {
            id: self_param,
            name: Atom::from("self"),
            ty: struct_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: i32_ty,
        },
    ];
    bump.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: field_id,
                    ty: i32_ty,
                },
                value: MirExpr::Local(value_param),
            }],
        },
    };

    let mut next = empty_func("next_index");
    next.id = next_id;
    next.ret = i32_ty;
    next.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 0,
                ty: i32_ty,
            }))],
        },
    };

    let mut arg = empty_func("arg_value");
    arg.id = arg_id;
    arg.ret = i32_ty;
    arg.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 1,
                ty: i32_ty,
            }))],
        },
    };

    let mut pick = empty_func("pick");
    pick.id = FunctionId::from_raw(13);
    pick.ret = i32_ty;
    pick.params = vec![MirParam {
        id: arr_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    pick.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Call {
                callee: bump_id,
                args: vec![
                    MirExpr::Index {
                        base: Box::new(MirExpr::Local(arr_param)),
                        index: Box::new(MirExpr::Call {
                            callee: next_id,
                            args: Vec::new(),
                            ty: i32_ty,
                        }),
                        ty: struct_ty,
                    },
                    MirExpr::Call {
                        callee: arg_id,
                        args: Vec::new(),
                        ty: i32_ty,
                    },
                ],
                ty: i32_ty,
            })],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![bump],
    }));
    prog.push_decl(MirDecl::Function(next));
    prog.push_decl(MirDecl::Function(arg));
    prog.push_decl(MirDecl::Function(pick));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn pick").nth(1).unwrap_or(""));
    let index_call_at = body
        .find("next_index(__ctx)")
        .unwrap_or_else(|| panic!("the receiver index call must be emitted, got: {body}"));
    let arg_call_at = body
        .find("arg_value(__ctx)")
        .unwrap_or_else(|| panic!("the staged argument call must be emitted, got: {body}"));
    assert!(
        index_call_at < arg_call_at,
        "arr[nextIndex()] evaluates the receiver index before the staged argument, so the \
         `next_index(__ctx)` call must be emitted before `arg_value(__ctx)`; got: {body}"
    );
    assert!(
        body.contains("arr[__ts_place_index"),
        "the receiver must reuse the hoisted index local instead of re-emitting the call; got: \
         {body}"
    );
    assert_eq!(
        body.matches("next_index(__ctx)").count(),
        1,
        "the receiver index expression must be evaluated exactly once; got: {body}"
    );
}

#[test]
fn mutating_method_local_receiver_is_captured_before_staged_arguments() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let owner = StructId::from_raw(1);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let field_id = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let recv_param = LocalId::from_raw(2);
    let bump_id = FunctionId::from_raw(30);
    let arg_id = FunctionId::from_raw(31);

    let mut bump = empty_func("bump");
    bump.id = bump_id;
    bump.kind = FunctionKind::Method { owner, self_param };
    bump.params = vec![
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
    bump.body = MirBody {
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

    let mut arg = empty_func("arg_value");
    arg.id = arg_id;
    arg.ret = f64_ty;
    arg.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Float {
                value: 1.0,
                ty: f64_ty,
            }))],
        },
    };

    let mut pick = empty_func("pick");
    pick.id = FunctionId::from_raw(32);
    pick.ret = f64_ty;
    pick.params = vec![MirParam {
        id: recv_param,
        name: Atom::from("b"),
        ty: struct_ty,
    }];
    pick.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Call {
                    callee: bump_id,
                    args: vec![
                        MirExpr::Local(recv_param),
                        MirExpr::Call {
                            callee: arg_id,
                            args: Vec::new(),
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
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: field_id,
            name: Atom::from("x"),
            ty: f64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![bump],
    }));
    prog.push_decl(MirDecl::Function(arg));
    prog.push_decl(MirDecl::Function(pick));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn pick").nth(1).unwrap_or(""));
    let capture_at = body.find("let__ts_recv=(b).clone();").unwrap_or_else(|| {
        panic!(
            "a local receiver must be cloned into a hoisted handle in a statement of its own, so \
             the receiver is selected before the staged arguments run and any environment borrow \
             ends before the call; got: {body}"
        )
    });
    let arg_at = body
        .find("let__ts_arg_0=")
        .unwrap_or_else(|| panic!("the argument must be staged, got: {body}"));
    assert!(
        capture_at < arg_at,
        "`b.bump(argValue())` selects the receiver before the arguments, so the cloned receiver \
         must be bound ahead of the staged argument binds; got: {body}"
    );
    assert!(
        body.contains("Box::bump(&(__ts_recv),__ctx,__ts_arg_0)"),
        "the call must use the captured handle instead of re-reading the receiver after the \
         arguments ran; got: {body}"
    );
}

#[test]
fn mutating_method_field_over_index_receiver_precedes_staged_arguments() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let inner_owner = StructId::from_raw(1);
    let outer_owner = StructId::from_raw(2);
    let inner_ty = types.intern(&Type::Struct { id: inner_owner });
    let outer_ty = types.intern(&Type::Struct { id: outer_owner });
    let arr_ty = types.intern(&Type::Array { element: outer_ty });
    let inner_field = FieldId::from_raw(0);
    let outer_field = FieldId::from_raw(0);
    let self_param = LocalId::from_raw(0);
    let value_param = LocalId::from_raw(1);
    let arr_param = LocalId::from_raw(2);
    let bump_id = FunctionId::from_raw(20);
    let next_id = FunctionId::from_raw(21);
    let arg_id = FunctionId::from_raw(22);

    let mut bump = empty_func("bump");
    bump.id = bump_id;
    bump.kind = FunctionKind::Method {
        owner: inner_owner,
        self_param,
    };
    bump.params = vec![
        MirParam {
            id: self_param,
            name: Atom::from("self"),
            ty: inner_ty,
        },
        MirParam {
            id: value_param,
            name: Atom::from("v"),
            ty: i32_ty,
        },
    ];
    bump.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(self_param)),
                    field: inner_field,
                    ty: i32_ty,
                },
                value: MirExpr::Local(value_param),
            }],
        },
    };

    let mut next = empty_func("next_index");
    next.id = next_id;
    next.ret = i32_ty;
    next.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 0,
                ty: i32_ty,
            }))],
        },
    };

    let mut arg = empty_func("arg_value");
    arg.id = arg_id;
    arg.ret = i32_ty;
    arg.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 1,
                ty: i32_ty,
            }))],
        },
    };

    let mut pick = empty_func("pick");
    pick.id = FunctionId::from_raw(23);
    pick.ret = i32_ty;
    pick.params = vec![MirParam {
        id: arr_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    pick.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Call {
                callee: bump_id,
                args: vec![
                    MirExpr::Field {
                        base: Box::new(MirExpr::Index {
                            base: Box::new(MirExpr::Local(arr_param)),
                            index: Box::new(MirExpr::Call {
                                callee: next_id,
                                args: Vec::new(),
                                ty: i32_ty,
                            }),
                            ty: outer_ty,
                        }),
                        field: outer_field,
                        ty: inner_ty,
                    },
                    MirExpr::Call {
                        callee: arg_id,
                        args: Vec::new(),
                        ty: i32_ty,
                    },
                ],
                ty: i32_ty,
            })],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: inner_owner,
        name: Atom::from("Inner"),
        fields: vec![MirFieldDecl {
            id: inner_field,
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: vec![bump],
    }));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: outer_owner,
        name: Atom::from("Outer"),
        fields: vec![MirFieldDecl {
            id: outer_field,
            name: Atom::from("inner"),
            ty: inner_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(next));
    prog.push_decl(MirDecl::Function(arg));
    prog.push_decl(MirDecl::Function(pick));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let body = norm(s.split("fn pick").nth(1).unwrap_or(""));
    let index_call_at = body
        .find("next_index(__ctx)")
        .unwrap_or_else(|| panic!("the receiver index call must be emitted, got: {body}"));
    let arg_call_at = body
        .find("arg_value(__ctx)")
        .unwrap_or_else(|| panic!("the staged argument call must be emitted, got: {body}"));
    assert!(
        index_call_at < arg_call_at,
        "a field receiver over an indexed base must emit the `next_index(__ctx)` call before the \
         `arg_value(__ctx)` call; got: {body}"
    );
    assert!(
        body.contains("[__ts_place_index") && body.matches("next_index(__ctx)").count() == 1,
        "the field receiver must index the array through the hoisted local exactly once; got: \
         {body}"
    );
}
