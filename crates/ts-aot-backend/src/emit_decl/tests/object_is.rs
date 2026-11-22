use super::super::emit_decls;
use super::{emit_object_is, empty_func, norm};
use ts_aot_core::{
    Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirProgram, MirStmt,
    MirStructDecl, RuntimeOp,
};

#[test]
fn object_is_same_nullish_literal_emits_true() {
    for (a, b, label) in [
        (MirExpr::Unit, MirExpr::Unit, "undefined"),
        (
            MirExpr::Null {
                ty: TypeId::from_raw(0),
            },
            MirExpr::Null {
                ty: TypeId::from_raw(0),
            },
            "null",
        ),
    ] {
        let s = emit_object_is(vec![a, b]);
        assert!(
            !s.contains("__ts_aot_object_is"),
            "Object.is({label}, {label}) must not call the runtime helper, got: {s}"
        );
        assert!(
            s.contains("true") && !s.contains("false"),
            "Object.is({label}, {label}) must lower to `true` and never `false`, got: {s}"
        );
    }
}

#[test]
fn object_is_null_versus_undefined_emits_false() {
    let s = emit_object_is(vec![
        MirExpr::Null {
            ty: TypeId::from_raw(0),
        },
        MirExpr::Unit,
    ]);
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is(null, undefined) must not call the runtime helper, got: {s}"
    );
    assert!(
        s.contains("false") && !s.contains("true"),
        "Object.is(null, undefined) must lower to `false` and never `true`, got: {s}"
    );
}

fn emit_struct_local_object_is(field_count: u32, lhs: u32, rhs: u32) -> String {
    let mut types = TypeTable::new();
    let struct_id = StructId::from_raw(7);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(9);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::from("p"),
                ty: struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: LocalId::from_raw(1),
                name: Atom::from("q"),
                ty: struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(lhs)),
                    MirExpr::Local(LocalId::from_raw(rhs)),
                ],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let fields = (0..field_count)
        .map(|index| MirFieldDecl {
            id: FieldId::from_raw(index),
            name: Atom::from(format!("f{index}")),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        })
        .collect();
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields,
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    emit_decls(&prog, &types)
        .expect("struct object-is emission should succeed")
        .to_string()
}

fn emit_optional_struct_local_object_is(field_count: u32, lhs: u32, rhs: u32) -> String {
    let mut types = TypeTable::new();
    let struct_id = StructId::from_raw(7);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_struct_ty = types.intern(&Type::Optional {
        inner: struct_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(9);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::from("p"),
                ty: opt_struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: LocalId::from_raw(1),
                name: Atom::from("q"),
                ty: opt_struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(lhs)),
                    MirExpr::Local(LocalId::from_raw(rhs)),
                ],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let fields = (0..field_count)
        .map(|index| MirFieldDecl {
            id: FieldId::from_raw(index),
            name: Atom::from(format!("f{index}")),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        })
        .collect();
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields,
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    emit_decls(&prog, &types)
        .expect("optional struct object-is emission should succeed")
        .to_string()
}

