use super::super::types::emit_type_id;
use super::super::{emit_decls, emit_function};
use super::empty_func;
use crate::BackendError;
use ts_aot_core::{Atom, ModuleId, Type, TypeId, TypeTable};
use ts_aot_ir_mir::{MirBlock, MirBody, MirDecl, MirExpr, MirProgram, MirStmt};

#[test]
fn emit_type_resolves_primitives() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let bool_id = types.intern(&Type::Bool);
    let tokens = emit_type_id(i32_id, &types);
    assert_eq!(tokens.to_string(), "i32");
    let tokens = emit_type_id(bool_id, &types);
    assert_eq!(tokens.to_string(), "bool");
}

#[test]
fn emit_type_for_unknown_id_emits_placeholder() {
    let types = TypeTable::new();
    let tokens = emit_type_id(TypeId::from_raw(42), &types);
    assert!(tokens.to_string().contains("__ty42"), "got: {tokens}");
}

#[test]
fn emit_type_optional_resolves_inner_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let opt_id = types.intern(&Type::Optional {
        inner: i32_id,
        nullish: ts_aot_core::NullishKinds::BOTH,
    });
    let tokens = emit_type_id(opt_id, &types);
    assert_eq!(tokens.to_string(), "Option < i32 >");
}

#[test]
fn emit_type_array_resolves_element_via_table() {
    let mut types = TypeTable::new();
    let str_id = types.intern(&Type::String);
    let arr_id = types.intern(&Type::Array { element: str_id });
    let tokens = emit_type_id(arr_id, &types);
    assert_eq!(
        tokens.to_string(),
        "ts_aot_runtime :: bumpalo :: collections :: Vec < 'a , ts_aot_runtime :: JsString < 'a > >"
    );
}

#[test]
fn emit_type_result_resolves_ok_and_err_via_table() {
    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let str_id = types.intern(&Type::String);
    let res_id = types.intern(&Type::Result {
        ok: i32_id,
        err: str_id,
    });
    let tokens = emit_type_id(res_id, &types);
    assert_eq!(
        tokens.to_string(),
        "Result < i32 , ts_aot_runtime :: JsString < 'a > >"
    );
}

#[test]
fn emit_type_generator_emits_namespaced_generic() {
    let mut types = TypeTable::new();
    let i64_id = types.intern(&Type::I64);
    let gen_id = types.intern(&Type::Generator {
        inner: i64_id,
        is_async: false,
    });
    let tokens = emit_type_id(gen_id, &types);
    assert_eq!(
        tokens.to_string(),
        "ts_aot_runtime :: Generator < i64 >",
        "Generator must emit as namespaced generic with its yield type"
    );
}

#[test]
fn emit_type_generator_result_emits_namespaced_generic() {
    let mut types = TypeTable::new();
    let i64_id = types.intern(&Type::I64);
    let res_id = types.intern(&Type::GeneratorResult { inner: i64_id });
    let tokens = emit_type_id(res_id, &types);
    assert_eq!(
        tokens.to_string(),
        "ts_aot_runtime :: GeneratorResult < i64 >"
    );
}

#[test]
fn function_returning_result_emits_result_type_with_ok_and_err_stmts() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let str_ty = types.intern(&Type::String);
    let res_ty = types.intern(&Type::Result {
        ok: int_ty,
        err: str_ty,
    });

    let mut f = empty_func("try_get");
    f.ret = res_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::ReturnResultErr {
                    error: MirExpr::String {
                        id: Atom::from("oops"),
                        ty: str_ty,
                    },
                    err_ty: str_ty,
                },
                MirStmt::Return(Some(MirExpr::ResultOk {
                    value: Box::new(MirExpr::Int {
                        value: 42,
                        ty: int_ty,
                    }),
                    ty: res_ty,
                })),
            ],
        },
    };

    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("-> Result < i32 , ts_aot_runtime :: JsString < 'a > >"),
        "ret type must be Result<i32, JsString>, got: {s}"
    );
    assert!(
        s.contains("Err (") && s.contains("\"oops\""),
        "ReturnResultErr must emit `Err(...)` containing the error, got: {s}"
    );
    assert!(
        s.contains("Ok (42)"),
        "ResultOk must emit `Ok(42)`, got: {s}"
    );
    assert!(
        !s.contains("-> ()"),
        "Result ret must not fall back to unit, got: {s}"
    );
}

#[test]
fn cast_to_bool_target_is_rejected_with_internal_error() {
    let mut types = ts_aot_core::TypeTable::new();
    let bool_ty = types.intern(&Type::Bool);
    let mut func = empty_func("cast_to_bool");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Cast {
                expr: Box::new(MirExpr::Int {
                    value: 1,
                    ty: bool_ty,
                }),
                ty: bool_ty,
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let err = emit_decls(&prog, &types)
        .expect_err("Cast to Type::Bool must be rejected — Rust's `as bool` is not valid syntax");
    let BackendError::Internal(msg) = err else {
        panic!("expected BackendError::Internal, got other error variant");
    };
    assert!(
        msg.contains("MirExpr::Cast"),
        "Internal error must mention the MirExpr::Cast context, got: {msg:?}"
    );
    assert!(
        msg.contains("not a numeric primitive"),
        "Internal error must explain the numeric-primitive restriction, got: {msg:?}"
    );
    assert!(
        msg.contains(&bool_ty.raw().to_string()),
        "Internal error must include the offending TypeId raw id, got: {msg:?}"
    );
}

#[test]
fn cast_to_numeric_target_emits_as_cast() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let f64_ty = types.intern(&Type::F64);
    let mut func = empty_func("cast_to_numeric");
    func.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Expr(MirExpr::Cast {
                expr: Box::new(MirExpr::Int {
                    value: 1,
                    ty: i64_ty,
                }),
                ty: f64_ty,
            })],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(func));
    let tokens = emit_decls(&prog, &types).expect("numeric Cast must succeed");
    let s = tokens.to_string();
    assert!(
        s.contains("as f64"),
        "Cast to numeric primitive (f64) must emit `as f64`, got: {s}"
    );
}
