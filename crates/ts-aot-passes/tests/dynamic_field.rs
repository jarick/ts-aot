use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirStmt, RuntimeOp};
use ts_aot_passes::{PassContext, convert_program};

fn named_any_ty() -> Type {
    Type::Named {
        symbol: Atom::new_inline("any"),
    }
}

fn named_object_ty() -> Type {
    Type::Named {
        symbol: Atom::new_inline("Object"),
    }
}

fn named_unknown_ty() -> Type {
    Type::Named {
        symbol: Atom::new_inline("unknown"),
    }
}

fn run_convert_with_param_type(param_ty: Type) -> Vec<MirStmt> {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let fn_name = Atom::new_inline("getField");
    let param_ty_id = types.intern(&param_ty);
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: ts_aot_core::LocalId::from_raw(0),
                    ty: param_ty_id,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
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
    f.body.block.stmts.clone()
}

#[test]
fn end_to_end_any_field_access_emits_object_get() {
    let stmts = run_convert_with_param_type(named_any_ty());
    let runtime_stmt = stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                args,
                ..
            } => Some(args.clone()),
            _ => None,
        })
        .expect("any field access must emit Runtime stmt with OpObjectGet");
    assert_eq!(
        runtime_stmt.len(),
        2,
        "OpObjectGet needs (obj, field_name) args"
    );
    match &runtime_stmt[1] {
        ts_aot_ir_mir::MirExpr::String { id, .. } => {
            assert_eq!(
                id.as_str(),
                "foo",
                "field name must be preserved as String arg"
            );
        }
        other => panic!("args[1] must be MirExpr::String with field name, got {other:?}"),
    }
}

#[test]
fn end_to_end_object_field_access_emits_object_get() {
    let stmts = run_convert_with_param_type(named_object_ty());
    assert!(
        stmts.iter().any(|s| matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )),
        "Object-typed field access must emit OpObjectGet, got stmts: {stmts:?}"
    );
}

#[test]
fn end_to_end_unknown_field_access_emits_object_get() {
    let stmts = run_convert_with_param_type(named_unknown_ty());
    assert!(
        stmts.iter().any(|s| matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )),
        "unknown-typed field access must emit OpObjectGet, got stmts: {stmts:?}"
    );
}

#[test]
fn end_to_end_emit_dynamic_field_access_uses_object_get_helper() {
    let stmts = run_convert_with_param_type(named_any_ty());
    let _ = stmts;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let fn_name = Atom::new_inline("getField");
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: ts_aot_core::LocalId::from_raw(0),
                    ty: param_ty_id,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
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
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_get"),
        "emitted Rust must call __ts_aot_dynamic_get for dynamic field access, got: {s}"
    );
    assert!(
        s.contains("\"foo\""),
        "emitted Rust must include the field name \"foo\" as a string literal, got: {s}"
    );
    assert!(
        s.contains("DynamicValue"),
        "emitted Rust must use the DynamicValue type for the parameter/local \
         (Type::Dynamic denotes the value, not the container), got: {s}"
    );
}

#[test]
fn dynamic_field_emit_pattern_compiles_with_runtime_stub() {
    use ts_aot_core::LocalId;
    use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};

    enum DynamicValue {
        Undefined,
    }

    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&Type::Dynamic);
    let dynamic_ty = param_ty_id;
    let fn_name = Atom::new_inline("get_field");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: param_ty_id,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
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
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();

    fn __ts_aot_dynamic_get_stub(_v: &DynamicValue, _name: &str) -> DynamicValue {
        DynamicValue::Undefined
    }
    fn generated_pattern_check(x: DynamicValue) -> DynamicValue {
        let owner: DynamicValue = x;
        let local: DynamicValue = __ts_aot_dynamic_get_stub(&owner, "foo");
        local
    }
    let _ = generated_pattern_check;

    assert!(
        s.contains("__ts_aot_dynamic_get (&"),
        "emit must call __ts_aot_dynamic_get with a borrow, got: {s}"
    );
    assert!(
        s.contains("let _ : DynamicValue = __ts_aot_dynamic_get"),
        "emit must bind dest with type DynamicValue (Type::Dynamic = value), got: {s}"
    );
    assert!(
        s.contains("fn get_field (x : DynamicValue) -> DynamicValue"),
        "emit must use DynamicValue for both param and return, got: {s}"
    );
}