#[test]
fn object_is_struct_locals_compare_place_identity() {
    for (lhs, rhs, expected, label) in [
        (0u32, 0u32, "true", "the same local"),
        (0u32, 1u32, "false", "two distinct locals"),
    ] {
        let mut types = TypeTable::new();
        let struct_ty = types.intern(&Type::Named {
            symbol: Atom::from("Point"),
        });
        let bool_ty = types.intern(&Type::Bool);
        let temp = LocalId::from_raw(9);
        let mut f = empty_func("is");
        f.ret = bool_ty;
        f.body = MirBody {
            locals: vec![
                MirLocalDecl {
                    id: LocalId::from_raw(0),
                    name: Atom::from("p"),
                    ty: struct_ty,
                    mutable: false,
                    narrowed: false,
                },
                MirLocalDecl {
                    id: LocalId::from_raw(1),
                    name: Atom::from("q"),
                    ty: struct_ty,
                    mutable: false,
                    narrowed: false,
                },
                MirLocalDecl {
                    id: temp,
                    name: Atom::from(""),
                    ty: bool_ty,
                    mutable: false,
                    narrowed: false,
                },
            ],
            block: MirBlock {
                stmts: vec![MirStmt::Runtime {
                    op: RuntimeOp::ObjectIs,
                    args: vec![
                        MirExpr::Local(LocalId::from_raw(lhs)),
                        MirExpr::Local(LocalId::from_raw(rhs)),
                    ],
                    dest: Some(temp),
                    ty: bool_ty,
                    target_ty: None,
                }],
            },
        };
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Function(f));
        let s = emit_decls(&prog, &types)
            .expect("struct object-is emission should succeed")
            .to_string();
        assert!(
            !s.contains("__ts_aot_object_is"),
            "Object.is on structs must not use the runtime helper, got: {s}"
        );
        assert!(
            s.contains(expected),
            "Object.is on a named struct with no declaration must keep the compile-time \
             place-identity answer ({label} -> {expected}), got: {s}"
        );
    }
}

#[test]
fn object_is_aliased_struct_locals_compare_shared_field_identity() {
    let s = emit_struct_local_object_is(1, 0, 1);
    let compact = norm(&s);
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is on declared struct locals must not use the runtime helper, got: {s}"
    );
    assert!(
        compact.contains("let__ts_aot_a=&p;") && compact.contains("let__ts_aot_b=&q;"),
        "each operand must be evaluated exactly once into its own binding, got: {s}"
    );
    assert!(
        compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_a).f0)")
            && compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_b).f0)"),
        "two distinct locals of a declared struct must be compared by the shared field cell of \
         their object, so `const q = p` answers `true` at runtime, got: {s}"
    );
}

#[test]
fn object_is_struct_without_fields_keeps_compile_time_answer() {
    for (lhs, rhs, expected, label) in [
        (0u32, 0u32, "true", "the same local"),
        (0u32, 1u32, "false", "two distinct locals"),
    ] {
        let s = emit_struct_local_object_is(0, lhs, rhs);
        assert!(
            !s.contains("Rc :: as_ptr") && !s.contains("Rc::as_ptr"),
            "a struct with no field carries no identity probe, got: {s}"
        );
        assert!(
            s.contains(expected),
            "distinct locals of a fieldless struct cannot be distinguished, so Object.is must \
             keep the compile-time answer ({label} -> {expected}), got: {s}"
        );
    }
}

#[test]
fn object_is_optional_struct_locals_compare_shared_field_identity() {
    let s = emit_optional_struct_local_object_is(1, 0, 1);
    let compact = norm(&s);
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is on optional struct locals must not use the runtime helper, got: {s}"
    );
    assert!(
        compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_left_0).f0)")
            && compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_right_0).f0)"),
        "two present optional locals of a declared struct must be compared by the shared field \
         cell of their object, so an alias answers `true` at runtime, got: {s}"
    );
    assert!(
        compact.contains("(None,None)=>true"),
        "two absent optional locals must compare equal without a field probe, got: {s}"
    );
    assert!(
        compact.contains("_=>false"),
        "exactly one absent optional local must compare unequal, got: {s}"
    );
}

#[test]
fn object_is_optional_struct_versus_plain_struct_compares_shared_field_identity() {
    let mut types = TypeTable::new();
    let struct_id = StructId::from_raw(7);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_struct_ty = types.intern(&Type::Optional {
        inner: struct_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(9);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::from("p"),
                ty: opt_struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: LocalId::from_raw(1),
                name: Atom::from("q"),
                ty: struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(0)),
                    MirExpr::Local(LocalId::from_raw(1)),
                ],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("f0"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("mixed optional/plain struct object-is emission should succeed")
        .to_string();
    let compact = norm(&s);
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is on struct locals must not use the runtime helper, got: {s}"
    );
    assert!(
        compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_left_0).f0)")
            && compact.contains("::std::rc::Rc::as_ptr(&(__ts_aot_b).f0)"),
        "a present optional local and a plain local of the same struct hold one object, so they \
         must be compared by the shared field cell of that object, got: {s}"
    );
    assert!(
        compact.contains("None=>false"),
        "an absent optional local is never the same object as a plain struct value, got: {s}"
    );
}

