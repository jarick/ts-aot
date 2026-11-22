use super::super::emit_function;
use super::empty_func;
use super::norm;
use ts_aot_core::{Atom, LocalId, Type};
use ts_aot_ir_mir::{BinaryOp, MirBlock, MirBody, MirExpr, MirLocalDecl, MirStmt};

#[test]
fn untyped_catch_equality_uses_tag_aware_helper() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let error_ty = types.intern(&Type::Error);
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: error_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock {
                    stmts: vec![MirStmt::If {
                        cond: MirExpr::Binary {
                            op: BinaryOp::Eq,
                            left: Box::new(MirExpr::Local(catch_local)),
                            right: Box::new(MirExpr::Int {
                                value: 1,
                                ty: i64_ty,
                            }),
                            ty: bool_ty,
                        },
                        then_block: MirBlock::new(),
                        else_block: None,
                    }],
                }),
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        s.contains("CaughtValue") && s.contains("__ts_aot_caught_eq"),
        "comparing an untyped catch binding must use the tag-aware helper against a \
         `CaughtValue` (so a thrown number is not stringified), got:\n{s}"
    );
}

#[test]
fn non_primitive_typed_catch_uses_plain_downcast() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let bigint_ty = types.intern(&Type::BigInt);
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: bigint_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    let compact = norm(&s);
    assert!(
        compact.contains("__e.downcast::<ts_aot_runtime::BigIntHandle>()"),
        "an owned non-primitive typed catch must recover the payload via \
         `__e.downcast::<T>()` for the catch binding's own type, got:\n{s}"
    );
    assert!(
        !compact.contains("__ts_aot_catch_typed"),
        "a non-primitive typed catch must not use the primitive extraction helper, got:\n{s}"
    );
}

#[test]
fn arena_typed_catch_binding_is_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: array_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let err = emit_function(&f, &types).expect_err(
        "an arena-backed catch binding must be rejected: `Any::downcast` requires `T: 'static`",
    );
    assert!(
        err.to_string().contains("panic boundary"),
        "the arena-typed catch rejection must explain the `'static` panic boundary, got: {err}"
    );
    assert!(
        err.to_string().contains("catch binding"),
        "the arena-typed catch rejection must name the catch binding, got: {err}"
    );
}

#[test]
fn primitive_typed_catch_uses_catch_typed_helper() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let catch_local = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: catch_local,
            name: Atom::from("e"),
            ty: i64_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Int {
                            value: 1,
                            ty: i64_ty,
                        },
                        error_ty: i64_ty,
                    }],
                },
                catch_param: Some(catch_local),
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let s = emit_function(&f, &types)
        .expect("function should emit")
        .to_string();
    assert!(
        s.contains("__ts_aot_catch_typed"),
        "a primitive-typed catch must recover rethrown `CaughtValue` payloads via \
         `__ts_aot_catch_typed`, got:\n{s}"
    );
    assert!(
        !s.contains("__ts_aot_catch_bind"),
        "a primitive-typed catch must not use the untyped catch-all binding, got:\n{s}"
    );
}

#[test]
fn array_throw_inside_try_is_rejected() {
    let mut types = ts_aot_core::TypeTable::new();
    let i64_ty = types.intern(&Type::I64);
    let array_ty = types.intern(&Type::Array { element: i64_ty });
    let xs = LocalId::from_raw(0);
    let mut f = empty_func("f");
    f.ret = i64_ty;
    f.body = MirBody {
        locals: vec![MirLocalDecl {
            id: xs,
            name: Atom::from("xs"),
            ty: array_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![MirStmt::Try {
                body: MirBlock {
                    stmts: vec![MirStmt::Throw {
                        error: MirExpr::Local(xs),
                        error_ty: array_ty,
                    }],
                },
                catch_param: None,
                catch: Some(MirBlock::new()),
                finally: None,
            }],
        },
    };
    let err = emit_function(&f, &types).expect_err(
        "a thrown arena-backed array must be rejected: the runtime cannot recover its payload",
    );
    assert!(
        err.to_string().contains("panic boundary"),
        "the array-throw rejection must explain the `'static` panic boundary, got: {err}"
    );
}