#[test]
fn dynamic_field_set_emit_pattern_compiles_with_runtime_stub() {
    use ts_aot_core::LocalId;
    use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};

    enum DynamicValue {
        Undefined,
        #[allow(dead_code)]
        Integer(i64),
    }

    impl From<i64> for DynamicValue {
        fn from(v: i64) -> Self {
            DynamicValue::Integer(v)
        }
    }

    fn __ts_aot_dynamic_set_stub(
        _target: &mut DynamicValue,
        _field_name: &str,
        _value: DynamicValue,
    ) {
    }
    fn __ts_aot_dynamic_get_stub(_v: &DynamicValue, _name: &str) -> DynamicValue {
        DynamicValue::Undefined
    }

    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&Type::Dynamic);
    let dynamic_ty = param_ty_id;
    let fn_name = Atom::new_inline("set_field");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: ts_aot_core::FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
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
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();

    fn generated_set_pattern_check(x: DynamicValue) -> DynamicValue {
        let mut owner: DynamicValue = x;
        let v: i64 = 42;
        let boxed: DynamicValue = DynamicValue::from(v);
        __ts_aot_dynamic_set_stub(&mut owner, "foo", boxed);
        let _ = __ts_aot_dynamic_get_stub(&owner, "foo");
        owner
    }
    let _ = generated_set_pattern_check;

    assert!(
        s.contains("__ts_aot_dynamic_set (& mut"),
        "set emit must produce `&mut <owner>` (mutable place), got: {s}"
    );
    assert!(
        s.contains("\"foo\""),
        "set emit must include the field name literal, got: {s}"
    );
    assert!(
        s.contains("fn set_field (x : DynamicValue) -> DynamicValue"),
        "set emit must use DynamicValue for both param and return, got: {s}"
    );
}

#[test]
fn dynamic_field_compound_update_emit_pattern_compiles_with_runtime_stub() {
    use ts_aot_core::LocalId;
    use ts_aot_ir_hir::{
        HirBinaryOp, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
    };

    enum DynamicValue {
        Undefined,
    }

    fn __ts_aot_dynamic_set_stub(
        _target: &mut DynamicValue,
        _field_name: &str,
        _value: DynamicValue,
    ) {
    }
    fn __ts_aot_dynamic_get_stub(_v: &DynamicValue, _name: &str) -> DynamicValue {
        DynamicValue::Undefined
    }
    fn __ts_aot_dynamic_op_stub(
        _op: u8,
        _left: &DynamicValue,
        _right: &DynamicValue,
    ) -> DynamicValue {
        DynamicValue::Undefined
    }

    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&Type::Dynamic);
    let dynamic_ty = param_ty_id;
    let fn_name = Atom::new_inline("inc_field");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: ts_aot_core::FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
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

    fn generated_compound_update_pattern_check(x: DynamicValue) -> DynamicValue {
        let mut owner: DynamicValue = x;
        let _old: DynamicValue = __ts_aot_dynamic_get_stub(&owner, "foo");
        let _new: DynamicValue = __ts_aot_dynamic_op_stub(0u8, &_old, &DynamicValue::Undefined);
        __ts_aot_dynamic_set_stub(&mut owner, "foo", _new);
        owner
    }
    let _ = generated_compound_update_pattern_check;

    assert!(
        s.contains("let mut"),
        "compound update must bind owner as `let mut` (mutable place for set), got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_set (& mut"),
        "compound update set emit must use `&mut <owner>`, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_op"),
        "compound update must route binary op through __ts_aot_dynamic_op, got: {s}"
    );
    assert!(
        s.contains("fn inc_field (x : DynamicValue) -> DynamicValue"),
        "compound update emit must use DynamicValue, got: {s}"
    );
}

