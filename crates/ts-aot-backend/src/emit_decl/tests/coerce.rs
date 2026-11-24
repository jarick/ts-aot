use super::super::{emit_decls, emit_function};
use super::{empty_func, norm};
use ts_aot_core::{Atom, FunctionId, LocalId, ModuleId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{
    BinaryOp, MirBlock, MirBody, MirDecl, MirExpr, MirGlobalDecl, MirLocalDecl, MirParam, MirPlace,
    MirProgram, MirStmt, RuntimeOp,
};

#[test]
fn null_init_of_optional_string_local_emits_none() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("clear");
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(0),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(0),
                ty: opt_ty,
                init: Some(MirExpr::Null { ty: opt_ty }),
                mutable: true,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letmutx:Option<") && norm.contains("=::core::option::Option::None"),
        "initializing an optional-string local from `null` must emit `Option::None`, not `()`; \
         got: {s}"
    );
}

#[test]
fn let_of_string_into_optional_string_local_wraps_in_some() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("wrap");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("s"),
        ty: string_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(1),
                ty: opt_ty,
                init: Some(MirExpr::Local(LocalId::from_raw(0))),
                mutable: false,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letx:Option<") && norm.contains("=::core::option::Option::Some(s.clone())"),
        "initializing an optional-string local from a `string` must wrap the source parameter in \
         `Some(..)`; got: {s}"
    );
}

#[test]
fn assignment_of_string_to_optional_string_local_wraps_in_some() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let opt_ty = types.intern(&Type::Optional {
        inner: string_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("set");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("s"),
        ty: string_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local {
                    id: LocalId::from_raw(1),
                },
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x=::core::option::Option::Some(s.clone())"),
        "assigning a `string` to an optional-string local must wrap the source parameter in \
         `Some(..)`; got: {s}"
    );
}

#[test]
fn null_init_of_optional_number_local_emits_none() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("clear");
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(0),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(0),
                ty: opt_ty,
                init: Some(MirExpr::Null { ty: opt_ty }),
                mutable: true,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letmutx:Option<") && norm.contains("=::core::option::Option::None"),
        "initializing an optional-number local from `null` must emit `Option::None`, not `()`; \
         got: {s}"
    );
}

#[test]
fn let_of_number_into_optional_number_local_wraps_in_some() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("wrap");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("n"),
        ty: f64_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(1),
                ty: opt_ty,
                init: Some(MirExpr::Local(LocalId::from_raw(0))),
                mutable: false,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("letx:Option<") && norm.contains("=::core::option::Option::Some(n)"),
        "initializing an optional-number local from a `number` must wrap the source parameter in \
         `Some(..)`; got: {s}"
    );
}

#[test]
fn let_of_int_literal_into_optional_number_local_coerces_the_inner_value() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("wrap");
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(0),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(0),
                ty: opt_ty,
                init: Some(MirExpr::Int {
                    value: 3,
                    ty: i64_ty,
                }),
                mutable: false,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("=::core::option::Option::Some(3.0)"),
        "an `int` literal assigned to an optional-number local must be coerced to the inner \
         `f64` inside `Some(..)`; got: {s}"
    );
}

#[test]
fn assignment_of_number_to_optional_number_local_wraps_in_some() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("set");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("n"),
        ty: f64_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("x"),
            ty: opt_ty,
            mutable: true,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local {
                    id: LocalId::from_raw(1),
                },
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("x=::core::option::Option::Some(n)"),
        "assigning a `number` to an optional-number local must wrap the source parameter in \
         `Some(..)`; got: {s}"
    );
}

#[test]
fn assignment_of_number_to_optional_number_global_wraps_in_some() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("g"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("setG");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("n"),
        ty: f64_ty,
    }];
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("g")),
                value: MirExpr::Local(LocalId::from_raw(0)),
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{g=::core::option::Option::Some(n)"),
        "assigning a `number` to an optional-number global must wrap the source parameter in \
         `Some(..)` inside the `unsafe` write; got: {s}"
    );
}

