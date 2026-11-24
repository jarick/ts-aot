use super::super::{emit_decls, emit_function};
use super::{empty_func, signature_fragment};
use ts_aot_core::{
    Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_mir::{
    FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirParam, MirPlace,
    MirPlaceBase, MirProgram, MirStmt, MirStructDecl, RuntimeOp,
};

#[test]
fn reassigned_param_emits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(1);
    let mut f = empty_func("reset");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("p"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Local { id: param },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "reset");
    assert!(
        sig.contains("mut p : i32"),
        "reassigned param must emit `mut p : i32` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn generator_next_consumes_param_emits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let g_param = LocalId::from_raw(1);
    let temp = LocalId::from_raw(2);

    let mut f = empty_func("drive");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.params = vec![MirParam {
        id: g_param,
        name: Atom::from("g"),
        ty: gen_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Local(g_param)],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive");
    assert!(
        sig.contains("mut g : ts_aot_runtime :: Generator < i64 >"),
        "generator param consumed by RuntimeOp::GeneratorNext must emit `mut g : ts_aot_runtime :: Generator < i64 >` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn indirect_generator_next_consumes_param_emits_mut() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator {
        inner: i64_ty,
        is_async: false,
    });
    let gen_result_ty = types.intern(&Type::GeneratorResult { inner: i64_ty });
    let struct_id = StructId::from_raw(0);
    let box_ty = types.intern(&Type::Struct { id: struct_id });
    let g_field = FieldId::from_raw(0);
    let obj_param = LocalId::from_raw(1);
    let temp = LocalId::from_raw(2);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: g_field,
            name: Atom::from("g"),
            ty: gen_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("drive_indirect");
    f.ret = gen_ty;
    f.kind = FunctionKind::Generator;
    f.params = vec![MirParam {
        id: obj_param,
        name: Atom::from("obj"),
        ty: box_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Runtime {
                op: RuntimeOp::GeneratorNext,
                args: vec![MirExpr::Field {
                    base: Box::new(MirExpr::Local(obj_param)),
                    field: g_field,
                    ty: gen_ty,
                }],
                dest: Some(temp),
                ty: gen_result_ty,
                target_ty: None,
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "drive_indirect");
    assert!(
        sig.contains("mut obj"),
        "param consumed by indirect RuntimeOp::GeneratorNext (obj.g.next()) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
    assert!(
        s.contains("obj . g"),
        "field access on a struct param must resolve to the declared field name `g` via EmitCtx, \
         not the placeholder `__field0` that standalone EmitCtx would emit. Pushing the struct \
         decl into the same `prog` and calling `emit_decls` is what wires the field name into the \
         ctx. Got: `{s}`"
    );
}

#[test]
fn param_used_in_nested_place_assign_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let param = LocalId::from_raw(1);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("poke");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("obj"),
        ty: obj_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Local(param)),
                    field: FieldId::from_raw(0),
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke");
    assert!(
        sig.contains("mut obj"),
        "param used in a nested place assign (obj.x = 0) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn param_used_in_optional_chain_place_assign_emits_mut() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let struct_id = StructId::from_raw(0);
    let obj_ty = types.intern(&Type::Struct { id: struct_id });
    let opt_ty = types.intern(&Type::Optional {
        inner: obj_ty,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let param = LocalId::from_raw(1);

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: struct_id,
        name: Atom::from("Box"),
        fields: vec![MirFieldDecl {
            id: FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: i32_ty,
            mutable: true,
            visibility: Visibility::Public,
        }],
        methods: Vec::new(),
    }));

    let mut f = empty_func("poke_opt");
    f.ret = TypeId::from_raw(0);
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("obj"),
        ty: obj_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Assign {
                target: MirPlace::Field {
                    base: Box::new(MirPlaceBase::Chain {
                        base: Box::new(MirExpr::OptionalChain {
                            base: Box::new(MirExpr::Local(param)),
                            ty: opt_ty,
                        }),
                        ty: opt_ty,
                    }),
                    field: FieldId::from_raw(0),
                    ty: i32_ty,
                },
                value: MirExpr::Int {
                    value: 0,
                    ty: i32_ty,
                },
            }],
        },
    };
    prog.push_decl(MirDecl::Function(f));

    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "poke_opt");
    assert!(
        sig.contains("mut obj"),
        "param used in a nested place assign through an optional chain (MirPlaceBase::Chain whose base is MirExpr::OptionalChain) must emit `mut obj` in the function signature, got signature fragment: `{sig}`"
    );
}

#[test]
fn non_reassigned_param_omits_mut_in_signature() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(1);
    let mut f = empty_func("read");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("p"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(param)))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    let sig = signature_fragment(&s, "read");
    assert!(
        sig.contains("p : i32"),
        "non-reassigned param must emit `p : i32` in the function signature, got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains("mut p"),
        "non-reassigned param must NOT emit `mut`, got signature fragment: `{sig}`"
    );
}
