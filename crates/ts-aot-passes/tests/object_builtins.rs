use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt, ObjectLiteralField,
};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};
use ts_aot_passes::{PassContext, convert_program};

struct Setup {
    types: TypeTable,
    dynamic_ty: TypeId,
}

impl Setup {
    fn new() -> Self {
        let mut types = TypeTable::new();
        let dynamic_ty = types.intern(&Type::Dynamic);
        Self { types, dynamic_ty }
    }
}

fn object_builtin_call_fn(setup: &mut Setup, method: &str, arg_count: usize) -> Vec<MirStmt> {
    let Setup { types, dynamic_ty } = setup;
    let fn_name = Atom::new_inline("f");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let mut args = Vec::with_capacity(arg_count);
    for i in 0..arg_count {
        args.push(HirExpr::Local {
            id: LocalId::from_raw(i as u32),
            ty: *dynamic_ty,
        });
    }
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Object"),
                    ty: *dynamic_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline(method),
                ty: *dynamic_ty,
            })),
            args,
            ty: *dynamic_ty,
        }),
    }];
    let mut params = Vec::with_capacity(arg_count);
    for i in 0..arg_count {
        params.push(HirParam {
            name: Atom::new_inline(if i == 0 { "x" } else { "y" }),
            ty: *dynamic_ty,
        });
    }
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params,
        ret: *dynamic_ty,
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut ctx = PassContext::default();
    let mir = convert_program(&hir, types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    f.body.block.stmts.clone()
}

fn find_runtime_op(stmts: &[MirStmt], expected: RuntimeOp) -> Option<MirStmt> {
    stmts.iter().find_map(|s| match s {
        MirStmt::Runtime { op, .. } if *op == expected => Some(s.clone()),
        _ => None,
    })
}

#[test]
fn object_get_prototype_of_emits_proto_get_runtime_call() {
    let mut setup = Setup::new();
    let stmts = object_builtin_call_fn(&mut setup, "getPrototypeOf", 1);
    let stmt = find_runtime_op(&stmts, RuntimeOp::OpObjectProtoGet)
        .expect("Object.getPrototypeOf must emit MirStmt::Runtime with OpObjectProtoGet");
    let MirStmt::Runtime { args, dest, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(
        args.len(),
        1,
        "OpObjectProtoGet takes (obj) — no field name"
    );
    assert!(dest.is_some(), "OpObjectProtoGet must produce a dest local");
}

#[test]
fn object_get_prototype_of_emit_uses_proto_get_helper() {
    let mut setup = Setup::new();
    let stmts = object_builtin_call_fn(&mut setup, "getPrototypeOf", 1);
    assert!(!stmts.is_empty(), "expected at least one stmt");
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("Object"),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("getPrototypeOf"),
                    ty: dynamic_ty,
                })),
                args: vec![HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_object_proto_get"),
        "emitted Rust must call __ts_aot_object_proto_get for Object.getPrototypeOf, got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_object_keys"),
        "Object.getPrototypeOf must NOT emit keys helper, got: {s}"
    );
}

#[test]
fn object_set_prototype_of_emits_set_prototype_of_runtime_call() {
    let mut setup = Setup::new();
    let stmts = object_builtin_call_fn(&mut setup, "setPrototypeOf", 2);
    let stmt = find_runtime_op(&stmts, RuntimeOp::OpObjectSetPrototypeOf).expect(
        "Object.setPrototypeOf must emit OpObjectSetPrototypeOf (strict — throws on invalid proto)",
    );
    let has_lenient_proto_set = stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoSet,
                ..
            }
        )
    });
    assert!(
        !has_lenient_proto_set,
        "Object.setPrototypeOf must NOT route through the lenient OpObjectProtoSet (which is for obj.__proto__ = x assignment), got stmts: {stmts:?}"
    );
    let MirStmt::Runtime { args, dest, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(args.len(), 2, "OpObjectSetPrototypeOf takes (obj, proto)");
    assert!(
        dest.is_some(),
        "OpObjectSetPrototypeOf returns the obj per JS spec, so dest must be Some"
    );
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: vec![
            HirParam {
                name: Atom::new_inline("x"),
                ty: dynamic_ty,
            },
            HirParam {
                name: Atom::new_inline("y"),
                ty: dynamic_ty,
            },
        ],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("Object"),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("setPrototypeOf"),
                    ty: dynamic_ty,
                })),
                args: vec![
                    HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: dynamic_ty,
                    },
                    HirExpr::Local {
                        id: LocalId::from_raw(1),
                        ty: dynamic_ty,
                    },
                ],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_object_set_prototype_of"),
        "emitted Rust must call __ts_aot_object_set_prototype_of (strict helper for Object.setPrototypeOf), got: {s}"
    );
    assert!(
        !s.contains("__ts_aot_object_proto_set("),
        "Object.setPrototypeOf must NOT call the lenient __ts_aot_object_proto_set, got: {s}"
    );
}

