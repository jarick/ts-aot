use ts_aot_core::{Atom, ModuleId, Span, Type, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirDecl, MirExpr};
use ts_aot_passes::{PassContext, convert_program};

#[test]
fn convert_program_preserves_global_with_int_init() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let name_sym = Atom::new_inline("ANSWER");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Global {
        name: name_sym.clone(),
        ty: types.intern(&ts_aot_core::Type::I64),
        init: Some(HirExpr::Int(42, Span::default())),
    });

    let mir = convert_program(&hir, &mut types, &mut ctx);

    assert_eq!(mir.declarations.len(), 1);
    let MirDecl::Global(g) = &mir.declarations[0] else {
        panic!("expected MirDecl::Global");
    };
    assert_eq!(g.name, name_sym);
    let typed_id = types.intern(&ts_aot_core::Type::I64);
    assert_eq!(g.ty, typed_id, "global.ty must be the i64 from HIR");
    let Some(init) = &g.init else {
        panic!("init must be preserved through HIR->MIR");
    };
    let MirExpr::Int { value, ty } = init else {
        panic!("expected Int init, got {init:?}");
    };
    assert_eq!(*value, 42);
    assert_eq!(*ty, g.ty, "init.ty must match global.ty, not TypeId(0)");
}

#[test]
fn convert_function_with_throw_sets_throws() {
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

    let mir = convert_program(&hir, &mut types, &mut ctx);
    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(fns.len(), 1);
    let f = fns[0];
    assert!(
        f.throws.is_some(),
        "convert_function must populate throws when body has Throw"
    );
    assert!(f.effects.can_throw);
}

#[test]
fn convert_function_without_throw_leaves_throws_none() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let name = Atom::new_inline("ok");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name,
        params: Vec::new(),
        ret: types.intern(&Type::Void),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Int(1, Span::default()),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mir = convert_program(&hir, &mut types, &mut ctx);
    let f = mir.functions().next().expect("one function");
    assert!(f.throws.is_none());
    assert!(!f.effects.can_throw);
}