#[test]
fn dynamic_field_set_optional_owner_emit_pattern_compiles_with_runtime_stub() {
    use ts_aot_core::LocalId;
    use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};

    enum DynamicValue {
        Undefined,
    }

    fn __ts_aot_dynamic_set_stub(
        _target: &mut DynamicValue,
        _field_name: &str,
        _value: DynamicValue,
    ) {
    }
    fn __ts_aot_dynamic_unwrap_stub(_v: std::option::Option<DynamicValue>) -> DynamicValue {
        DynamicValue::Undefined
    }

    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: dynamic_ty });
    let fn_name = Atom::new_inline("set_opt_field");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: opt_ty,
                    }),
                    field: ts_aot_core::FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
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
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();

    fn generated_optional_set_pattern_check(x: std::option::Option<DynamicValue>) -> DynamicValue {
        let mut unwrapped: DynamicValue = __ts_aot_dynamic_unwrap_stub(x);
        __ts_aot_dynamic_set_stub(&mut unwrapped, "foo", DynamicValue::Undefined);
        unwrapped
    }
    let _ = generated_optional_set_pattern_check;

    assert!(
        s.contains("let mut"),
        "Optional<Dynamic> set must bind unwrapped as `let mut`, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_set (& mut"),
        "Optional<Dynamic> set must use `&mut <unwrapped>`, got: {s}"
    );
    assert!(
        s.contains("fn set_opt_field (x : Option < DynamicValue >) -> DynamicValue"),
        "Optional<Dynamic> set emit must use Option<DynamicValue> for param, got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_assignment_unit_value_emits_undefined() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                value: Box::new(HirExpr::Unit),
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
        s.contains("DynamicValue :: Undefined"),
        "unit RHS of dynamic field assignment must box to DynamicValue::Undefined \
         (no From<()> impl exists; explicit case in DynamicFrom emit), got: {s}"
    );
    assert!(
        !s.contains("DynamicValue :: from ()"),
        "unit RHS must NOT use DynamicValue::from(()) (no impl), got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_assignment_null_value_emits_null() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                value: Box::new(HirExpr::Null),
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
        s.contains("DynamicValue :: Null"),
        "null RHS of dynamic field assignment must box to DynamicValue::Null \
         (emit MirExpr::Null is otherwise rendered as `()`), got: {s}"
    );
    assert!(
        !s.contains("DynamicValue :: from ()"),
        "null RHS must NOT use DynamicValue::from(()), got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_assignment_value_init_emits_dynamic_from() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
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
    let boxed_init = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(ts_aot_ir_mir::MirExpr::DynamicFrom { .. }),
                ..
            }
        )
    });
    assert!(
        boxed_init,
        "dynamic field assignment must box the value via MirExpr::DynamicFrom \
         (preserves DynamicValue type for the temp, matches runtime __ts_aot_dynamic_set), \
         got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("DynamicValue :: from"),
        "emitted Rust must box value via DynamicValue::from (RuntimeOp call shape), got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_assignment_emits_object_set() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
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
        has_object_set,
        "dynamic field assignment must emit OpObjectSet, got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_set"),
        "emitted Rust must call __ts_aot_dynamic_set for dynamic field assignment, got: {s}"
    );
    assert!(
        s.contains("\"foo\""),
        "emitted Rust must include the field name \"foo\" as a string literal, got: {s}"
    );
}

#[test]
fn end_to_end_in_on_dynamic_emits_object_has() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::String(Atom::new_inline("foo"))),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: param_ty_id,
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
    let has_object_has = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    assert!(
        has_object_has,
        "in operator on dynamic value must emit OpObjectHas, got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_has"),
        "emitted Rust must call __ts_aot_dynamic_has for `in` on dynamic, got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_compound_update_emits_get_compute_set() {
    use ts_aot_ir_hir::HirBinaryOp;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("incFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
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
    let has_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    let has_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert!(
        has_get,
        "dynamic field compound update must emit OpObjectGet (read current value), got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        has_set,
        "dynamic field compound update must emit OpObjectSet (write new value), got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_get"),
        "emit must include get, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_set"),
        "emit must include set, got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_delete_emits_object_delete() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("delFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: ts_aot_ir_hir::HirUnaryOp::Delete,
                expr: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
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
    let has_object_delete = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectDelete,
                ..
            }
        )
    });
    assert!(
        has_object_delete,
        "delete on dynamic field must emit OpObjectDelete, got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_delete"),
        "emitted Rust must call __ts_aot_dynamic_delete for delete on dynamic, got: {s}"
    );
}

