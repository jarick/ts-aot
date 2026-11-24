use super::super::ident::sanitize_ident;
use super::super::{emit_decls, emit_function};
use super::{empty_func, signature_fragment};
use ts_aot_core::{Atom, LocalId, ModuleId, Type, TypeTable};
use ts_aot_ir_mir::{
    BinaryOp, FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirLocalDecl, MirParam,
    MirProgram, MirStmt,
};

#[test]
fn plain_function_emits_fn_signature() {
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(empty_func("greet")));
    let types = TypeTable::new();
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(s.contains("fn greet"), "got: {s}");
    assert!(s.contains("->"), "expected ret arrow, got: {s}");
}

#[test]
fn exported_function_emits_pub_keyword() {
    let mut f = empty_func("render");
    f.export_name = Some("render".to_owned());
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.starts_with("pub "), "expected `pub` prefix, got: {s}");
}

#[test]
fn private_function_omits_pub_keyword() {
    let tokens =
        emit_function(&empty_func("internal"), &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("pub "),
        "private function should not have `pub`, got: {s}"
    );
}

#[test]
fn async_function_emits_async_keyword() {
    let mut f = empty_func("fetch_data");
    f.effects.is_async = true;
    let tokens = emit_function(&f, &TypeTable::new()).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("async fn"), "got: {s}");
}

#[test]
fn function_body_return_emits_expression() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let mut f = empty_func("answer");
    f.ret = int_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 42,
                ty: int_ty,
            }))],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("return 42"), "got: {s}");
    assert!(!s.contains("unimplemented"), "got: {s}");
}

#[test]
fn empty_void_body_emits_empty_block_not_unimplemented() {
    let mut types = TypeTable::new();
    let void_ty = types.intern(&Type::Void);
    let mut f = empty_func("noop");
    f.ret = void_ty;
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("unimplemented"),
        "an empty void body must not emit `unimplemented!()`, got: {s}"
    );
    assert!(s.contains("{ }"), "expected an empty body block, got: {s}");
}

#[test]
fn empty_non_void_body_emits_unreachable_not_unimplemented() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let mut f = empty_func("missing");
    f.ret = int_ty;
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(
        !s.contains("unimplemented"),
        "an empty non-void body must not emit `unimplemented!()`, got: {s}"
    );
    assert!(
        s.contains("unreachable"),
        "an empty non-void body must emit `unreachable!()`, got: {s}"
    );
}

#[test]
fn function_body_let_binary_and_return_emits_statements() {
    let mut types = TypeTable::new();
    let int_ty = types.intern(&Type::I32);
    let local = LocalId::from_raw(0);
    let mut f = empty_func("sum");
    f.ret = int_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: local,
            name: Atom::from("total"),
            ty: int_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Let {
                    local,
                    ty: int_ty,
                    init: Some(MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Int {
                            value: 1,
                            ty: int_ty,
                        }),
                        right: Box::new(MirExpr::Int {
                            value: 2,
                            ty: int_ty,
                        }),
                        ty: int_ty,
                    }),
                    mutable: false,
                },
                MirStmt::Return(Some(MirExpr::Local(local))),
            ],
        },
    };
    let tokens = emit_function(&f, &types).expect("function should emit");
    let s = tokens.to_string();
    assert!(s.contains("let total : i32 = 1 + 2"), "got: {s}");
    assert!(s.contains("return total"), "got: {s}");
}

#[test]
fn sanitize_ident_replaces_dash_with_underscore() {
    assert_eq!(sanitize_ident("foo-bar"), "foo_bar");
}

#[test]
fn sanitize_ident_prefixes_digit_start() {
    assert_eq!(sanitize_ident("7greet"), "_7greet");
}

#[test]
fn sanitize_ident_appends_underscore_to_keyword() {
    assert_eq!(sanitize_ident("type"), "type_");
    assert_eq!(sanitize_ident("fn"), "fn_");
    assert_eq!(sanitize_ident("try"), "try_");
    assert_eq!(sanitize_ident("gen"), "gen_");
}