#[test]
fn object_is_optional_struct_local_versus_null_uses_the_presence_of_the_value() {
    let mut types = TypeTable::new();
    let struct_id = StructId::from_raw(7);
    let struct_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_struct_ty = types.intern(&Type::Optional {
        inner: struct_ty,
        nullish: ts_aot_core::NullishKinds {
            null: true,
            undefined: false,
        },
    });
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(9);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::from("p"),
                ty: opt_struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(0)),
                    MirExpr::Null { ty: opt_struct_ty },
                ],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("f0"),
            ty: i64_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("optional struct versus null object-is emission should succeed")
        .to_string();
    let compact = norm(&s);
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is on an optional struct local must not use the runtime helper, got: {s}"
    );
    assert!(
        compact.contains("None=>true") && compact.contains("Some(__ts_aot_left_0)=>false"),
        "a `T | null` local that holds `null` is the same value as the `null` literal, and a \
         present value never is, got: {s}"
    );
}

#[test]
fn object_is_struct_field_place_identity() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: i64_ty });
    let bool_ty = types.intern(&Type::Bool);
    let owner = StructId::from_raw(7);
    let struct_ty = types.intern(&Type::Struct { id: owner });
    let temp = LocalId::from_raw(9);
    let base = LocalId::from_raw(0);
    let field = FieldId::from_raw(0);
    let field_expr = || MirExpr::Field {
        base: Box::new(MirExpr::Local(base)),
        field,
        ty: arr_ty,
    };
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: base,
                name: Atom::from("p"),
                ty: struct_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![field_expr(), field_expr()],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: owner,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: field,
            name: Atom::from("items"),
            ty: arr_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("field object-is emission should succeed")
        .to_string();
    assert!(
        !s.contains("__ts_aot_object_is"),
        "Object.is(p.f, p.f) must not use the runtime helper, got: {s}"
    );
    assert!(
        s.contains("true") && !s.contains("false"),
        "Object.is(p.f, p.f) on the same field place must lower to `true`, got: {s}"
    );
}

#[test]
fn object_is_symbol_uses_runtime_helper() {
    let mut types = TypeTable::new();
    let symbol_ty = types.intern(&Type::Symbol);
    let bool_ty = types.intern(&Type::Bool);
    let temp = LocalId::from_raw(9);
    let mut f = empty_func("is");
    f.ret = bool_ty;
    f.body = MirBody {
        locals: vec![
            MirLocalDecl {
                id: LocalId::from_raw(0),
                name: Atom::from("s"),
                ty: symbol_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: LocalId::from_raw(1),
                name: Atom::from("t"),
                ty: symbol_ty,
                mutable: false,
                narrowed: false,
            },
            MirLocalDecl {
                id: temp,
                name: Atom::from(""),
                ty: bool_ty,
                mutable: false,
                narrowed: false,
            },
        ],
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: vec![
                    MirExpr::Local(LocalId::from_raw(0)),
                    MirExpr::Local(LocalId::from_raw(1)),
                ],
                dest: Some(temp),
                ty: bool_ty,
                target_ty: None,
            }],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let s = emit_decls(&prog, &types)
        .expect("symbol object-is emission should succeed")
        .to_string();
    assert!(
        s.contains("__ts_aot_object_is"),
        "Object.is on symbols must use the runtime SameValue helper (symbols are i64 ids), got: {s}"
    );
}
