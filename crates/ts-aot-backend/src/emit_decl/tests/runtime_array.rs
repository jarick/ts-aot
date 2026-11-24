use super::super::emit_decls;
use super::{empty_func, norm, signature_fragment};
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirParam, MirProgram, MirStmt, RuntimeOp,
};

#[test]
fn array_len_borrows_plain_array_receiver() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let arr_param = LocalId::from_raw(0);
    let temp = LocalId::from_raw(1);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let mut f = empty_func("len_of");
    f.ret = i64_ty;
    f.params = vec![MirParam {
        id: arr_param,
        name: Atom::from("a"),
        ty: arr_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: temp,
            name: Atom::from(""),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayLen,
                args: vec![MirExpr::Local(arr_param)],
                dest: Some(temp),
                ty: i64_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_array_len (& a)"),
        "an ordinary array local must be passed by reference (slice) to the array-length helper, got: {s}"
    );
}

#[test]
fn array_len_maps_over_optional_chain_receiver() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let opt_arr_ty = types.intern(&Type::Optional {
        inner: arr_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let opt_i64_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let arr_param = LocalId::from_raw(0);
    let temp = LocalId::from_raw(1);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    let mut f = empty_func("len_opt");
    f.ret = opt_i64_ty;
    f.params = vec![MirParam {
        id: arr_param,
        name: Atom::from("a"),
        ty: opt_arr_ty,
    }];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: temp,
            name: Atom::from(""),
            ty: opt_i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayLen,
                args: vec![MirExpr::OptionalChain {
                    base: Box::new(MirExpr::Local(arr_param)),
                    ty: opt_arr_ty,
                }],
                dest: Some(temp),
                ty: opt_i64_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains(". as_ref () . map (| o | __ts_aot_array_len (o))"),
        "an optional receiver must map the array-length helper over `as_ref()` (Some-only), got: {s}"
    );
    assert!(
        s.contains("Option < i64 >"),
        "the optional array-length result must stay optional, got: {s}"
    );
}

#[test]
fn array_push_runtime_marks_array_param_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("grow");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    args: vec![MirExpr::Local(array_param), MirExpr::Local(value)],
                    dest: None,
                    ty: arr_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(value))),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "grow");
    assert!(
        sig.contains("mut arr"),
        "param passed as `args[0]` to RuntimeOp::ArrayPush must emit `mut arr` in the function \
         signature (array push needs &mut Vec). Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_push_runtime_clones_noncopy_item() {
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let arr_ty = types.intern(&Type::Array { element: str_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("grow");
    f.ret = str_ty;
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: str_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    args: vec![MirExpr::Local(array_param), MirExpr::Local(value)],
                    dest: None,
                    ty: arr_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(value))),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("decls should emit")
        .to_string();
    assert!(
        s.contains("__ts_aot_array_push (& mut arr , v . clone ())"),
        "a non-Copy item consumed by value by RuntimeOp::ArrayPush must be cloned so the source \
         stays usable; got:\n{s}"
    );
}

#[test]
fn array_set_runtime_marks_array_param_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArraySet,
                args: vec![
                    MirExpr::Local(array_param),
                    MirExpr::Int {
                        value: 0,
                        ty: i32_ty,
                    },
                    MirExpr::Local(value),
                ],
                dest: None,
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke");
    assert!(
        sig.contains("mut arr"),
        "param passed as `args[0]` to RuntimeOp::ArraySet must emit `mut arr` in the function \
         signature (array indexed-set needs &mut [T]). Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_set_runtime_call_emits_mut_borrow() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let arr_ty = types.intern(&Type::Array { element: i32_ty });
    let array_param = LocalId::from_raw(0);
    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: array_param,
        name: Atom::from("arr"),
        ty: arr_ty,
    }];
    let value = LocalId::from_raw(1);
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: value,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArraySet,
                args: vec![
                    MirExpr::Local(array_param),
                    MirExpr::Int {
                        value: 0,
                        ty: i32_ty,
                    },
                    MirExpr::Local(value),
                ],
                dest: None,
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_array_set (& mut arr"),
        "RuntimeOp::ArraySet call site must emit `&mut arr` to match __ts_aot_array_set's \
         &mut [T] parameter (dedicated arm in emit_runtime_call). Got emitted code: `{s}`"
    );
}

