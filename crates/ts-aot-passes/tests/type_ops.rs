use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, ModuleId, Type, TypeTable};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirProgram, HirStmt, HirUnaryOp,
};
use ts_aot_ir_mir::{MirExpr, MirStmt};
use ts_aot_passes::{PassContext, convert_program};

#[test]
fn end_to_end_typeof_int_literal_emits_runtime_typeof_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let string_ty = types.intern(&Type::String);
    let fn_name = Atom::new_inline("getType");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: string_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: HirUnaryOp::TypeOf,
                expr: Box::new(HirExpr::Int(42)),
                ty: string_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on TypeOf, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::TypeOf { expr, ty, .. })) = &f.body.block.stmts[0] else {
        panic!(
            "TypeOf must lower to MirExpr::TypeOf, got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert_eq!(
        *ty, string_ty,
        "MirExpr::TypeOf.ty must be the string TypeId (typeof returns a string), got {ty:?}"
    );
    let MirExpr::Int { value, .. } = expr.as_ref() else {
        panic!("inner of TypeOf must be the original Int(42), got {expr:?}");
    };
    assert_eq!(*value, 42);

    let tokens = emit_decls(&mir, &types).expect("end-to-end typeof must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_typeof"),
        "typeof must emit a __ts_aot_typeof runtime call, got: {s}"
    );
    assert!(
        s.contains("String :: from"),
        "typeof with String return type must wrap __ts_aot_typeof result in String::from \
         (runtime helper returns &'static str, function returns String), got: {s}"
    );
}

#[test]
fn typeof_emit_compile_pattern() {
    fn __ts_aot_typeof_stub(_v: &i64) -> &'static str {
        "number"
    }
    let v: i64 = 42;
    let _s: String = String::from(__ts_aot_typeof_stub(&v));
}

#[test]
fn end_to_end_typeof_unit_emits_typeof_unit_helper() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let string_ty = types.intern(&Type::String);
    let fn_name = Atom::new_inline("getTypeUnit");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: string_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: HirUnaryOp::TypeOf,
                expr: Box::new(HirExpr::Undefined),
                ty: string_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on TypeOf Unit, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::TypeOf { expr, .. })) = &f.body.block.stmts[0] else {
        panic!(
            "TypeOf Undefined must lower to MirExpr::TypeOf, got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert!(
        matches!(expr.as_ref(), MirExpr::Unit),
        "typeof undefined must wrap MirExpr::Unit, got {expr:?}"
    );

    let tokens = emit_decls(&mir, &types).expect("end-to-end typeof undefined must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_typeof_unit"),
        "typeof undefined must emit __ts_aot_typeof_unit (NOT the generic __ts_aot_typeof which would \
         match () via TypeId and miss the undefined case), got: {s}"
    );
}

#[test]
fn end_to_end_typeof_null_emits_typeof_null_helper() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let string_ty = types.intern(&Type::String);
    let fn_name = Atom::new_inline("getTypeNull");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: string_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: HirUnaryOp::TypeOf,
                expr: Box::new(HirExpr::Null),
                ty: string_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on TypeOf Null, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::TypeOf { expr, .. })) = &f.body.block.stmts[0] else {
        panic!(
            "TypeOf Null must lower to MirExpr::TypeOf, got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert!(
        matches!(expr.as_ref(), MirExpr::Null { .. }),
        "typeof null must wrap MirExpr::Null, got {expr:?}"
    );

    let tokens = emit_decls(&mir, &types).expect("end-to-end typeof null must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_typeof_null"),
        "typeof null must emit __ts_aot_typeof_null (NOT the generic __ts_aot_typeof which would \
         match () via TypeId and return wrong 'undefined' for null), got: {s}"
    );
}

#[test]
fn end_to_end_void_call_drops_result_to_unit() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let fn_name = Atom::new_inline("voidCall");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Unary {
                op: HirUnaryOp::Void,
                expr: Box::new(HirExpr::Int(7)),
                ty: i64_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on Void, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Expr(MirExpr::Unit) = &f.body.block.stmts[0] else {
        panic!(
            "void <expr> must lower to MirStmt::Expr(MirExpr::Unit) at statement level, got {:?}",
            f.body.block.stmts[0]
        );
    };
}

#[test]
fn end_to_end_delete_non_property_returns_true() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("delLocal");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: HirUnaryOp::Delete,
                expr: Box::new(HirExpr::Int(7)),
                ty: bool_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on Delete, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let MirStmt::Return(Some(MirExpr::Bool(value))) = &f.body.block.stmts[0] else {
        panic!(
            "delete <non-property> must lower to MirExpr::Bool(true), got {:?}",
            f.body.block.stmts[0]
        );
    };
    assert!(
        *value,
        "delete <non-property> must return true (JS spec: no-op returns true)"
    );
    let _ = i64_ty;
}