#[test]
fn end_to_end_optional_dynamic_field_access_emits_object_get() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let inner_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: inner_ty });
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: inner_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("foo"),
                ty: inner_ty,
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
    let has_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        has_get,
        "Optional<Dynamic> owner must be unwrapped to dynamic and emit OpObjectGet, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_named_any_field_access_emits_object_get() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let any_ty = types.intern(&named_any_ty());
    let opt_ty = types.intern(&Type::Optional { inner: any_ty });
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_ty,
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
    let has_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        has_get,
        "Optional<any> owner must unwrap to dynamic and emit OpObjectGet, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_i64_field_keeps_direct_access() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let opt_ty = types.intern(&Type::Optional { inner: i64_ty });
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: i64_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("foo"),
                ty: i64_ty,
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
    let has_runtime_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        !has_runtime_get,
        "Optional<i64> owner is NOT dynamic — must NOT emit OpObjectGet, got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_nested_optional_dynamic_field_access_emits_object_get() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_inner = types.intern(&Type::Optional { inner: dynamic_ty });
    let opt_outer = types.intern(&Type::Optional { inner: opt_inner });
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_outer,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_outer,
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
    let has_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        has_get,
        "Optional<Optional<Dynamic>> must recurse and emit OpObjectGet, got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_dynamic_field_compound_update_unsupported_op_does_not_silently_use_add() {
    use ts_aot_ir_hir::HirBinaryOp;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("bitAndFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::BitAnd,
                rhs: Box::new(HirExpr::Int(7)),
                post: false,
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
    let has_op_dynamic_binary = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpDynamicBinary,
                ..
            }
        )
    });
    assert!(
        !has_op_dynamic_binary,
        "unsupported operator (BitAnd) on dynamic compound update must NOT emit \
         OpDynamicBinary (which would silently use fallback ADD per old map_dynamic_op), \
         got stmts: {:?}",
        f.body.block.stmts
    );
    let has_diagnostic = ctx.diagnostics().iter().any(|d| d.code == "P0005".into());
    assert!(
        has_diagnostic,
        "unsupported dynamic compound update op must emit a P0005 diagnostic so the \
         silent fallback is replaced by a loud failure"
    );
}

#[test]
fn end_to_end_dynamic_field_compound_update_emits_dynamic_binary_runtime() {
    use ts_aot_ir_hir::HirBinaryOp;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("incFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
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
    let has_op_dynamic_binary = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpDynamicBinary,
                ..
            }
        )
    });
    assert!(
        has_op_dynamic_binary,
        "compound update for dynamic field must route the binary op through \
         OpDynamicBinary (via __ts_aot_dynamic_op), not MirExpr::Binary, got stmts: {:?}",
        f.body.block.stmts
    );
    let has_inline_binary_dynamic = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(ts_aot_ir_mir::MirExpr::Binary { ty, .. }),
                ..
            } if *ty == dynamic_ty
        )
    });
    assert!(
        !has_inline_binary_dynamic,
        "compound update must NOT materialize MirExpr::Binary with dynamic type \
         (left DynamicValue + right raw would not compile), got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_op"),
        "emitted Rust must call __ts_aot_dynamic_op for dynamic compound update, got: {s}"
    );
    assert!(
        s.contains("DynamicValue :: from"),
        "rhs must be boxed via DynamicValue::from for the dynamic op, got: {s}"
    );
}

#[test]
fn end_to_end_dynamic_field_compound_update_postfix_returns_old_value() {
    use ts_aot_ir_hir::HirBinaryOp;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("postInc");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: param_ty_id,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: true,
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
    let has_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    let has_set = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    let get_dest = f.body.block.stmts.iter().find_map(|s| match s {
        MirStmt::Runtime {
            op: RuntimeOp::OpObjectGet,
            dest: Some(d),
            ..
        } => Some(*d),
        _ => None,
    });
    let set_arg_local = f.body.block.stmts.iter().find_map(|s| match s {
        MirStmt::Runtime {
            op: RuntimeOp::OpObjectSet,
            args,
            ..
        } => args.iter().find_map(|a| match a {
            ts_aot_ir_mir::MirExpr::Local(id) => Some(*id),
            _ => None,
        }),
        _ => None,
    });
    let returned_local = match f.body.block.stmts.last() {
        Some(MirStmt::Return(Some(ts_aot_ir_mir::MirExpr::Local(id)))) => Some(*id),
        _ => None,
    };
    assert!(
        has_get && has_set,
        "postfix compound update must still emit get+set, got stmts: {:?}",
        f.body.block.stmts
    );
    let get_dest = get_dest.expect(
        "OpObjectGet must have a dest binding (the old/pre-update value), got stmts: \
         {f.body.block.stmts:?}",
    );
    let returned_local = returned_local.expect(
        "postfix must still return a Local (set + return sequence), got stmts: \
         {f.body.block.stmts:?}",
    );
    assert_eq!(
        returned_local, get_dest,
        "postfix must return the OpObjectGet destination (pre-update value), not the new value, \
         got stmts: {:?}",
        f.body.block.stmts
    );
    if let Some(set_arg) = set_arg_local {
        assert_ne!(
            set_arg, returned_local,
            "postfix must NOT return the value passed to OpObjectSet (that is the new/post-update value), \
             got stmts: {:?}",
            f.body.block.stmts
        );
    }
}