#[test]
fn object_keys_emits_keys_runtime_call() {
    let mut setup = Setup::new();
    let stmts = object_builtin_call_fn(&mut setup, "keys", 1);
    let stmt = find_runtime_op(&stmts, RuntimeOp::OpObjectKeys)
        .expect("Object.keys must emit MirStmt::Runtime with OpObjectKeys");
    let MirStmt::Runtime { args, dest, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(args.len(), 1, "OpObjectKeys takes (obj)");
    assert!(dest.is_some(), "OpObjectKeys must produce a dest local");
}

#[test]
fn object_keys_emit_uses_keys_helper() {
    let mut setup = Setup::new();
    let _ = object_builtin_call_fn(&mut setup, "keys", 1);
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("Object"),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("keys"),
                    ty: dynamic_ty,
                })),
                args: vec![HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_object_keys"),
        "emitted Rust must call __ts_aot_object_keys for Object.keys, got: {s}"
    );
}

#[test]
fn object_unknown_builtin_call_falls_through_to_indirect_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("Object"),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("assign"),
                    ty: dynamic_ty,
                })),
                args: vec![HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let has_known_proto_runtime = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoGet
                    | RuntimeOp::OpObjectProtoSet
                    | RuntimeOp::OpObjectSetPrototypeOf
                    | RuntimeOp::OpObjectKeys,
                ..
            }
        )
    });
    assert!(
        !has_known_proto_runtime,
        "unknown Object.assign must not match any of the three builtins, got stmts: {:?}",
        f.body.block.stmts
    );
    let has_indirect = f
        .body
        .block
        .stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::IndirectCall { .. }))));
    assert!(
        has_indirect,
        "unknown Object.assign must lower to MirExpr::IndirectCall (Object.assign is a generic function), got stmts: {:?}",
        f.body.block.stmts
    );
}

fn builtin_wrong_arg_count_call(
    setup: &mut Setup,
    method: &str,
    arg_count: usize,
) -> (Vec<MirStmt>, Vec<String>) {
    let Setup { types, dynamic_ty } = setup;
    let fn_name = Atom::new_inline("f");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let mut args = Vec::with_capacity(arg_count);
    for i in 0..arg_count {
        args.push(HirExpr::Local {
            id: LocalId::from_raw(i as u32),
            ty: *dynamic_ty,
        });
    }
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Object"),
                    ty: *dynamic_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline(method),
                ty: *dynamic_ty,
            })),
            args,
            ty: *dynamic_ty,
        }),
    }];
    let mut params = Vec::with_capacity(arg_count);
    for i in 0..arg_count {
        params.push(HirParam {
            name: Atom::new_inline(if i == 0 { "x" } else { "y" }),
            ty: *dynamic_ty,
        });
    }
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params,
        ret: *dynamic_ty,
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut ctx = PassContext::default();
    let mir = convert_program(&hir, types, &mut ctx);
    let diagnostic_codes = ctx
        .take_diagnostics()
        .into_iter()
        .map(|d| d.code.as_str().to_owned())
        .collect();
    let f = mir.functions().next().expect("one function");
    (f.body.block.stmts.clone(), diagnostic_codes)
}

#[test]
fn object_get_prototype_of_with_two_args_emits_unit_and_error() {
    let mut setup = Setup::new();
    let (stmts, diag_codes) = builtin_wrong_arg_count_call(&mut setup, "getPrototypeOf", 2);
    let has_proto_get = stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoGet,
                ..
            }
        )
    });
    assert!(
        !has_proto_get,
        "Object.getPrototypeOf(a, b) must NOT emit OpObjectProtoGet (would call runtime with extra arg, silently ignored or type-error at emit), got stmts: {stmts:?}"
    );
    let returns_unit = stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::Unit))));
    assert!(
        returns_unit,
        "Object.getPrototypeOf(a, b) must return MirExpr::Unit after P0005 error, got stmts: {stmts:?}"
    );
    assert!(
        diag_codes.iter().any(|c| c == "P0005"),
        "Object.getPrototypeOf(a, b) must emit P0005 diagnostic, got codes: {diag_codes:?}"
    );
}