#[test]
fn end_to_end_in_emits_runtime_call() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasIn");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::String(Atom::new_inline("foo"))),
                rhs: Box::new(HirExpr::Int(0)),
                ty: bool_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on In, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let has_runtime = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: ts_aot_ir_mir::RuntimeOp::OpIn,
                ..
            }
        )
    });
    assert!(
        has_runtime,
        "In must emit a Runtime stmt with OpIn, got stmts: {:?}",
        f.body.block.stmts
    );
    let has_return_local = f
        .body
        .block
        .stmts
        .iter()
        .any(|s| matches!(s, MirStmt::Return(Some(MirExpr::Local(_)))));
    assert!(
        has_return_local,
        "In must produce a Return(Some(MirExpr::Local)) for the runtime result, got stmts: {:?}",
        f.body.block.stmts
    );
    let _ = i64_ty;

    let tokens = emit_decls(&mir, &types).expect("end-to-end in must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_op_in"),
        "In must emit a __ts_aot_op_in runtime call, got: {s}"
    );
}

#[test]
fn end_to_end_instanceof_preserves_rhs_and_dispatches_real_check() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("isInstance");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::InstanceOf,
                lhs: Box::new(HirExpr::Int(0)),
                rhs: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Object"),
                    ty: i64_ty,
                }),
                ty: bool_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error on InstanceOf, got {:?}",
        ctx.diagnostics()
    );
    let f = mir.functions().next().expect("one function");
    let runtime_args = f
        .body
        .block
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime {
                op: ts_aot_ir_mir::RuntimeOp::OpInstanceof,
                args,
                ..
            } => Some(args.clone()),
            _ => None,
        })
        .expect("InstanceOf must emit a Runtime stmt with OpInstanceof");
    assert_eq!(
        runtime_args.len(),
        3,
        "OpInstanceof must carry value, rhs (preserved for side effects), AND resolved target_type_id (real check), got {runtime_args:?}"
    );
    let target_name = match &runtime_args[1] {
        MirExpr::Global(name) => name.clone(),
        other => panic!(
            "OpInstanceof args[1] must be the target MirExpr::Global (rhs preserved for side effects), got {other:?}"
        ),
    };
    assert_eq!(
        target_name.as_str(),
        "Object",
        "OpInstanceof args[1] must be the original rhs name, got {target_name:?}"
    );
    let target_type_id = match &runtime_args[2] {
        MirExpr::Int { value, .. } => *value as u32,
        other => panic!(
            "OpInstanceof args[2] must be the resolved target_type_id (MirExpr::Int), got {other:?}"
        ),
    };
    assert_eq!(
        target_type_id, 0,
        "Object's type is i64 (not a struct), so shared_struct_ids lookup fails and target_type_id is 0. The runtime's real check: i64::class_id() (0xFFFF_FF03, reserved high range) != 0, so __ts_aot_op_instanceof returns false (correct: 0 is not an instance of Object)"
    );

    let tokens = emit_decls(&mir, &types).expect("end-to-end instanceof must emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_op_instanceof"),
        "InstanceOf must emit a __ts_aot_op_instanceof runtime call, got: {s}"
    );
    assert!(
        s.contains(", 0u32"),
        "InstanceOf must pass the resolved target_type_id (0u32 for non-struct rhs) to __ts_aot_op_instanceof, got: {s}"
    );
}

#[test]
fn end_to_end_instanceof_side_effectful_rhs_is_preserved() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("isInstance");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: Vec::new(),
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::InstanceOf,
                lhs: Box::new(HirExpr::Int(0)),
                rhs: Box::new(HirExpr::Call {
                    callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                        name: Atom::new_inline("ctor"),
                        ty: i64_ty,
                    })),
                    args: Vec::new(),
                    ty: i64_ty,
                }),
                ty: bool_ty,
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
    let has_call_side_effect = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. }) | MirStmt::Runtime { .. }
        )
    });
    assert!(
        has_call_side_effect,
        "instanceof <side-effectful call> must preserve the rhs call as a side effect stmt (side effect IS evaluated, even when identity is unresolvable), got stmts: {:?}",
        f.body.block.stmts
    );
    let p0005 = ctx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005" && d.message.contains("non-Global"));
    assert!(
        p0005.is_some(),
        "instanceof with non-Global rhs (e.g., getConstructor() call) must emit P0005 diagnostic explaining identity is unresolvable; got diagnostics: {:?}",
        ctx.diagnostics()
    );
}
