use super::common::*;

#[test]
fn convert_global_with_int_init_lowers_to_int() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("MAX"),
        ty: unit_ty(),
        init: Some(int_lit(42)),
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let g = mir.globals().next().expect("one global");
    assert!(matches!(g.init, Some(MirExpr::Int { value: 42, .. })));
    assert!(!cx.has_errors(), "constant init must not error");
}

#[test]
fn convert_global_with_string_init_lowers_to_string() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("GREETING"),
        ty: unit_ty(),
        init: Some(HirExpr::String(Atom::new_inline("hi"), Span::default())),
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let g = mir.globals().next().expect("one global");
    assert!(matches!(g.init, Some(MirExpr::String { .. })));
}

#[test]
fn convert_global_with_complex_init_emits_warning_and_drops_init() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("X"),
        ty: unit_ty(),
        init: Some(HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: Vec::new(),
            ty: unit_ty(),
            type_args: vec![],

            span: Span::default(),
        }),
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let g = mir.globals().next().expect("one global");
    assert!(
        g.init.is_none(),
        "non-constant global init must be dropped, got {:?}",
        g.init
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0006"),
        "expected P0006 warning for non-constant global init, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_global_does_not_consume_function_id() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("X"),
        ty: unit_ty(),
        init: Some(int_lit(0)),
    });
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("main"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let f = mir.functions().next().expect("one function");
    assert_eq!(
        f.id,
        FunctionId::from_raw(0),
        "Global decl must not shift next_function_id; main must remain at #0"
    );
}

#[test]
fn convert_global_visibility_defaults_to_public() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Global {
        name: Atom::new_inline("X"),
        ty: unit_ty(),
        init: Some(int_lit(0)),
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let g = mir.globals().next().expect("one global");
    assert_eq!(
        g.visibility,
        Visibility::Public,
        "Global visibility must default to Public (per prior behavior, not Private)"
    );
}
