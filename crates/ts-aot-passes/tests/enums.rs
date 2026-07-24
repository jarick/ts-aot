use ts_aot_core::{Atom, ModuleId, Span, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirEnumVariant, HirExpr, HirFunction, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirExpr, MirGlobalDecl, MirStmt};
use ts_aot_passes::{PassContext, convert_program, lower_enums};

fn build_enum_decl(name: &str, variants: Vec<(&str, Option<i64>)>) -> HirDecl {
    let variants = variants
        .into_iter()
        .map(|(n, v)| HirEnumVariant {
            name: Atom::new_inline(n),
            value: v.map(|v| HirExpr::Int(v, Span::default())),
        })
        .collect();
    HirDecl::Enum {
        name: Atom::new_inline(name),
        variants,
    }
}

fn build_enum_decl_returning_sym(
    name: &str,
    variants: Vec<(&str, Option<i64>)>,
) -> (HirDecl, ts_aot_core::Atom) {
    let enum_name = Atom::new_inline(name);
    let variants = variants
        .into_iter()
        .map(|(n, v)| HirEnumVariant {
            name: Atom::new_inline(n),
            value: v.map(|v| HirExpr::Int(v, Span::default())),
        })
        .collect();
    (
        HirDecl::Enum {
            name: enum_name.clone(),
            variants,
        },
        enum_name,
    )
}

#[test]
fn lower_enums_then_convert_program_emits_globals_with_values() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(build_enum_decl(
        "Color",
        vec![("Red", None), ("Green", Some(10)), ("Blue", None)],
    ));

    lower_enums(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "lower_enums + convert_program must not error for enums.rs:51, got {:?}",
        ctx.diagnostics()
    );

    let globals: Vec<&MirGlobalDecl> = mir.globals().collect();
    assert_eq!(
        globals.len(),
        3,
        "enum with 3 variants must produce 3 MirDecl::Global"
    );

    let mut by_name: Vec<(String, i128)> = Vec::new();
    for g in globals {
        let raw = g.name.as_str().to_owned();
        let val = match &g.init {
            Some(MirExpr::Int { value, .. }) => *value,
            other => panic!("expected Int init for {raw}, got {other:?}"),
        };
        by_name.push((raw, val));
    }
    by_name.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(by_name[0].0, "Color.Blue");
    assert_eq!(by_name[0].1, 11);
    assert_eq!(by_name[1].0, "Color.Green");
    assert_eq!(by_name[1].1, 10);
    assert_eq!(by_name[2].0, "Color.Red");
    assert_eq!(by_name[2].1, 0);
}

#[test]
fn end_to_end_enum_through_hir_to_mir_dump_includes_values() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations
        .push(build_enum_decl("E", vec![("A", None), ("B", None)]));

    lower_enums(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "lower_enums + convert_program must not error for enums.rs:87, got {:?}",
        ctx.diagnostics()
    );
    let text = mir.dump_text();
    assert!(text.contains("global"), "expected global in dump:\n{text}");

    let globals: Vec<_> = mir.globals().collect();
    assert_eq!(globals.len(), 2);
    let mut by_name: Vec<(String, i128)> = globals
        .into_iter()
        .map(|g| {
            let raw = g.name.as_str().to_owned();
            let val = match &g.init {
                Some(MirExpr::Int { value, .. }) => *value,
                other => panic!("expected Int init for {raw}, got {other:?}"),
            };
            (raw, val)
        })
        .collect();
    by_name.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(by_name[0].0, "E.A");
    assert_eq!(by_name[0].1, 0);
    assert_eq!(by_name[1].0, "E.B");
    assert_eq!(by_name[1].1, 1);
    assert!(
        text.contains("= 0(:0)"),
        "dump must render init=0 explicitly for E.A:\n{text}"
    );
    assert!(
        text.contains("= 1(:0)"),
        "dump must render init=1 explicitly for E.B:\n{text}"
    );
}

#[test]
fn enum_member_use_in_function_body_is_rewritten_to_namespaced_global() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let mut hir = HirProgram::new(ModuleId::from_raw(0));

    let (enum_decl, color_sym) =
        build_enum_decl_returning_sym("Color", vec![("Red", None), ("Green", Some(10))]);
    hir.declarations.push(enum_decl);

    let typed_id = types.intern(&ts_aot_core::Type::I64);
    let green_name = Atom::new_inline("Green");
    let fn_name = Atom::new_inline("pick");

    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name.clone(),
        params: Vec::new(),
        ret: typed_id,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Global {
                    name: color_sym.clone(),
                    ty: typed_id,

                    span: Span::default(),
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: green_name.clone(),
                ty: typed_id,

                span: Span::default(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    lower_enums(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    assert!(
        !ctx.has_errors(),
        "lower_enums + convert_program must not error for enums.rs:156, got {:?}",
        ctx.diagnostics()
    );

    let fns: Vec<_> = mir.functions().collect();
    assert_eq!(fns.len(), 1);
    let f = fns[0];
    assert_eq!(f.name, fn_name.clone());

    let MirStmt::Return(Some(ret_expr)) = &f.body.block.stmts[0] else {
        panic!(
            "expected Return(Some(expr)), got {:?}",
            f.body.block.stmts[0]
        );
    };
    let MirExpr::Global(resolved) = ret_expr else {
        panic!(
            "Color.Green use must be rewritten to MirExpr::Global, got {:?}",
            ret_expr
        );
    };
    let expected = Atom::new_inline("Color.Green");
    assert_eq!(
        *resolved, expected,
        "Field(Global(Color), Green) must rewrite to Global(Color.Green)"
    );

    let text = mir.dump_text();
    assert!(
        text.contains("Color.Green"),
        "dump must show the namespaced global:\n{text}"
    );
}