#[test]
fn dynamic_import_emits_turbofish_for_unused_namespace() {
    let mut types = TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let str_ty = types.intern(&Type::String);
    let mut f = empty_func("load_module");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![
                MirStmt::Expr(MirExpr::Import {
                    source: Box::new(MirExpr::String {
                        id: Atom::from("./mod.js"),
                        ty: str_ty,
                    }),
                    ty: i64_ty,
                }),
                MirStmt::Return(Some(MirExpr::Int {
                    value: 0,
                    ty: i64_ty,
                })),
            ],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("__ts_aot_dynamic_import :: < i64 >"),
        "Standalone (unused) dynamic import must carry the resolved namespace type as a turbofish, \
         otherwise the generic parameter is unconstrained and the call fails to type-check. Got: {s}"
    );
}

#[test]
fn underscore_param_uses_consistent_ident_in_signature_and_body() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(0);
    let mut f = empty_func("forward");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("_"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(param)))],
        },
    };
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();

    let sig = signature_fragment(&s, "forward");
    assert!(
        sig.contains("__tmp0 : i32"),
        "frontend-emitted `_` param must be renamed to a consistent `__tmp0` in the signature, not \
         left as a bare `_` (Rust would treat `_` as a wildcard binding and refuse to address it). \
         Got signature fragment: `{sig}`"
    );
    assert!(
        !sig.contains(" _ : i32") && !sig.contains("(_ : i32"),
        "signature must NOT emit a literal `_ : i32` (anonymous wildcard) for the param, got: `{sig}`"
    );
    assert!(
        s.contains("return __tmp0"),
        "body must use the SAME `__tmp0` ident the signature declared, otherwise the function \
         fails to compile (signature names one binding, body references another). Got: `{s}`"
    );
    assert!(
        !s.contains("return _ ;") && !s.contains("return (_"),
        "body must NOT use a bare `_` (Rust would not accept `_` as an expression). Got: `{s}`"
    );
}

#[test]
fn dispatch_wrapper_renames_underscore_param_to_usable_ident() {
    let mut types = TypeTable::new();
    let i32_ty = types.intern(&Type::I32);
    let param = LocalId::from_raw(0);
    let mut f = empty_func("dispatchable");
    f.ret = i32_ty;
    f.params = vec![MirParam {
        id: param,
        name: Atom::from("_"),
        ty: i32_ty,
    }];
    f.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Int {
                value: 0,
                ty: i32_ty,
            }))],
        },
    };
    f.kind = FunctionKind::Plain;
    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Function(f));
    let tokens = emit_decls(&prog, &types).expect("decls should emit");
    let s = tokens.to_string();
    assert!(
        s.contains("pub fn __ts_aot_dispatch_dispatchable"),
        "dispatchable plain function with packable ret+args must produce a `__ts_aot_dispatch_*` \
         wrapper, got: {s}"
    );
    let wrapper_pos = s
        .find("pub fn __ts_aot_dispatch_dispatchable")
        .expect("wrapper position");
    let wrapper_body = s
        .get(wrapper_pos..)
        .and_then(|rest| {
            let open = rest.find('{')?;
            let close = rest.rfind('}')?;
            rest.get(open..=close)
        })
        .unwrap_or("");
    assert!(
        wrapper_body.contains("let __tmp0 : i32 = __slot_0 as i32 ;"),
        "dispatch wrapper must unpack `_args[0]` into a renamed `__tmp0 : i32` local (not a bare \
         `_` which Rust would treat as an anonymous wildcard binding and refuse to address), got: \
         `{wrapper_body}`"
    );
    assert!(
        wrapper_body.contains("dispatchable (__ctx , __tmp0)"),
        "dispatch wrapper must call the inner function with the renamed `__tmp0` arg, got: \
         `{wrapper_body}`"
    );
    assert!(
        !wrapper_body.contains("dispatchable (_)"),
        "dispatch wrapper must NOT pass a bare `_` as the inner call argument (Rust would reject \
         the call site), got: `{wrapper_body}`"
    );
}