#[test]
fn end_to_end_known_container_in_keeps_op_in() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let i64_ty = types.intern(&Type::I64);
    let vec_ty = types.intern(&Type::Array { element: i64_ty });
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasIdx");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("v"),
            ty: vec_ty,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::String(Atom::new_inline("foo"))),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: vec_ty,
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
    let has_object_has = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    assert!(
        !has_object_has,
        "in on non-dynamic Vec must NOT emit OpObjectHas (regression check), got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_known_struct_field_keeps_direct_access() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let class_ty = types.intern(&Type::I64);
    let field_ty = types.intern(&Type::I64);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    let class_id_raw = 0u32;
    hir.declarations.push(HirDecl::Class(HirClass {
        ty: types.intern(&Type::Named {
            symbol: Atom::new_inline("MyStruct"),
        }),
        name: Atom::new_inline("MyStruct"),
        fields: vec![HirField {
            name: Atom::new_inline("foo"),
            ty: field_ty,
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    let _ = class_ty;
    let _ = class_id_raw;
    let fn_name = Atom::new_inline("getField");
    let mystruct_ty = types.intern(&Type::Named {
        symbol: Atom::new_inline("MyStruct"),
    });
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: mystruct_ty,
        }],
        ret: field_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: ts_aot_core::LocalId::from_raw(0),
                    ty: mystruct_ty,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: Atom::new_inline("foo"),
                ty: field_ty,
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
    let has_runtime_get = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        !has_runtime_get,
        "known struct field access must NOT emit OpObjectGet (regression check), got stmts: {:?}",
        f.body.block.stmts
    );
    let has_direct_field = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Return(Some(ts_aot_ir_mir::MirExpr::Field { .. }))
        )
    });
    assert!(
        has_direct_field,
        "known struct field access must keep direct MirExpr::Field, got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_dynamic_field_access_emits_unwrap_before_get() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let inner_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: inner_ty });
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: inner_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("foo"),
                ty: inner_ty,
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
    let unwrap_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                ..
            }
        )
    });
    let get_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    assert!(
        unwrap_idx.is_some(),
        "Optional<Dynamic> owner must emit OpObjectUnwrap before OpObjectGet, got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        get_idx.is_some(),
        "Optional<Dynamic> field access must still emit OpObjectGet, got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        unwrap_idx.unwrap() < get_idx.unwrap(),
        "OpObjectUnwrap must appear before OpObjectGet (normalize owner first), \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_nested_optional_dynamic_field_access_emits_two_unwraps() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_inner = types.intern(&Type::Optional { inner: dynamic_ty });
    let opt_outer = types.intern(&Type::Optional { inner: opt_inner });
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_outer,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_outer,
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
    let unwrap_count = f
        .body
        .block
        .stmts
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::OpObjectUnwrap,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        unwrap_count, 2,
        "Optional<Optional<Dynamic>> must emit two OpObjectUnwrap (one per Optional layer), \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_bare_dynamic_field_access_does_not_emit_unwrap() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("getFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
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
    let unwrap_count = f
        .body
        .block
        .stmts
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::OpObjectUnwrap,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        unwrap_count, 0,
        "bare Dynamic owner must NOT emit OpObjectUnwrap (already DynamicValue, no Optional layer), \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_dynamic_assignment_emits_unwrap_before_set() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: dynamic_ty });
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: opt_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
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
    let unwrap_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                ..
            }
        )
    });
    let set_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert!(
        unwrap_idx.is_some() && set_idx.is_some() && unwrap_idx.unwrap() < set_idx.unwrap(),
        "Optional<Dynamic> assignment must emit OpObjectUnwrap before OpObjectSet, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_dynamic_in_emits_unwrap_before_has() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: dynamic_ty });
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::String(Atom::new_inline("foo"))),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: opt_ty,
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
    let unwrap_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                ..
            }
        )
    });
    let has_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    assert!(
        unwrap_idx.is_some() && has_idx.is_some() && unwrap_idx.unwrap() < has_idx.unwrap(),
        "Optional<Dynamic> `in` must emit OpObjectUnwrap before OpObjectHas, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_dynamic_delete_emits_unwrap_before_delete() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: dynamic_ty });
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("delFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Unary {
                op: ts_aot_ir_hir::HirUnaryOp::Delete,
                expr: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: opt_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
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
    let unwrap_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                ..
            }
        )
    });
    let delete_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectDelete,
                ..
            }
        )
    });
    assert!(
        unwrap_idx.is_some() && delete_idx.is_some() && unwrap_idx.unwrap() < delete_idx.unwrap(),
        "Optional<Dynamic> delete must emit OpObjectUnwrap before OpObjectDelete, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_optional_dynamic_compound_update_emits_unwraps_for_get_and_set() {
    use ts_aot_ir_hir::HirBinaryOp;
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let dynamic_ty = types.intern(&Type::Dynamic);
    let opt_ty = types.intern(&Type::Optional { inner: dynamic_ty });
    let fn_name = Atom::new_inline("incFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: opt_ty,
        }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::CompoundUpdate {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local {
                        id: LocalId::from_raw(0),
                        ty: opt_ty,
                    }),
                    field: FieldId::from_raw(0),
                    field_name: Atom::new_inline("foo"),
                    ty: dynamic_ty,
                }),
                op: HirBinaryOp::Add,
                rhs: Box::new(HirExpr::Int(1)),
                post: false,
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
    let unwrap_count = f
        .body
        .block
        .stmts
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::OpObjectUnwrap,
                    ..
                }
            )
        })
        .count();
    let unwrap_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectUnwrap,
                ..
            }
        )
    });
    let get_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectGet,
                ..
            }
        )
    });
    let set_idx = f.body.block.stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectSet,
                ..
            }
        )
    });
    assert_eq!(
        unwrap_count, 1,
        "Optional<Dynamic> compound update must emit one OpObjectUnwrap (reused via temp for both get and set), \
         got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        unwrap_idx.is_some()
            && get_idx.is_some()
            && set_idx.is_some()
            && unwrap_idx.unwrap() < get_idx.unwrap()
            && unwrap_idx.unwrap() < set_idx.unwrap(),
        "Optional<Dynamic> compound update must emit OpObjectUnwrap before both OpObjectGet and OpObjectSet, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}

#[test]
fn end_to_end_in_on_dynamic_with_evaluated_string_local_emits_object_has() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let string_ty = types.intern(&Type::String);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasKey");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![
            HirParam {
                name: Atom::new_inline("key"),
                ty: string_ty,
            },
            HirParam {
                name: Atom::new_inline("obj"),
                ty: param_ty_id,
            },
        ],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: string_ty,
                }),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: param_ty_id,
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
    let has_object_has = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    let has_op_in = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpIn,
                ..
            }
        )
    });
    assert!(
        has_object_has,
        "evaluated string local on dynamic `in` must emit OpObjectHas, got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        !has_op_in,
        "evaluated string local on dynamic `in` must NOT fall through to generic OpIn, \
         got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_key"),
        "emitted Rust must call __ts_aot_dynamic_key to box evaluated string key, got: {s}"
    );
    assert!(
        s.contains("__ts_aot_dynamic_has"),
        "emitted Rust must call __ts_aot_dynamic_has for dynamic `in`, got: {s}"
    );
}