#[test]
fn assignment_of_null_to_optional_number_global_writes_none() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let void_ty = types.intern(&Type::Void);
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Global(MirGlobalDecl {
        name: Atom::from("g"),
        ty: opt_ty,
        mutable: true,
        visibility: Visibility::Public,
        export_name: None,
        init: None,
    }));
    let mut f = empty_func("clearG");
    f.ret = void_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Global(Atom::from("g")),
                value: MirExpr::Null { ty: opt_ty },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("assignment must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("unsafe{g=::core::option::Option::None"),
        "assigning `null` to an optional-number global must write `Option::None`; got: {s}"
    );
}

#[test]
fn plain_float_into_optional_float_narrows_the_payload() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let f32_ty = types.intern(&Type::F32);
    let opt_f32_ty = types.intern(&Type::Optional {
        inner: f32_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut f = empty_func("narrow");
    f.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("n"),
        ty: f64_ty,
    }];
    f.ret = opt_f32_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("x"),
            ty: opt_f32_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(1),
                ty: opt_f32_ty,
                init: Some(MirExpr::Local(LocalId::from_raw(0))),
                mutable: false,
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("narrowing a float into an optional must emit");
    let s = tokens.to_string();
    let norm: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("Option::Some((n)asf32)"),
        "a plain `f64` stored into an optional `f32` must cast the payload to `f32`; got: {s}"
    );
}

fn typed_null_call_emission(types: &mut TypeTable, inner: &Type) -> String {
    let null_ty = types.intern(&Type::Null);
    let inner_ty = types.intern(inner);
    let target = types.intern(&Type::Optional {
        inner: inner_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let mut make_null = empty_func("make_null");
    make_null.id = FunctionId::from_raw(5);
    make_null.ret = null_ty;
    let mut f = empty_func("caller");
    f.ret = target;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(0),
            name: Atom::from("x"),
            ty: target,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Let {
                local: LocalId::from_raw(0),
                ty: target,
                init: Some(MirExpr::Call {
                    callee: FunctionId::from_raw(5),
                    args: Vec::new(),
                    ty: null_ty,
                }),
                mutable: false,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(make_null));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, types)
        .expect("a call typed `null` coerced into an optional target must emit")
        .to_string();
    norm(&s)
}

#[test]
fn typed_null_call_into_a_nested_optional_evaluates_the_source_once() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let s = typed_null_call_emission(
        &mut types,
        &Type::Optional {
            inner: f64_ty,
            nullish: ts_aot_core::NullishKinds::BOTH,
        },
    );
    assert_eq!(
        s.matches("make_null(__ctx)").count(),
        1,
        "the `null`-typed source must be evaluated exactly once; got: {s}"
    );
    assert!(
        s.contains(
            "let_=make_null(__ctx);::core::option::Option::Some(::core::option::Option::None)"
        ),
        "a `null`-typed call for a `number | null | undefined` parameter must run the source \
         before yielding the replacement `Some(None)`; got: {s}"
    );
}

#[test]
fn typed_null_call_into_an_optional_string_evaluates_the_source_once() {
    let mut types = TypeTable::new();
    let s = typed_null_call_emission(&mut types, &Type::String);
    assert_eq!(
        s.matches("make_null(__ctx)").count(),
        1,
        "the `null`-typed source must be evaluated exactly once; got: {s}"
    );
    assert!(
        s.contains("let_=make_null(__ctx);::core::option::Option::None"),
        "a `null`-typed call for a `string | null` parameter must run the source before yielding \
         `None`; got: {s}"
    );
}

#[test]
fn typed_null_call_into_a_flat_optional_evaluates_the_source_once() {
    let mut types = TypeTable::new();
    let s = typed_null_call_emission(&mut types, &Type::F64);
    assert_eq!(
        s.matches("make_null(__ctx)").count(),
        1,
        "the `null`-typed source must be evaluated exactly once; got: {s}"
    );
    assert!(
        s.contains("let_=make_null(__ctx);::core::option::Option::None"),
        "a `null`-typed call for a `number | null` parameter must run the source before yielding \
         `None`; got: {s}"
    );
}