#[test]
fn object_set_prototype_of_with_one_arg_emits_unit_and_error() {
    let mut setup = Setup::new();
    let (stmts, diag_codes) = builtin_wrong_arg_count_call(&mut setup, "setPrototypeOf", 1);
    let has_set_proto = stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSetPrototypeOf,
                ..
            }
        )
    });
    assert!(
        !has_set_proto,
        "Object.setPrototypeOf(a) must NOT emit OpObjectSetPrototypeOf (backend emit would call args.get(1) = None → uncompilable Rust), got stmts: {stmts:?}"
    );
    let returns_unit = stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::Unit))));
    assert!(
        returns_unit,
        "Object.setPrototypeOf(a) must return MirExpr::Unit after P0005 error, got stmts: {stmts:?}"
    );
    assert!(
        diag_codes.iter().any(|c| c == "P0005"),
        "Object.setPrototypeOf(a) must emit P0005 diagnostic, got codes: {diag_codes:?}"
    );
}

#[test]
fn object_keys_with_two_args_emits_unit_and_error() {
    let mut setup = Setup::new();
    let (stmts, diag_codes) = builtin_wrong_arg_count_call(&mut setup, "keys", 2);
    let has_keys = stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectKeys,
                ..
            }
        )
    });
    assert!(
        !has_keys,
        "Object.keys(a, b) must NOT emit OpObjectKeys, got stmts: {stmts:?}"
    );
    let returns_unit = stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::Unit))));
    assert!(
        returns_unit,
        "Object.keys(a, b) must return MirExpr::Unit after P0005 error, got stmts: {stmts:?}"
    );
    assert!(
        diag_codes.iter().any(|c| c == "P0005"),
        "Object.keys(a, b) must emit P0005 diagnostic, got codes: {diag_codes:?}"
    );
}

fn proto_get_fn() -> Vec<MirStmt> {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getProto"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("__proto__"),
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    f.body.block.stmts.clone()
}

#[test]
fn proto_get_on_dynamic_owner_emits_proto_get_runtime_call() {
    let stmts = proto_get_fn();
    let stmt = find_runtime_op(&stmts, RuntimeOp::OpObjectProtoGet)
        .expect("dynamic __proto__ get must emit OpObjectProtoGet, not OpObjectGet");
    let MirStmt::Runtime { args, dest, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(
        args.len(),
        1,
        "OpObjectProtoGet takes (obj) — not (obj, name)"
    );
    assert!(dest.is_some(), "OpObjectProtoGet must produce a dest local");
    let has_object_get = stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        !has_object_get,
        "__proto__ must NOT route through OpObjectGet (which looks at fields, not proto), got stmts: {stmts:?}"
    );
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getProto"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("__proto__"),
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_object_proto_get"),
        "emitted Rust must call __ts_aot_object_proto_get for __proto__ access, got: {s}"
    );
}

#[test]
fn proto_set_on_dynamic_owner_emits_proto_set_runtime_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("setProto"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("__proto__"),
                    ty: dynamic_ty,
                }),
                value: Box::new(HirExpr::Int(42)),
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let stmt = find_runtime_op(&f.body.block.stmts, RuntimeOp::OpObjectProtoSet)
        .expect("dynamic __proto__ = value must emit OpObjectProtoSet, not OpObjectSet");
    let MirStmt::Runtime { args, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(args.len(), 2, "OpObjectProtoSet takes (obj, proto)");
    let has_object_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert!(
        !has_object_set,
        "__proto__ = x must NOT route through OpObjectSet (which writes to fields, not proto), got stmts: {:?}",
        f.body.block.stmts
    );
    let returns_rhs = f
        .body
        .block
        .stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::Local(_)))));
    assert!(
        returns_rhs,
        "obj.__proto__ = 42 must evaluate to the assigned RHS (42), not the obj — JS spec: assignment expression value = RHS"
    );
    let value_temp_local = f
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::Return(Some(MirExpr::Local(id))) => Some(*id),
            _ => None,
        })
        .expect("return value must be a Local");
    let value_init_is_rhs = f.body.block.stmts.iter().any(|s| match s {
        MirStmt::Let {
            local,
            init: Some(MirExpr::DynamicFrom { value, .. }),
            ..
        } if *local == value_temp_local => {
            matches!(value.as_ref(), MirExpr::Int { value: 42, .. })
        }
        _ => false,
    });
    assert!(
        value_init_is_rhs,
        "the local returned by the __proto__ assignment must be bound to DynamicFrom(Int(42)), \
         so `obj.__proto__ = 42` evaluates to 42 per JS assignment semantics (not the obj)"
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_object_proto_set"),
        "emitted Rust must call __ts_aot_object_proto_set for __proto__ assignment, got: {s}"
    );
}