#[test]
fn end_to_end_in_on_dynamic_with_literal_string_still_emits_object_has() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::String(Atom::new_inline("foo"))),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: param_ty_id,
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
    let has_object_has = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    assert!(
        has_object_has,
        "literal string lhs on dynamic `in` must still emit OpObjectHas (regression), \
         got stmts: {:?}",
        f.body.block.stmts
    );
    let tokens = emit_decls(&mir, &types).expect("emit must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_key"),
        "literal string lhs must also be boxed via __ts_aot_dynamic_key (unified path), got: {s}"
    );
    assert!(
        s.contains("\"foo\""),
        "emitted Rust must include the literal field name \"foo\", got: {s}"
    );
}

#[test]
fn end_to_end_in_on_dynamic_with_non_string_lhs_keeps_op_in() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&named_any_ty());
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let fn_name = Atom::new_inline("hasKey");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("x"),
            ty: param_ty_id,
        }],
        ret: bool_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Binary {
                op: HirBinaryOp::In,
                lhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: i64_ty,
                }),
                rhs: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(1),
                    ty: param_ty_id,
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
    let has_object_has = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpObjectHas,
                ..
            }
        )
    });
    let has_op_in = f.body.block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::OpIn,
                ..
            }
        )
    });
    assert!(
        !has_object_has,
        "non-string lhs on dynamic `in` must NOT emit OpObjectHas (key is not a string), \
         got stmts: {:?}",
        f.body.block.stmts
    );
    assert!(
        has_op_in,
        "non-string lhs on dynamic `in` must fall through to generic OpIn, \
         got stmts: {:?}",
        f.body.block.stmts
    );
}