fn numeric_add_emission(left: &Type, right: &Type, result: &Type) -> String {
    let mut types = TypeTable::new();
    let left_ty = types.intern(left);
    let right_ty = types.intern(right);
    let result_ty = types.intern(result);
    let mut f = empty_func("add");
    f.ret = result_ty;
    f.params = vec![
        MirParam {
            id: LocalId::from_raw(0),
            name: Atom::from("a"),
            ty: left_ty,
        },
        MirParam {
            id: LocalId::from_raw(1),
            name: Atom::from("b"),
            ty: right_ty,
        },
    ];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Binary {
                op: BinaryOp::Add,
                left: Box::new(MirExpr::Local(LocalId::from_raw(0))),
                right: Box::new(MirExpr::Local(LocalId::from_raw(1))),
                ty: result_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("mixed numeric operand widths must emit");
    norm(&tokens.to_string())
}

#[test]
fn integer_operands_of_different_widths_share_the_wider_type() {
    let s = numeric_add_emission(&Type::I32, &Type::I64, &Type::I64);
    assert!(
        s.contains("((a)asi64)+b"),
        "an `i32` operand combined with an `i64` operand must be widened to `i64` before `+`, so \
         the generated Rust does not add two different integer types; got: {s}"
    );
}

#[test]
fn float_operands_of_different_widths_share_the_wider_type() {
    let s = numeric_add_emission(&Type::F32, &Type::F64, &Type::F64);
    assert!(
        s.contains("((a)asf64)+b"),
        "an `f32` operand combined with an `f64` operand must be widened to `f64` before `+`, so \
         the generated Rust does not add two different float types; got: {s}"
    );
}

#[test]
fn mixed_signedness_operands_share_the_common_f64_type() {
    let s = numeric_add_emission(&Type::I32, &Type::U32, &Type::F64);
    assert!(
        s.contains("((a)asf64)+((b)asf64)"),
        "an `i32` operand combined with a `u32` operand must widen both to the common `f64` type, \
         because neither integer type can represent the other's range; got: {s}"
    );
}

#[test]
fn array_from_mapped_wraps_the_runtime_index_for_an_optional_mapper_parameter() {
    let mut types = TypeTable::new();
    let f64_ty = types.intern(&Type::F64);
    let opt_f64_ty = types.intern(&Type::Optional {
        inner: f64_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let arr_ty = types.intern(&Type::Array { element: f64_ty });

    let mut opt_idx = empty_func("optIdx");
    opt_idx.id = FunctionId::from_raw(1);
    opt_idx.params = vec![
        MirParam {
            id: LocalId::from_raw(0),
            name: Atom::from("x"),
            ty: f64_ty,
        },
        MirParam {
            id: LocalId::from_raw(1),
            name: Atom::from("i"),
            ty: opt_f64_ty,
        },
    ];
    opt_idx.ret = f64_ty;
    opt_idx.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(LocalId::from_raw(0))))],
        },
    };

    let mut mapped = empty_func("mapped");
    mapped.id = FunctionId::from_raw(2);
    mapped.params = vec![MirParam {
        id: LocalId::from_raw(0),
        name: Atom::from("a"),
        ty: arr_ty,
    }];
    mapped.ret = arr_ty;
    mapped.body = MirBody {
        locals: vec![MirLocalDecl {
            id: LocalId::from_raw(1),
            name: Atom::from("b"),
            ty: arr_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ArrayFromMapped,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(0)),
                    MirExpr::Global(Atom::from("optIdx")),
                ],
                dest: Some(LocalId::from_raw(1)),
                ty: arr_ty,
                target_ty: None,
            }],
        },
    };

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(opt_idx));
    prog.push_decl(MirDecl::Function(mapped));
    let s = emit_decls(&prog, &types)
        .expect("Array.from with an optional index parameter must emit")
        .to_string();
    let compact = norm(&s);
    assert!(
        compact.contains("::core::option::Option::Some((__ts_aot_idx)asf64)"),
        "the runtime index is typed `i64` while the mapper's optional index parameter is \
         `Option<f64>`, so the index argument must be wrapped in `Some(..)` with the payload cast \
         to the parameter's inner type; got: {s}"
    );
}
