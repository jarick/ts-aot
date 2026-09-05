use ts_aot_core::{DiagnosticCode, StructId, Type, TypeId};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::MirExpr;
use ts_aot_ir_mir::RuntimeOp;

use super::common::*;
use crate::hir_to_mir::convert_expr::weakmap_dispatch::is_supported_weakmap_key;

fn wm_set_call(receiver_ty: TypeId, key: HirExpr, value: HirExpr) -> HirExpr {
    HirExpr::Call {
        callee: ts_aot_ir_hir::HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: receiver_ty,
                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("set"),
            ty: TypeId::from_raw(0),
            span: Span::default(),
        })),
        args: vec![key, value],
        ty: receiver_ty,
        type_args: Vec::new(),
        span: Span::default(),
    }
}

#[test]
fn is_supported_weakmap_key_accepts_struct_type() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(7),
    });
    assert!(
        is_supported_weakmap_key(key_ty, &types),
        "resolved struct key type must be accepted by is_supported_weakmap_key"
    );
}

#[test]
fn is_supported_weakmap_key_accepts_placeholder() {
    let types = empty_types();
    assert!(
        is_supported_weakmap_key(TypeId::from_raw(0), &types),
        "placeholder TypeId 0 must be accepted for backward compat with `new WeakMap()`"
    );
}

#[test]
fn is_supported_weakmap_key_rejects_unsupported_types() {
    let mut types = empty_types();
    let _ = types.intern(&Type::Null);
    let i64_ty = types.intern(&Type::I64);
    let bool_ty = types.intern(&Type::Bool);
    let string_ty = types.intern(&Type::String);
    assert!(
        !is_supported_weakmap_key(i64_ty, &types),
        "Type::I64 is not a valid WeakMap key"
    );
    assert!(
        !is_supported_weakmap_key(bool_ty, &types),
        "Type::Bool is not a valid WeakMap key"
    );
    assert!(
        !is_supported_weakmap_key(string_ty, &types),
        "Type::String is not a valid WeakMap key"
    );
}

#[test]
fn weakmap_set_with_struct_key_and_i64_value_dispatches_runtime_call() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(3),
    });
    let value_ty = types.intern(&Type::I64);
    let wm_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: value_ty,
    });

    let key_expr = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: key_ty,
        span: Span::default(),
    };
    let value_expr = int_lit(42);

    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();

    let mir = c.convert_expr(
        &wm_set_call(wm_ty, key_expr, value_expr),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );

    assert!(
        !cx.has_errors(),
        "WeakMap<Struct, i64>.set with concrete struct key and i64 value must lower without errors, got: {:?}",
        cx.diagnostics()
    );

    let runtime_calls: Vec<_> = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::WeakMapSet,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        runtime_calls.len(),
        1,
        "exactly one WeakMapSet runtime stmt must be emitted; got stmts: {out:?}"
    );
    assert!(
        matches!(mir, MirExpr::Local(_)),
        "the call expression itself must lower to a MirExpr::Local binding the result, got {mir:?}"
    );
}

#[test]
fn weakmap_set_with_struct_literal_key_is_rejected_with_e0406() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(3),
    });
    let value_ty = types.intern(&Type::I64);
    let wm_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: value_ty,
    });

    let key_expr = HirExpr::StructLiteral {
        ty: key_ty,
        fields: Vec::new(),
        span: Span::default(),
    };
    let value_expr = int_lit(42);

    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();

    let _ = c.convert_expr(
        &wm_set_call(wm_ty, key_expr, value_expr),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );

    assert!(
        cx.has_errors(),
        "WeakMap.set with a struct-literal key must produce an E0406 diagnostic; got: {:?}",
        cx.diagnostics()
    );
    let e0406: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0406"))
        .collect();
    assert!(
        !e0406.is_empty(),
        "diagnostic bag must contain an E0406 entry for the struct-literal key; got: {:?}",
        cx.diagnostics()
    );
    assert!(
        e0406[0]
            .message
            .contains("WeakMap keys must be local variables"),
        "E0406 message must explain that keys must be local variables; got: {:?}",
        e0406[0].message
    );
    assert!(
        out.iter().all(|s| !matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::WeakMapSet,
                ..
            }
        )),
        "no WeakMapSet runtime stmt must be emitted when the key is rejected; got stmts: {out:?}"
    );
}

