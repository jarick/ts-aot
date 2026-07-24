use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, ModuleId, Span, Type, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirProgram, HirStmt};
use ts_aot_ir_mir::MirStmt;
use ts_aot_passes::{PassContext, convert_program, lower_result};

#[test]
fn end_to_end_lower_result_rewrites_throw_to_return_result_err() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let name = Atom::new_inline("oops");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: types.intern(&Type::Void),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Int(7, Span::default()),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut types, &mut ctx);
    lower_result(&mut mir, &mut types);
    assert!(
        !ctx.has_errors(),
        "convert_program + lower_result must not error for result.rs:27, got {:?}",
        ctx.diagnostics()
    );

    let f = mir.functions().next().expect("one function");
    assert!(f.throws.is_some());
    assert_eq!(f.body.block.stmts.len(), 1);
    assert!(
        matches!(f.body.block.stmts[0], MirStmt::ReturnResultErr { .. }),
        "Throw must be rewritten to ReturnResultErr by lower_result, got {:?}",
        f.body.block.stmts[0]
    );
}

#[test]
fn end_to_end_throwing_function_emits_result_in_rust_signature() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let int_ty = types.intern(&Type::I32);
    let name = Atom::new_inline("boom");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: int_ty,
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Int(7, Span::default()),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "convert_program must not error for result.rs:61, got {:?}",
        ctx.diagnostics()
    );
    let pre_throws = mir.functions().next().expect("one function").throws;
    assert!(
        pre_throws.is_some(),
        "convert_program must set f.throws when body has Throw"
    );
    lower_result(&mut mir, &mut types);

    let f = mir.functions().next().expect("one function");
    let Some(Type::Result { ok, err: _ }) = types.resolve(f.ret) else {
        panic!(
            "lower_result must wrap f.ret in Type::Result when f.throws is set, got {:?}",
            types.resolve(f.ret)
        );
    };
    assert_eq!(
        *ok, int_ty,
        "Result.ok must be the original return type (i32)"
    );
    assert!(
        f.body
            .block
            .stmts
            .iter()
            .any(|s| matches!(s, MirStmt::ReturnResultErr { .. }))
    );

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 ,") || s.contains("-> Result<i32 ,"),
        "emitted Rust signature must show Result<i32, ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        !s.contains("-> ()"),
        "ret must not fall back to unit, got: {s}"
    );
}

#[test]
fn end_to_end_throwing_function_with_success_return_emits_ok() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let int_ty = types.intern(&Type::I32);
    let name = Atom::new_inline("maybe_boom");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: int_ty,
        throws: None,
        body: vec![
            HirStmt::If {
                cond: HirExpr::Bool(true, Span::default()),
                then: Box::new(HirStmt::Throw {
                    expr: HirExpr::Int(7, Span::default()),
                }),
                otherwise: None,
            },
            HirStmt::Return {
                value: Some(HirExpr::Int(42, Span::default())),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut types, &mut ctx);
    lower_result(&mut mir, &mut types);
    assert!(
        !ctx.has_errors(),
        "convert_program + lower_result must not error for result.rs:134, got {:?}",
        ctx.diagnostics()
    );

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 ,") || s.contains("-> Result<i32 ,"),
        "ret must be Result<i32, ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        s.contains("Ok (42)"),
        "Success Return(42) must be lowered to `Ok(42)`, got: {s}"
    );
    assert!(
        !s.contains("return 42 ;"),
        "Bare `return 42;` would not typecheck in Result-returning fn, got: {s}"
    );
}

#[test]
fn end_to_end_throwing_void_function_bare_return_emits_ok_unit() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let unit_ty = types.intern(&Type::Void);
    let name = Atom::new_inline("void_boom");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: unit_ty,
        throws: None,
        body: vec![
            HirStmt::If {
                cond: HirExpr::Bool(true, Span::default()),
                then: Box::new(HirStmt::Throw {
                    expr: HirExpr::Int(7, Span::default()),
                }),
                otherwise: None,
            },
            HirStmt::Return { value: None },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut types, &mut ctx);
    lower_result(&mut mir, &mut types);
    assert!(
        !ctx.has_errors(),
        "convert_program + lower_result must not error for result.rs:185, got {:?}",
        ctx.diagnostics()
    );

    let tokens = emit_decls(&mir, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < () ,")
            || s.contains("-> Result<()> ,")
            || s.contains("Result < ()"),
        "ret must be Result<(), ...>, got: {s}"
    );
    assert!(
        s.contains("Err (7)"),
        "Throw must be lowered to `Err(7)`, got: {s}"
    );
    assert!(
        s.contains("Ok (())"),
        "Bare return must be lowered to `Ok(())`, got: {s}"
    );
    assert!(
        !s.contains("return ;"),
        "Bare `return;` would not typecheck in Result-returning fn, got: {s}"
    );
}