#[test]
fn regular_field_name_on_dynamic_owner_still_uses_object_get() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: dynamic_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("foo"),
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let stmt = find_runtime_op(&f.body.block.stmts, RuntimeOp::OpObjectGet)
        .expect("regular field access on dynamic must still emit OpObjectGet");
    let MirStmt::Runtime { args, .. } = stmt else {
        panic!("expected Runtime stmt");
    };
    assert_eq!(args.len(), 2, "OpObjectGet takes (obj, name)");
}

#[test]
fn proto_field_name_on_non_dynamic_owner_skips_proto_runtime_path() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty = Type::Named {
        symbol: Atom::new_inline("MyClass"),
    };
    let param_ty_id = types.intern(&param_ty);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getProto"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: param_ty_id,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: param_ty_id,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("__proto__"),
                ty: param_ty_id,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let has_proto_runtime = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoGet | RuntimeOp::OpObjectProtoSet,
                ..
            }
        )
    });
    assert!(
        !has_proto_runtime,
        "static-typed __proto__ must NOT route through OpObjectProtoGet/Set — that path is only for dynamic owners. \
         Static __proto__ is a regular field per JS spec on non-dynamic types, got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn proto_compound_update_on_dynamic_owner_uses_proto_runtime_ops() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("bumpProto"),
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: dynamic_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("__proto__"),
                    ty: dynamic_ty,
                }),
                op: ts_aot_ir_hir::HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
                ty: dynamic_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let has_proto_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoGet,
                ..
            }
        )
    });
    let has_proto_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectProtoSet,
                ..
            }
        )
    });
    let has_plain_object_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    let has_plain_object_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert!(
        has_proto_get,
        "x.__proto__ += y must emit OpObjectProtoGet (proto is not a field), got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        has_proto_set,
        "x.__proto__ += y must emit OpObjectProtoSet (proto is not a field), got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        !has_plain_object_get,
        "x.__proto__ += y must NOT route through OpObjectGet, got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        !has_plain_object_set,
        "x.__proto__ += y must NOT route through OpObjectSet, got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn object_literal_arg_to_object_keys_emits_object_keys_runtime_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let arg = HirExpr::ObjectLiteral {
        fields: vec![ObjectLiteralField::Property {
            name: Atom::new_inline("a"),
            value: HirExpr::Int(1),
        }],
        ty: dynamic_ty,
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: Vec::new(),
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Global {
                        name: Atom::new_inline("Object"),
                        ty: dynamic_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("keys"),
                    ty: dynamic_ty,
                })),
                args: vec![arg],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let has_object_keys = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectKeys,
                ..
            }
        )
    });
    assert!(
        has_object_keys,
        "Object.keys({{a:1}}) on a fresh object literal must emit OpObjectKeys (object literals are dynamic, so the inlined path applies), got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn empty_object_literal_emits_object_new_runtime_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let arg = HirExpr::ObjectLiteral {
        fields: Vec::new(),
        ty: dynamic_ty,
    };
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: Vec::new(),
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return { value: Some(arg) }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    let has_object_new = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectNew,
                ..
            }
        )
    });
    assert!(
        has_object_new,
        "empty `{{}}` must emit OpObjectNew (not DynamicFrom(Unit) which would yield undefined), got stmts: {:?}",
        f.body.block.stmts
    );
    let has_object_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert!(
        !has_object_set,
        "empty `{{}}` must NOT emit OpObjectSet (no fields), got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn object_literal_with_field_emits_mutable_dest_for_subsequent_set() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: Atom::new_inline("f"),
        params: Vec::new(),
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::ObjectLiteral {
                fields: vec![ObjectLiteralField::Property {
                    name: Atom::new_inline("x"),
                    value: HirExpr::Int(1),
                }],
                ty: dynamic_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mir = convert_program(&hir, &mut types, &mut ctx);
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("let mut "),
        "OpObjectNew dest must be `let mut` (not `let`) so subsequent `&mut <local>` in OpObjectSet compiles, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_object_new ()"),
        "OpObjectNew must call __ts_aot_object_new, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_set (& mut "),
        "OpObjectSet on the dest must take `&mut <local>` (matches the mutability of the let), got: {s}"
    );
}