#[test]
fn weakmap_set_with_field_access_key_is_rejected_with_e0406() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(3),
    });
    let value_ty = types.intern(&Type::I64);
    let wm_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: value_ty,
    });

    let key_expr = HirExpr::Field {
        owner: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: key_ty,
            span: Span::default(),
        }),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("x"),
        ty: value_ty,
        span: Span::default(),
    };
    let value_expr = int_lit(42);

    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();

    let _ = c.convert_expr(
        &wm_set_call(wm_ty, key_expr, value_expr),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );

    assert!(
        cx.has_errors(),
        "WeakMap.set with a field-access key (a.b) must produce an E0406 diagnostic; got: {:?}",
        cx.diagnostics()
    );
    let e0406: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0406"))
        .collect();
    assert!(
        !e0406.is_empty(),
        "diagnostic bag must contain an E0406 entry for the field-access key; got: {:?}",
        cx.diagnostics()
    );
    assert!(
        e0406[0].message.contains("field access"),
        "E0406 message must call out field access as unsupported; got: {:?}",
        e0406[0].message
    );
    assert!(
        out.iter().all(|s| !matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::WeakMapSet,
                ..
            }
        )),
        "no WeakMapSet runtime stmt must be emitted when the field key is rejected; got stmts: {out:?}"
    );
}

#[test]
fn weakmap_set_with_index_access_key_is_rejected_with_e0406() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(3),
    });
    let value_ty = types.intern(&Type::I64);
    let arr_ty = types.intern(&Type::Array { element: key_ty });
    let wm_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: value_ty,
    });

    let key_expr = HirExpr::Index {
        owner: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: arr_ty,
            span: Span::default(),
        }),
        index: Box::new(int_lit(0)),
        ty: key_ty,
        span: Span::default(),
    };
    let value_expr = int_lit(42);

    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();

    let _ = c.convert_expr(
        &wm_set_call(wm_ty, key_expr, value_expr),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );

    assert!(
        cx.has_errors(),
        "WeakMap.set with an index-access key (a[i]) must produce an E0406 diagnostic; got: {:?}",
        cx.diagnostics()
    );
    let e0406: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0406"))
        .collect();
    assert!(
        e0406.iter().any(|d| d.message.contains("index access")),
        "E0406 message must call out index access as unsupported; got: {:?}",
        e0406
    );
    assert!(
        out.iter().all(|s| !matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::WeakMapSet,
                ..
            }
        )),
        "no WeakMapSet runtime stmt must be emitted when the index key is rejected; got stmts: {out:?}"
    );
}

#[test]
fn weakmap_with_string_value_rejected_with_e0406_and_no_runtime_op() {
    let mut types = empty_types();
    let key_ty = types.intern(&Type::Struct {
        id: StructId::from_raw(3),
    });
    let value_ty = types.intern(&Type::String);
    let wm_ty = types.intern(&Type::WeakMap {
        key: key_ty,
        value: value_ty,
    });

    let key_expr = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: key_ty,
        span: Span::default(),
    };
    let value_expr = int_lit(42);

    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();

    let _ = c.convert_expr(
        &wm_set_call(wm_ty, key_expr, value_expr),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );

    assert!(
        cx.has_errors(),
        "WeakMap<Struct, String>.set must produce an E0406 diagnostic for unsupported value type; got: {:?}",
        cx.diagnostics()
    );
    let e0406: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code == DiagnosticCode::from("E0406"))
        .collect();
    assert!(
        e0406.iter().any(|d| d.message.contains("value")),
        "E0406 message must mention value type issue; got: {:?}",
        e0406
    );
    assert!(
        out.iter().all(|s| !matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::WeakMapSet,
                ..
            }
        )),
        "no WeakMapSet runtime stmt must be emitted when value type is unsupported; got stmts: {out:?}"
    );
}