#[test]
fn array_get_or_default_evaluates_the_receiver_before_the_index() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let i64_ty = types.intern(&Type::I64);
    let row_ty = types.intern(&Type::Array { element: i32_ty });
    let grid_ty = types.intern(&Type::Array { element: row_ty });
    let grid_param = LocalId::from_raw(0);
    let which_param = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let advance_id = FunctionId::from_raw(21);
    let mut advance = empty_func("advance");
    advance.id = advance_id;
    advance.ret = i64_ty;
    advance.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 1,
                ty: i64_ty,
            }))],
        },
    };
    let mut f = empty_func("pick");
    f.ret = i32_ty;
    f.params = vec![
        MirParam {
            id: grid_param,
            name: Atom::from("grid"),
            ty: grid_ty,
        },
        MirParam {
            id: which_param,
            name: Atom::from("which"),
            ty: i64_ty,
        },
    ];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("v"),
            ty: i32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayGetOrDefault,
                args: vec![
                    MirExpr::Index {
                        base: Box::new(MirExpr::Local(grid_param)),
                        index: Box::new(MirExpr::Local(which_param)),
                        ty: row_ty,
                    },
                    MirExpr::Call {
                        callee: advance_id,
                        args: Vec::new(),
                        ty: i64_ty,
                    },
                ],
                dest: Some(dest),
                ty: i32_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(advance));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = norm(&tokens.to_string());
    let receiver_at = s
        .find("grid[whichasusize]")
        .expect("the receiver must select the row before the index runs");
    let index_at = s
        .find("advance(__ctx)")
        .expect("the index expression must be evaluated");
    let lookup_at = s
        .find("__ts_aot_array_get_or_default")
        .expect("the lookup must use the bound receiver and index");
    assert_eq!(
        s.matches("grid[whichasusize]").count(),
        1,
        "the receiver must be selected exactly once, before the index can change the selection; \
         got: {s}"
    );
    assert!(
        receiver_at < index_at,
        "`arr[i]` evaluates the receiver before the index, so the row selected by `which` must be \
         read before the index call runs; got: {s}"
    );
    assert!(
        index_at < lookup_at,
        "the lookup must run after both the receiver and the index have been evaluated; got: {s}"
    );
}

#[test]
fn map_set_runtime_marks_map_param_mut() {
    let mut types = TypeTable::new();
    let str_ty = types.intern(&Type::String);
    let map_ty_placeholder = types.intern(&Type::Error);
    let map_param = LocalId::from_raw(0);
    let mut f = empty_func("set_kv");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: map_param,
        name: Atom::from("m"),
        ty: map_ty_placeholder,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::MapSet,
                args: vec![
                    MirExpr::Local(map_param),
                    MirExpr::String {
                        id: Atom::from("k"),
                        ty: str_ty,
                    },
                    MirExpr::String {
                        id: Atom::from("v"),
                        ty: str_ty,
                    },
                ],
                dest: None,
                ty: map_ty_placeholder,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "set_kv");
    assert!(
        sig.contains("mut m"),
        "param passed as `args[0]` to RuntimeOp::MapSet must emit `mut m` in the function \
         signature (map set needs &mut map). Got signature fragment: `{sig}`"
    );
}

#[test]
fn array_index_of_float_needle_rejects_the_first_integer_past_max() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let array_param = LocalId::from_raw(0);
    let needle_param = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let mut f = empty_func("find");
    f.ret = f64_ty;
    f.params = vec![
        MirParam {
            id: array_param,
            name: Atom::from("arr"),
            ty: arr_ty,
        },
        MirParam {
            id: needle_param,
            name: Atom::from("needle"),
            ty: f64_ty,
        },
    ];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("v"),
            ty: f64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayIndexOf,
                args: vec![MirExpr::Local(array_param), MirExpr::Local(needle_param)],
                dest: Some(dest),
                ty: f64_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("indexOf over an i64 array must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("<i64>::MAXasf64+1.0"),
        "an i64 needle's upper bound must be exclusive at the first integer past i64::MAX, so \
         2^63 is rejected; got: {s}"
    );
    assert!(
        !norm.contains("<=<i64>::MAXasf64"),
        "an inclusive upper bound accepts 2^63 and saturates on the cast; got: {s}"
    );
}

#[test]
fn array_index_of_undefined_needle_searches_optional_elements() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let element_ty = types.intern(&Type::Optional {
        inner: i64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let needle_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let arr_ty = types.intern(&Type::Array {
        element: element_ty,
    });
    let array_param = LocalId::from_raw(0);
    let needle_param = LocalId::from_raw(1);
    let dest = LocalId::from_raw(2);
    let mut f = empty_func("find_undefined");
    f.ret = f64_ty;
    f.params = vec![
        MirParam {
            id: array_param,
            name: Atom::from("arr"),
            ty: arr_ty,
        },
        MirParam {
            id: needle_param,
            name: Atom::from("needle"),
            ty: needle_ty,
        },
    ];
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: dest,
            name: Atom::from("v"),
            ty: f64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayIndexOf,
                args: vec![MirExpr::Local(array_param), MirExpr::Local(needle_param)],
                dest: Some(dest),
                ty: f64_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types)
        .expect("indexOf with an undefined needle over optional elements must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let calls = norm.matches("__ts_aot_array_index_of").count();
    assert_eq!(
        calls, 2,
        "both the Some and None needle arms must search the optional-element array; got: {s}"
    );
    assert!(
        norm.contains("&::core::option::Option::None"),
        "the None needle arm must compare against `None` rather than returning -1; got: {s}"
    );
}
