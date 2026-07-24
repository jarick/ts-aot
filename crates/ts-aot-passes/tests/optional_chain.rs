use ts_aot_backend::emit_decls;
use ts_aot_core::{Atom, LocalId, ModuleId, Span, Type, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirDecl, MirFieldDecl, MirStructDecl};
use ts_aot_passes::{PassContext, convert_program, lower_result};

#[test]
fn end_to_end_optional_chain_field_emit_uses_as_ref_map() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let point = types.intern(&Type::I64);
    let point_struct_id = ts_aot_core::StructId::from_raw(0);
    let point_struct = types.intern(&Type::Struct {
        id: point_struct_id,
    });
    let opt_point = types.intern(&Type::Optional {
        inner: point_struct,
    });
    let opt_i64 = types.intern(&Type::Optional { inner: point });
    let fn_name = Atom::new_inline("getX");
    let obj_local = LocalId::from_raw(0);
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam {
            name: Atom::new_inline("obj"),
            ty: opt_point,
        }],
        ret: opt_i64,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                span: Span::default(),
                owner: Box::new(HirExpr::OptionalChain {
                    span: Span::default(),
                    base: Box::new(HirExpr::Local {
                        span: Span::default(),
                        id: obj_local,
                        ty: opt_point,
                    }),
                    ty: opt_i64,
                }),
                field: ts_aot_core::FieldId::from_raw(0),
                field_name: Atom::new_inline("x"),
                ty: opt_i64,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut mir = convert_program(&hir, &mut types, &mut ctx);
    lower_result(&mut mir, &mut types);
    mir.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_struct_id,
        name: Atom::from("Point"),
        fields: vec![MirFieldDecl {
            id: ts_aot_core::FieldId::from_raw(0),
            name: Atom::from("x"),
            ty: point,
            mutable: false,
            visibility: ts_aot_core::Visibility::Public,
        }],
        methods: Vec::new(),
    }));
    let tokens = emit_decls(&mir, &types).expect("end-to-end optional chain field must emit");
    let s = tokens.to_string();

    assert!(
        s.contains("obj . as_ref () . map (| o | o . x)"),
        "end-to-end OptionalChain + Field with shared types must emit `obj.as_ref().map(|o| o.x)` \
         If this fails with `obj . x` (no as_ref), convert and emit are seeing different \
         TypeTables. Got: {s}"
    );
}
