use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt};
use ts_aot_ir_mir::{MirStmt, RuntimeOp};
use ts_aot_passes::{PassContext, convert_program};

fn main() {
    let (mut types, mut ctx) = (TypeTable::new(), PassContext::default());
    let param_ty_id = types.intern(&Type::Named { symbol: Atom::new_inline("any") });
    let dynamic_ty = types.intern(&Type::Dynamic);
    let fn_name = Atom::new_inline("setFoo");
    let mut hir = HirProgram::new(ModuleId::from_raw(0));
    hir.declarations.push(HirDecl::Function(HirFunction {
        name: fn_name,
        params: vec![HirParam { name: Atom::new_inline("x"), ty: param_ty_id }],
        ret: dynamic_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Assignment {
                target: Box::new(HirExpr::Field {
                    owner: Box::new(HirExpr::Local { id: LocalId::from_raw(0), ty: param_ty_id }),
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
    let tokens = ts_aot_backend::emit_decls(&mir, &types).expect("emit must succeed");
    println!("{}", tokens.to_string());
}
