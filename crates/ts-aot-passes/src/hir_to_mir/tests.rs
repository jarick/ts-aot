use std::collections::HashMap;

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, Span, TypeId, TypeTable, Visibility,
};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
    HirSwitchCase, HirUnaryOp,
};
use ts_aot_ir_mir::{
    BinaryOp, ConstValue, FunctionKind, MirExpr, MirPlace, MirPlaceBase, MirStmt, RuntimeOp,
    UnaryOp,
};

use super::{ExprConverter, HirBlock, PLACEHOLDER_FUNCTION, convert_function, convert_program};
use crate::PassContext;
use crate::lower_classes;

fn ctx() -> PassContext {
    PassContext::new()
}

fn int_lit(v: i64) -> HirExpr {
    HirExpr::Int(v)
}

fn unit_ty() -> TypeId {
    TypeId::from_raw(0)
}

fn empty_hir() -> HirProgram {
    HirProgram::new(ModuleId::from_raw(0))
}

fn empty_struct_ids() -> std::collections::HashMap<ts_aot_core::TypeId, ts_aot_core::StructId> {
    std::collections::HashMap::new()
}

fn empty_next_struct() -> u32 {
    0
}

fn empty_types() -> TypeTable {
    TypeTable::new()
}

fn empty_field_id_lookup() -> HashMap<(ts_aot_core::StructId, Atom), FieldId> {
    HashMap::new()
}

#[test]
fn converter_starts_with_empty_state() {
    let c = ExprConverter::new();
    assert_eq!(c.peek_next_local(), 0);
}

#[test]
fn default_matches_new() {
    let a = ExprConverter::default();
    let b = ExprConverter::new();
    assert_eq!(a.peek_next_local(), b.peek_next_local());
}

#[test]
fn fresh_local_increments_counter() {
    let mut c = ExprConverter::new();
    let l0 = c.map_local_id(LocalId::from_raw(0));
    let l1 = c.map_local_id(LocalId::from_raw(1));
    assert_ne!(l0, l1);
    assert_eq!(c.peek_next_local(), 2);
}

#[test]
fn with_function_remap_and_offset_starts_past_offset() {
    let c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 5);
    assert_eq!(c.peek_next_local(), 5);
    let c2 = ExprConverter::with_function_remap(HashMap::new());
    assert_eq!(c2.peek_next_local(), 0);
}

#[test]
fn seed_params_advances_next_local_past_param_count() {
    let mut c = ExprConverter::with_function_remap_and_offset(HashMap::new(), 0);
    c.seed_params(3);
    assert_eq!(c.peek_next_local(), 3);
    let fresh = c.map_local_id(LocalId::from_raw(99));
    assert_eq!(fresh, LocalId::from_raw(3));
}

#[test]
fn map_local_returns_same_id_for_same_old() {
    let mut c = ExprConverter::new();
    let src = LocalId::from_raw(42);
    let a = c.map_local(src);
    let b = c.map_local(src);
    assert_eq!(a, b);
    assert_eq!(c.peek_next_local(), 1);
}

#[test]
fn map_local_id_returns_local_id() {
    let mut c = ExprConverter::new();
    let old = LocalId::from_raw(7);
    let new = c.map_local_id(old);
    assert_eq!(c.map_local_id(old), new);
}

#[test]
fn register_local_name_does_not_panic() {
    let mut c = ExprConverter::new();
    let id = LocalId::from_raw(0);
    c.register_local_name(id, Atom::new_inline("11"));
}

#[test]
fn resolve_callee_function_uses_remap() {
    let mut remap = HashMap::new();
    remap.insert(FunctionId::from_raw(3), FunctionId::from_raw(99));
    let mut c = ExprConverter::with_function_remap(remap);
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(3)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(99));
}

#[test]
fn resolve_callee_function_without_remap_returns_input() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Function(FunctionId::from_raw(7)), &mut cx);
    assert_eq!(fid, FunctionId::from_raw(7));
}

#[test]
fn resolve_callee_indirect_is_placeholder_and_warning() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Indirect(Box::new(int_lit(1))), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(
        !cx.has_errors(),
        "PR 1.2: unresolved indirect callee downgrades P0005 to warning (runtime fallback handles it)"
    );
    let p0005_count = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .count();
    assert_eq!(
        p0005_count, 1,
        "P0005 must still be emitted as a warning, got {p0005_count} diags"
    );
}

#[test]
fn resolve_callee_closure_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Closure(LocalId::from_raw(0)), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}

#[test]
fn resolve_callee_runtime_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(
        &HirCallee::Runtime {
            name: Atom::new_inline("0"),
            ty: TypeId::from_raw(0),
        },
        &mut cx,
    );
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
}

#[test]
fn convert_expr_unit_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Unit,
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Unit
    );
    assert!(out.is_empty());
}

#[test]
fn convert_expr_bool_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Bool(true),
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Bool(true)
    );
}

#[test]
fn convert_expr_int_emits_struct_with_value() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &int_lit(42),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Int { value, .. } => assert_eq!(value, 42),
        other => panic!("expected Int, got {other:?}"),
    }
    assert!(out.is_empty());
}

#[test]
fn convert_expr_string_emits_string() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &HirExpr::String(Atom::new_inline("5")),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::String { id, .. } => assert_eq!(id, Atom::new_inline("5")),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn convert_expr_null_emits_null() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &HirExpr::Null,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Null { .. }));
}

#[test]
fn convert_expr_undefined_becomes_unit() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    assert_eq!(
        c.convert_expr(
            &HirExpr::Undefined,
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Unit
    );
}

#[test]
fn convert_expr_local_remaps_id() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let old = LocalId::from_raw(7);
    let expr = HirExpr::Local {
        id: old,
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Local(lid) => assert_ne!(lid, old),
        other => panic!("expected Local, got {other:?}"),
    }
    assert_eq!(c.peek_next_local(), 1);
}

#[test]
fn convert_expr_global_passes_through() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Global {
        name: Atom::new_inline("13"),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(mir, MirExpr::Global(Atom::new_inline("13")));
}

#[test]
fn convert_expr_binary_converts_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Binary {
        op: HirBinaryOp::Add,
        lhs: Box::new(int_lit(1)),
        rhs: Box::new(int_lit(2)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(
        mir,
        MirExpr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ));
}

#[test]
fn convert_expr_unary_converts_op() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Unary {
        op: HirUnaryOp::Not,
        expr: Box::new(HirExpr::Bool(true)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(
        mir,
        MirExpr::Unary {
            op: UnaryOp::Not,
            ..
        }
    ));
}

#[test]
fn convert_expr_field_converts_owner() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Field {
        owner: Box::new(int_lit(0)),
        field: FieldId::from_raw(3),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Field { field, .. } if field == FieldId::from_raw(3)));
}

#[test]
fn convert_expr_index_converts_parts() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Index {
        owner: Box::new(int_lit(0)),
        index: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::Index { .. }));
}

#[test]
fn convert_expr_call_resolves_callee() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(2)),
        args: vec![int_lit(1)],
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    match mir {
        MirExpr::Call { callee, args, .. } => {
            assert_eq!(callee, FunctionId::from_raw(2));
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Call, got {other:?}"),
    }
}

#[test]
fn convert_expr_struct_literal_converts_fields() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::StructLiteral {
        ty: unit_ty(),
        fields: vec![(FieldId::from_raw(0), int_lit(7))],
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(matches!(mir, MirExpr::StructLiteral { .. }));
}

#[test]
fn convert_expr_distinct_struct_literal_types_get_distinct_struct_ids() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let type_a = TypeId::from_raw(11);
    let type_b = TypeId::from_raw(22);
    let mut shared_ids = empty_struct_ids();
    let mut shared_next = empty_next_struct();
    let mir_a = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_a,
            fields: Vec::new(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let mir_b = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_b,
            fields: Vec::new(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let id_a = match mir_a {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    let id_b = match mir_b {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    assert_ne!(
        id_a, id_b,
        "distinct HIR types must map to distinct MIR StructIds (got {id_a:?} and {id_b:?})"
    );
    let mir_a_again = c.convert_expr(
        &HirExpr::StructLiteral {
            ty: type_a,
            fields: Vec::new(),
        },
        out,
        &mut shared_ids,
        &mut shared_next,
        &mut empty_types(),
        &mut cx,
    );
    let id_a_again = match mir_a_again {
        MirExpr::StructLiteral { struct_id, .. } => struct_id,
        other => panic!("expected StructLiteral, got {other:?}"),
    };
    assert_eq!(
        id_a, id_a_again,
        "same HIR type must map to the same MIR StructId across calls"
    );
}

#[test]
fn convert_expr_array_emits_runtime_stmt() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::ArrayLiteral {
        elements: vec![int_lit(1), int_lit(2)],
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(
        out[0],
        MirStmt::Runtime {
            op: RuntimeOp::ArrayCreate,
            dest: Some(_),
            ..
        }
    ));
}

#[test]
fn convert_expr_array_returns_local_to_dest() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::ArrayLiteral {
        elements: vec![int_lit(1)],
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let dest_id = match &out[0] {
        MirStmt::Runtime { dest: Some(d), .. } => *d,
        other => panic!("expected Runtime with dest, got {other:?}"),
    };
    assert_eq!(mir, MirExpr::Local(dest_id));
}

#[test]
fn convert_expr_template_returns_local_to_dest() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Template {
        tag: None,
        expressions: vec![int_lit(1)],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let runtime_count = out
        .iter()
        .filter(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        runtime_count, 2,
        "3-part template must chain via N-1 = 2 StringConcat Runtime stmts; got out={out:?}"
    );
    let final_dest = match out.last().expect("at least one stmt") {
        MirStmt::Runtime { dest: Some(d), .. } => *d,
        other => panic!("expected trailing StringConcat Runtime with dest, got {other:?}"),
    };
    assert_eq!(
        mir,
        MirExpr::Local(final_dest),
        "convert_expr must return the LAST chained concat's dest local"
    );
}

#[test]

fn convert_expr_await_emits_mir_await_expr() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Await {
        expr: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 0);
    assert!(matches!(mir, MirExpr::Await { expr: _, ty: _ }));
}

#[test]
fn convert_expr_closure_returns_unit_and_diagnostics() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Closure {
        id: LocalId::from_raw(0),
        params: Vec::new(),
        captures: Vec::new(),
        body: Vec::new(),
        ty: unit_ty(),
    };
    assert_eq!(
        c.convert_expr(
            &expr,
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
            &mut empty_types(),
            &mut cx
        ),
        MirExpr::Unit
    );
    assert!(cx.has_errors());
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 diagnostic for Closure");
    assert!(diag.message.contains("closure"));
}

#[test]
fn convert_expr_assignment_to_local_emits_local_place() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(local),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        out.len(),
        2,
        "Assignment must emit Let value + Assign (no return-clone), got {out:?}"
    );
    let MirStmt::Let {
        init: Some(value_init),
        local: value_temp,
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=value, got {:?}", out[0]);
    };
    assert!(
        matches!(value_init, MirExpr::Int { value: 7, .. }),
        "Let init must capture the original RHS expression, got {value_init:?}"
    );
    assert!(matches!(
        out[1],
        MirStmt::Assign {
            target: ts_aot_ir_mir::MirPlace::Local { .. },
            value: MirExpr::Local(_),
            ..
        }
    ));
    let MirStmt::Assign {
        value: assign_value,
        ..
    } = &out[1]
    else {
        panic!("expected Assign, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = assign_value else {
        panic!(
            "Assign value must load from the materialized value temp (no value.clone), got {assign_value:?}"
        );
    };
    assert_eq!(
        *assign_src, *value_temp,
        "Assign value must point at the same temp as the Let init"
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_expr_assignment_returns_assigned_value() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let local = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(local),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let MirExpr::Local(returned) = mir else {
        panic!("assignment must yield MirExpr::Local(value_temp), got {mir:?}");
    };
    let MirStmt::Let {
        local: value_temp, ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=value, got {:?}", out[0]);
    };
    assert_eq!(
        returned, *value_temp,
        "assignment must yield the same temp that holds the assigned value"
    );
}

#[test]

fn convert_expr_assignment_to_invalid_target_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(call),
        value: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(cx.has_errors());
    assert!(
        !out.iter().any(|s| matches!(s, MirStmt::Assign { .. })),
        "no Assign must be emitted for invalid target, got {out:?}"
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0006")
        .expect("expected P0006 diagnostic for invalid assignment target");
    assert_eq!(diag.message, "expression is not a valid assignment target");
}

#[test]
fn convert_expr_assignment_to_field_emits_field_place() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let base = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let field = HirExpr::Field {
        owner: Box::new(base),
        field: FieldId::from_raw(2),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 2);
    assert!(matches!(&out[0], MirStmt::Let { init: Some(_), .. }));
    assert!(matches!(
        &out[1],
        MirStmt::Assign {
            target: ts_aot_ir_mir::MirPlace::Field { .. },
            value: MirExpr::Local(_),
            ..
        }
    ));
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Local-owner field access with no struct id registered for unit_ty() must surface P0012 (missing struct id) instead of silently dropping to placeholder; got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_assignment_to_indexed_field_emits_field_with_index_base() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let idx = HirExpr::Local {
        id: LocalId::from_raw(1),
        ty: unit_ty(),
    };
    let indexed = HirExpr::Index {
        owner: Box::new(arr),
        index: Box::new(idx),
        ty: unit_ty(),
    };
    let field = HirExpr::Field {
        owner: Box::new(indexed),
        field: FieldId::from_raw(3),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(out.len(), 2);
    let MirStmt::Assign { target, .. } = &out[1] else {
        panic!("expected out[1] = Assign, got {:?}", out[1]);
    };
    match target {
        ts_aot_ir_mir::MirPlace::Field { base, field, .. } => {
            assert_eq!(*field, FieldId::from_raw(3));
            assert!(matches!(**base, ts_aot_ir_mir::MirPlaceBase::Index { .. }));
        }
        other => panic!("expected Field place with Index base, got {other:?}"),
    }
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Index-owner field access with no struct id registered must surface P0012; got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_optional_chain_wraps_ty_as_optional_of_inner_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let expr = HirExpr::OptionalChain {
        base: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: unit_ty(),
        }),
        ty: TypeId::from_raw(7),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    assert!(!cx.has_errors());
    let MirExpr::OptionalChain { base, ty } = &mir else {
        panic!("expected MirExpr::OptionalChain, got {mir:?}");
    };
    assert!(matches!(base.as_ref(), MirExpr::Local(_)));
    let expected_opt_ty = types.intern(&ts_aot_core::Type::Optional {
        inner: TypeId::from_raw(0),
    });
    assert_eq!(
        *ty, expected_opt_ty,
        "convert_expr must wrap OptionalChain.ty as Type::Optional {{ inner: <base_inner.ty> }} \
         (PR 1.4 frontend-type-analysis closure). \
         Frontend sets ty to inner type, backend Optional-aware path needs Type::Optional wrapper."
    );
}

#[test]
fn convert_expr_assignment_to_optional_chain_field_emits_chain_base() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let obj = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let chain_base = HirExpr::OptionalChain {
        base: Box::new(obj),
        ty: unit_ty(),
    };
    let target = HirExpr::Field {
        owner: Box::new(chain_base),
        field: FieldId::from_raw(2),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let MirStmt::Assign { target, .. } = &out[out.len() - 1] else {
        panic!("expected final stmt to be MirStmt::Assign for obj?.x = y, got {out:?}");
    };
    let MirPlace::Field { base, field, .. } = target else {
        panic!("expected MirPlace::Field, got {target:?}");
    };
    assert_eq!(*field, FieldId::from_raw(2));
    let MirPlaceBase::Chain {
        base: chain_base_mir,
        ..
    } = base.as_ref()
    else {
        panic!(
            "MirPlace::Field.base must be MirPlaceBase::Chain (PR 1.4: obj?.x = y wires Chain through mir_expr_to_place), got {base:?}"
        );
    };
    assert!(
        matches!(chain_base_mir.as_ref(), MirExpr::Local(_)),
        "MirPlaceBase::Chain.base must be the materialized inner expression (Local), \
         not wrapped in MirExpr::OptionalChain (PR 1.4: the inverse mapping \
         `Chain -> OptionalChain` lives in mir_place_base_to_expr, kept intact). \
         Got: {chain_base_mir:?}"
    );
}

#[test]
fn convert_expr_indirect_call_emits_indirect_call_arm_for_optional_chain_callee() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut types = empty_types();
    let fn_ty = types.intern(&ts_aot_core::Type::I64);
    let opt_fn_ty = types.intern(&ts_aot_core::Type::Optional { inner: fn_ty });
    let obj = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: opt_fn_ty,
    };
    let optional_chain_callee = HirExpr::OptionalChain {
        base: Box::new(obj),
        ty: opt_fn_ty,
    };
    let expr = HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(optional_chain_callee)),
        args: vec![int_lit(7)],
        ty: fn_ty,
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut types,
        &mut cx,
    );
    let MirExpr::IndirectCall { callee, args, .. } = &mir else {
        panic!(
            "expected MirExpr::IndirectCall (PR 1.4: HirCallee::Indirect must always emit IndirectCall, \
             no Runtime::CallIndirect fallback), got {mir:?}"
        );
    };
    let MirExpr::OptionalChain { .. } = callee.as_ref() else {
        panic!(
            "IndirectCall.callee must be the OptionalChain expression (not materialized), got {callee:?}"
        );
    };
    assert_eq!(args.len(), 1, "call args must be preserved");
}

#[test]
fn convert_block_empty_produces_empty() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let (block, locals) = c.convert_block(&HirBlock(Vec::new()), &mut empty_types(), &mut cx);
    assert!(block.is_empty());
    assert!(locals.is_empty());
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_await_emits_mir_await_expr_without_temp_local() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(HirExpr::Await {
            expr: Box::new(int_lit(1)),
            ty: unit_ty(),
        }),
    }]);
    let (mir, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        locals.is_empty(),
        "await no longer needs a temp local (no state machine), got: {locals:?}"
    );
    assert!(
        matches!(
            &mir.stmts.as_slice(),
            [MirStmt::Return(Some(MirExpr::Await { expr: _, ty: _ }))],
        ),
        "expected Return(MirExpr::Await), got: {:?}",
        mir.stmts
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_direct_drains_new_alloc_temp_local() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(HirExpr::New {
            callee: Box::new(HirExpr::Global {
                name: Atom::new_inline("99"),
                ty: unit_ty(),
            }),
            args: Vec::new(),
            ty: unit_ty(),
        }),
    }]);
    let (_, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        locals.iter().any(|l| l.mutable),
        "new alloc must appear as mutable temp local in convert_block's locals"
    );
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_let_creates_local_and_let_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Let {
        id: LocalId::from_raw(0),
        name: Atom::new_inline("11"),
        ty: unit_ty(),
        init: Some(int_lit(5)),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert_eq!(mir_block.len(), 1);
    assert_eq!(locals.len(), 1);
    assert_eq!(locals[0].name, Atom::new_inline("11"));
}

#[test]
fn convert_block_expr_emits_expr_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Expr { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Expr(_)));
}

#[test]
fn convert_block_return_emits_return() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(int_lit(0)),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Return(_)));
}

#[test]
fn convert_block_if_emits_if_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::If {
        cond: HirExpr::Bool(true),
        then: Box::new(HirStmt::Expr { expr: int_lit(1) }),
        otherwise: None,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::If { .. }));
}

#[test]
fn convert_function_nested_let_in_if_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::Let {
                id: LocalId::from_raw(7),
                name: Atom::new_inline("99"),
                ty: unit_ty(),
                init: Some(int_lit(1)),
            }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.body.locals.len(),
        1,
        "nested let must surface in body.locals"
    );
    assert_eq!(mir.body.locals[0].name, Atom::new_inline("99"));
}

#[test]
fn convert_function_nested_let_in_while_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::While {
            cond: HirExpr::Bool(true),
            body: Box::new(HirStmt::Let {
                id: LocalId::from_raw(11),
                name: Atom::new_inline("33"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert!(
        names.contains(&"33".to_owned()),
        "while-body let must surface in body.locals (got {names:?})"
    );
}

#[test]
fn convert_function_nested_let_in_forof_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::ForOf {
            binding: LocalId::from_raw(20),
            iter: int_lit(0),
            body: Box::new(HirStmt::Let {
                id: LocalId::from_raw(21),
                name: Atom::new_inline("77"),
                ty: unit_ty(),
                init: Some(int_lit(0)),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let names: Vec<String> = mir
        .body
        .locals
        .iter()
        .map(|l| l.name.as_str().to_owned())
        .collect();
    assert_eq!(mir.body.locals.len(), 2, "for-of binding + nested let");
    assert!(
        names.contains(&"for_of_binding".to_owned()),
        "for-of binding synth name"
    );
    assert!(names.contains(&"77".to_owned()), "nested let name");
}

#[test]
fn convert_block_while_emits_while() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(true),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::While { .. }));
}

#[test]
fn convert_block_while_cond_with_side_effects_keeps_cond_as_loop_condition() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let MirStmt::While { cond, body } = &mir_block.stmts[1] else {
        panic!(
            "expected MirStmt::While at index 1, got {:?}",
            mir_block.stmts[1]
        );
    };
    assert!(
        matches!(*cond, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0)),
        "MirStmt::While.cond must be the real cond expression (not Bool(true) forever-loop), got {:?}",
        cond
    );
    let inner_while_body = match &body.stmts[0] {
        MirStmt::While { body: inner, .. } => &inner.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_while_body
            .iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Int { value: 0, .. }))),
        "original body stmts must remain in inner-while body, got {:?}",
        inner_while_body
    );
}

#[test]
fn convert_block_while_false_does_not_loop_forever() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(false),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let MirStmt::While { cond, .. } = &mir_block.stmts[1] else {
        panic!("expected MirStmt::While at index 1");
    };
    assert!(matches!(*cond, MirExpr::Bool(false)));
    assert!(!matches!(*cond, MirExpr::Bool(true)));
}

#[test]

fn convert_block_while_continue_re_evaluates_cond_via_inner_wrapper() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Template {
        tag: None,
        expressions: vec![],
        cooked_parts: vec![None],
        raw_parts: vec![None],
        ty: unit_ty(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Continue { label: None }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while_idx = mir_block
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected outer MirStmt::While");
    let outer_while = match &mir_block.stmts[outer_while_idx] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected MirStmt::While, got {other:?}"),
    };
    let inner_while = match &outer_while.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_while.iter().any(|s| matches!(s, MirStmt::Break)),
        "user's Continue must be rewritten to MirStmt::Break targeting the inner wrapper, got {:?}",
        inner_while
    );
    let cond_let_idx = outer_while
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::Let { .. }))
        .expect("cond Let (1-part template) must be present in outer-while body");
    let inner_while_idx_in_outer = 0;
    assert!(
        cond_let_idx > inner_while_idx_in_outer,
        "cond Let (idx {}) must appear AFTER the inner-while wrapper (idx {}) so cond re-evaluates each iteration (1-part template emits Let, not Runtime); got stmts {:?}",
        cond_let_idx,
        inner_while_idx_in_outer,
        outer_while.stmts
    );
}

#[test]
fn convert_block_while_break_breaks_outer_via_sentinel() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Bool(true),
        body: Box::new(HirStmt::Break { label: None }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while = match &mir_block.stmts[1] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected MirStmt::While at index 1, got {other:?}"),
    };
    let inner_while = match &outer_while.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    let has_assign_then_break = inner_while.windows(2).any(|w| {
        matches!(
            w[0],
            MirStmt::Assign {
                target: MirPlace::Local { .. },
                value: MirExpr::Bool(true),
            }
        ) && matches!(w[1], MirStmt::Break)
    });
    assert!(
        has_assign_then_break,
        "user's Break must be rewritten to is_break=true; Break targeting the inner wrapper, got {:?}",
        inner_while
    );
}

#[test]
fn convert_block_dowhile_executes_body_at_least_once() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Bool(false),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let body_stmts = match &mir_block.stmts[2] {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 2, got {other:?}"),
    };
    let inner_while_body = match &body_stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner While, got {other:?}"),
    };
    assert!(
        inner_while_body
            .iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Int { value: 0, .. }))),
        "body stmts must end up in inner-while, got {:?}",
        inner_while_body
    );
}

#[test]
fn convert_block_dowhile_continue_still_evaluates_cond() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Continue { label: None }),
        cond: HirExpr::Bool(false),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let while_stmt = &mir_block.stmts[2];
    let while_body = match while_stmt {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected While at index 2, got {other:?}"),
    };
    let inner_while_body = match &while_body[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner While, got {other:?}"),
    };
    assert!(
        inner_while_body.iter().any(|s| matches!(s, MirStmt::Break)),
        "user's Continue must be rewritten to Break targeting the inner wrapper, got {:?}",
        inner_while_body
    );
    let cond = match while_stmt {
        MirStmt::While { cond, .. } => cond,
        _ => unreachable!(),
    };
    assert!(
        matches!(
            cond,
            MirExpr::Binary {
                op: BinaryOp::Or,
                ..
            }
        ),
        "while cond must be `__first || cond`, got {cond:?}"
    );
}

#[test]

fn convert_block_while_call_cond_evaluated_once_per_iteration() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Call {
            callee: HirCallee::Function(FunctionId::from_raw(0)),
            args: Vec::new(),
            ty: unit_ty(),
        },
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    let outer_while = mir_block
        .stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::While { cond, .. } => Some(cond),
            _ => None,
        })
        .expect("expected outer MirStmt::While");
    assert!(
        matches!(*outer_while, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0)),
        "While.cond must hold the original Call (re-evaluated each iter by the header itself), got {outer_while:?}"
    );
    let outer_while_body = match mir_block.stmts.last().expect("non-empty") {
        MirStmt::While { body, .. } => &body.stmts,
        other => panic!("expected MirStmt::While, got {other:?}"),
    };
    let contains_not_call_break = outer_while_body.iter().any(|s| {
        matches!(
            s,
            MirStmt::If {
                cond: MirExpr::Unary {
                    op: UnaryOp::Not,
                    expr,
                    ..
                },
                ..
            } if matches!(**expr, MirExpr::Call { callee, .. } if callee == FunctionId::from_raw(0))
        )
    });
    assert!(
        !contains_not_call_break,
        "loop body must NOT contain `if !Call break` (would call the function a second time per iter); got {:?}",
        outer_while_body
    );
}

#[test]
fn convert_block_dowhile_false_runs_body_exactly_once_not_infinite() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Bool(false),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Let { .. }));
    assert!(matches!(mir_block.stmts[1], MirStmt::Let { .. }));
    let outer_while = match &mir_block.stmts[2] {
        MirStmt::While { cond, body } => (cond, body),
        other => panic!("expected MirStmt::While at index 2, got {other:?}"),
    };
    let first_id_local = match outer_while.0 {
        MirExpr::Binary {
            op: BinaryOp::Or,
            left,
            ..
        } => match left.as_ref() {
            MirExpr::Local(id) => *id,
            other => panic!("expected first_id Local, got {other:?}"),
        },
        other => panic!("expected first_id || cond_mir, got {other:?}"),
    };
    let inner_while = match &outer_while.1.stmts[0] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    let first_id_reset = inner_while.iter().any(|s| {
        matches!(
            s,
            MirStmt::Assign {
                target: MirPlace::Local { id },
                value: MirExpr::Bool(false),
            } if *id == first_id_local
        )
    });
    assert!(
        first_id_reset,
        "first_id must be reset to false inside the inner wrapper so the next iter's outer-while entry checks cond_mir (and `do {{}} while (false)` doesn't infinite-loop), got inner stmts {:?}",
        inner_while
    );
}

#[test]
fn convert_block_forof_emits_forof() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::ForOf {
        binding: LocalId::from_raw(0),
        iter: int_lit(0),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::ForOf { .. }));
    assert_eq!(locals.len(), 1);
}

#[test]
fn convert_block_forin_emits_forin_not_forof() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::ForIn {
        binding: LocalId::from_raw(0),
        iter: int_lit(0),
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, locals) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        matches!(mir_block.stmts[0], MirStmt::ForIn { .. }),
        "HirStmt::ForIn must lower to MirStmt::ForIn (got {:?})",
        mir_block.stmts[0]
    );
    assert!(!matches!(mir_block.stmts[0], MirStmt::ForOf { .. }));
    assert_eq!(locals.len(), 1);
}

#[test]
fn convert_block_break_continue_pass_through() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![
        HirStmt::Break { label: None },
        HirStmt::Continue { label: None },
    ]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Break));
    assert!(matches!(mir_block.stmts[1], MirStmt::Continue));
}

#[test]
fn convert_block_throw_emits_throw() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Throw { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Throw { .. }));
}

#[test]
fn convert_block_switch_emits_switch_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![
            ts_aot_ir_hir::HirSwitchCase::new(Some(HirExpr::Int(1)), vec![HirStmt::ret(None)]),
            ts_aot_ir_hir::HirSwitchCase::new(None, vec![HirStmt::ret(None)]),
        ],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(matches!(mir_block.stmts[0], MirStmt::Switch { .. }));
    if let MirStmt::Switch {
        disc,
        cases,
        default,
    } = &mir_block.stmts[0]
    {
        assert!(matches!(disc.as_ref(), MirExpr::Int { .. }));
        assert_eq!(cases.len(), 1);
        assert!(matches!(cases[0].value, ConstValue::Int(1)));
        assert!(default.is_some());
    } else {
        panic!("expected MirStmt::Switch");
    }
}

#[test]
fn convert_block_switch_non_terminating_case_inserts_implicit_break() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::expr(int_lit(0))],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0005"),
        "non-terminating case must emit a fall-through P0005 warning"
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let last_stmt = cases[0]
        .body
        .stmts
        .last()
        .expect("case body must have at least one stmt");
    assert!(
        matches!(last_stmt, MirStmt::Break),
        "non-terminating case body must end with implicit MirStmt::Break, got {last_stmt:?}"
    );
}

#[test]
fn convert_block_switch_terminating_case_does_not_insert_break() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(1)),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0005"),
        "terminating case must not emit P0005 warning"
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let last_stmt = cases[0].body.stmts.last().expect("case body");
    assert!(
        matches!(last_stmt, MirStmt::Return(_)),
        "terminating case must keep its terminator, not get an extra Break, got {last_stmt:?}"
    );
}

#[test]
fn convert_block_switch_case_preserves_full_i128_int_value() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Int(7)),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    let ConstValue::Int(stored) = &cases[0].value else {
        panic!("expected ConstValue::Int");
    };
    assert_eq!(*stored, i128::from(7));
    assert!(
        !cx.diagnostics()
            .iter()
            .any(|d| d.message.contains("does not fit in i64")),
        "ConstValue::Int(i128) storage must not emit i64-overflow fallback diagnostic anymore"
    );
}

#[test]
fn convert_block_switch_non_const_case_value_emits_p0006_error() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: vec![ts_aot_ir_hir::HirSwitchCase::new(
            Some(HirExpr::Binary {
                op: ts_aot_ir_hir::HirBinaryOp::Add,
                lhs: Box::new(HirExpr::Int(1)),
                rhs: Box::new(HirExpr::Int(2)),
                ty: TypeId::from_raw(0),
            }),
            vec![HirStmt::ret(None)],
        )],
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(
        cx.has_errors(),
        "non-const case value (Binary expression) must emit a hard error, not a warning, \
         so compilation fails instead of silently dropping the case"
    );
    let p0006 = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0006")
        .expect("expected P0006 diagnostic for non-const case value");
    assert!(
        p0006.message.contains("switch case"),
        "P0006 message must clearly identify switch-case context, got: {}",
        p0006.message
    );
    let MirStmt::Switch { cases, .. } = &mir_block.stmts[0] else {
        panic!("expected MirStmt::Switch");
    };
    assert!(
        cases.is_empty(),
        "non-const case value must be skipped (continue), not pushed as a malformed SwitchCase, got {} cases",
        cases.len()
    );
}

#[test]
fn convert_block_try_emits_try_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Try {
        body: Box::new(HirStmt::ret(None)),
        catch: Some(ts_aot_ir_hir::HirCatchClause::new(
            None,
            Box::new(HirStmt::ret(None)),
        )),
        finally: None,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    assert!(matches!(mir_block.stmts[0], MirStmt::Try { .. }));
    if let MirStmt::Try {
        body,
        catch_param,
        catch,
        finally,
    } = &mir_block.stmts[0]
    {
        assert_eq!(body.stmts.len(), 1);
        assert!(catch_param.is_none());
        assert!(catch.is_some());
        assert_eq!(catch.as_ref().unwrap().stmts.len(), 1);
        assert!(finally.is_none());
    } else {
        panic!("expected MirStmt::Try");
    }
}

#[test]
fn convert_block_try_finally_without_catch_emits_optional_catch_none() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Try {
        body: Box::new(HirStmt::ret(None)),
        catch: None,
        finally: Some(Box::new(HirStmt::ret(None))),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());
    let MirStmt::Try {
        body,
        catch,
        catch_param,
        finally,
    } = &mir_block.stmts[0]
    else {
        panic!("expected MirStmt::Try");
    };
    assert_eq!(body.stmts.len(), 1);
    assert!(
        catch.is_none(),
        "try-finally without catch clause must preserve `catch: None`, not encode as empty MirBlock. got: {catch:?}"
    );
    assert!(catch_param.is_none());
    assert!(finally.is_some());
}

#[test]
fn convert_function_basic_shape() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![HirParam {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: true,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        Some("f".to_owned()),
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(mir.id, FunctionId::from_raw(0));
    assert_eq!(mir.params.len(), 1);
    assert!(!mir.effects.is_async);
}

#[test]
fn convert_function_let_after_params_gets_fresh_id() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![
            HirParam {
                name: Atom::new_inline("10"),
                ty: unit_ty(),
            },
            HirParam {
                name: Atom::new_inline("11"),
                ty: unit_ty(),
            },
        ],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Let {
            id: LocalId::from_raw(5),
            name: Atom::new_inline("99"),
            ty: unit_ty(),
            init: Some(int_lit(0)),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(mir.params.len(), 2);
    assert_eq!(mir.body.locals.len(), 1);
    let let_id = mir.body.locals[0].id;
    assert_ne!(let_id, mir.params[0].id);
    assert_ne!(let_id, mir.params[1].id);
    assert!(
        let_id.raw() >= mir.params.len() as u32,
        "let id {} should be >= params len {}",
        let_id.raw(),
        mir.params.len()
    );
}

#[test]
fn convert_function_marks_async_effect() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: Vec::new(),
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(mir.effects.is_async);
}

#[test]
fn convert_function_body_references_param_id_resolves_to_param() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![HirParam {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let param_id = mir.params[0].id;
    let referenced = match &mir.body.block.stmts[0] {
        MirStmt::Expr(MirExpr::Local(lid)) => *lid,
        other => panic!("expected Expr(Local), got {other:?}"),
    };
    assert_eq!(
        referenced, param_id,
        "HIR LocalId(0) in body must resolve to the MIR param id, not a fresh local"
    );
    assert!(
        mir.body.locals.is_empty(),
        "no extra locals should be allocated for the param reference itself"
    );
}

#[test]
fn convert_program_empty_keeps_module() {
    let hir = empty_hir();
    let mut cx = ctx();
    let mir = convert_program(&hir, &mut empty_types(), &mut cx);
    assert_eq!(mir.module, hir.module);
    assert_eq!(mir.decl_count(), 0);
}

#[test]
fn convert_program_assigns_distinct_function_ids() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    for i in 0..3 {
        prog.push_decl(HirDecl::Function(HirFunction {
            name: Atom::from(format!("fn{}", i)),
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
    }
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let functions: Vec<_> = mir.functions().collect();
    assert_eq!(functions.len(), 3);
    let ids: std::collections::HashSet<_> = functions.iter().map(|f| f.id).collect();
    assert_eq!(
        ids.len(),
        3,
        "FunctionIds must be distinct across top-level decls"
    );
}

#[test]
fn convert_program_resolves_indirect_global_callee_to_function_id() {
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("callee"),
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
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("caller"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::new_inline("callee"),
                    ty: unit_ty(),
                })),
                args: Vec::new(),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let p0005: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "P0005")
        .collect();
    assert!(
        p0005.is_empty(),
        "expected no P0005 (indirect callee) errors, got {}: {:?}",
        p0005.len(),
        p0005
    );
    let caller = mir
        .functions()
        .find(|f| f.name == Atom::new_inline("caller"))
        .expect("caller function present in MIR");
    let stmt = caller
        .body
        .block
        .stmts
        .first()
        .expect("caller has at least one stmt");
    let MirStmt::Expr(MirExpr::Call { callee, .. }) = stmt else {
        panic!("expected MirStmt::Expr(MirExpr::Call), got {stmt:?}");
    };
    assert_eq!(
        *callee,
        FunctionId::from_raw(0),
        "caller's call to global 'callee' must resolve to FunctionId::from_raw(0); got {callee:?}"
    );
}

#[test]
fn convert_program_assigns_distinct_struct_ids() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    for i in 0..2 {
        prog.push_decl(HirDecl::Class(HirClass {
            name: Atom::from(format!("cls{}", i)),
            ty: TypeId::from_raw(100 + i),
            fields: vec![HirField {
                name: Atom::from(format!("f{}", i)),
                ty: unit_ty(),
            }],
            methods: Vec::new(),
            extends: None,
            type_params: Vec::new(),
        }));
    }
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let structs: Vec<_> = mir.structs().collect();
    assert_eq!(structs.len(), 2);
    let ids: std::collections::HashSet<_> = structs.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 2, "StructIds must be distinct across classes");
}

#[test]
fn convert_program_struct_id_consistent_across_functions_for_same_type() {
    let shared_ty = TypeId::from_raw(99);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    let make_fn = |name: u32, ty: TypeId| HirFunction {
        name: Atom::from(format!("f{}", name)),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::StructLiteral {
                ty,
                fields: Vec::new(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    prog.push_decl(HirDecl::Function(make_fn(1, shared_ty)));
    prog.push_decl(HirDecl::Function(make_fn(2, shared_ty)));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let mut struct_literal_ids: Vec<ts_aot_core::StructId> = Vec::new();
    for func in mir.functions() {
        for s in &func.body.block.stmts {
            let sl = match s {
                MirStmt::Return(Some(MirExpr::StructLiteral { struct_id, .. })) => Some(*struct_id),
                MirStmt::Expr(MirExpr::StructLiteral { struct_id, .. }) => Some(*struct_id),
                _ => None,
            };
            if let Some(id) = sl {
                struct_literal_ids.push(id);
            }
        }
    }
    assert_eq!(
        struct_literal_ids.len(),
        2,
        "expected 2 StructLiteral exprs, got {struct_literal_ids:?}"
    );
    assert_eq!(
        struct_literal_ids[0], struct_literal_ids[1],
        "same HIR TypeId must yield same MIR StructId across functions (got {:?})",
        struct_literal_ids
    );
}

#[test]
fn convert_program_class_methods_use_method_function_kind() {
    use ts_aot_ir_hir::{HirClass, HirField, HirParam};
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("42"),
        ty: TypeId::from_raw(4242),
        fields: Vec::new(),
        methods: vec![HirFunction {
            name: Atom::new_inline("100"),
            params: vec![HirParam {
                name: Atom::new_inline("200"),
                ty: unit_ty(),
            }],
            ret: unit_ty(),
            throws: None,
            body: vec![HirStmt::Return { value: None }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }],
        extends: None,
        type_params: Vec::new(),
    }));
    let _ = HirField {
        name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let expected_owner = struct_decl.id;
    assert_eq!(struct_decl.methods.len(), 1);
    let method = &struct_decl.methods[0];
    let (owner, self_param) = match method.kind {
        FunctionKind::Method { owner, self_param } => (owner, self_param),
        ref other => panic!("expected FunctionKind::Method, got {other:?}"),
    };
    assert_eq!(
        owner, expected_owner,
        "Method.owner must match owning struct"
    );
    assert_eq!(
        self_param, method.params[0].id,
        "Method.self_param must be the first param's LocalId"
    );
}

#[test]
fn convert_program_class_struct_id_shared_with_new_and_struct_literal() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(7777);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("2"),
        params: Vec::new(),
        ret: class_ty,
        throws: None,
        body: vec![
            HirStmt::Expr {
                expr: HirExpr::New {
                    callee: Box::new(HirExpr::Global {
                        name: Atom::new_inline("1"),
                        ty: class_ty,
                    }),
                    args: Vec::new(),
                    ty: class_ty,
                },
            },
            HirStmt::Return {
                value: Some(HirExpr::StructLiteral {
                    ty: class_ty,
                    fields: vec![(FieldId::from_raw(0), int_lit(1))],
                }),
            },
        ],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let mut new_id: Option<ts_aot_core::StructId> = None;
    let mut literal_id: Option<ts_aot_core::StructId> = None;
    let mut new_seen = false;
    for func in mir.functions() {
        for s in &func.body.block.stmts {
            if let MirStmt::Let {
                init: Some(MirExpr::StructLiteral { struct_id, .. }),
                ..
            } = s
                && !new_seen
            {
                new_id = Some(*struct_id);
                new_seen = true;
            }
            if let MirStmt::Return(Some(MirExpr::StructLiteral { struct_id, .. })) = s {
                literal_id = Some(*struct_id);
            }
        }
    }
    let new_id = new_id.expect("expected New expression to lower");
    let literal_id = literal_id.expect("expected StructLiteral expression to lower");
    assert_eq!(
        new_id, class_struct_id,
        "new Foo() must use class's StructId"
    );
    assert_eq!(
        literal_id, class_struct_id,
        "StructLiteral with class TypeId must use class's StructId"
    );
}

#[test]
fn convert_program_class_struct_id_shared_even_when_function_decl_comes_first() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(8888);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("2"),
        params: Vec::new(),
        ret: class_ty,
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("1"),
                    ty: class_ty,
                }),
                args: Vec::new(),
                ty: class_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("10"),
            ty: unit_ty(),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let func = mir.functions().next().expect("expected one function");
    let mut found_new_id: Option<ts_aot_core::StructId> = None;
    for s in &func.body.block.stmts {
        if let MirStmt::Let {
            init: Some(MirExpr::StructLiteral { struct_id, .. }),
            ..
        } = s
        {
            found_new_id = Some(*struct_id);
        }
    }
    let new_id = found_new_id.expect("expected New expression to lower");
    assert_eq!(
        new_id, class_struct_id,
        "new Foo() must use class's StructId even when class decl follows function decl"
    );
}

#[test]
fn body_can_throw_propagates_through_struct_literal_fields() {
    let throwing_call_ty = TypeId::from_raw(0);
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: throwing_call_ty,
    };
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::StructLiteral {
            ty: throwing_call_ty,
            fields: vec![(FieldId::from_raw(0), call)],
        }),
    }];
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "function returning a struct literal whose field calls a throwing callee must be can_throw"
    );
}

#[test]
fn body_can_throw_stays_false_for_plain_struct_literal() {
    let body = vec![HirStmt::Return {
        value: Some(HirExpr::StructLiteral {
            ty: unit_ty(),
            fields: vec![(FieldId::from_raw(0), int_lit(1))],
        }),
    }];
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !mir.effects.can_throw,
        "struct literal with non-throwing fields must not propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_assignment_target() {
    let throwing_call_ty = TypeId::from_raw(0);
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: throwing_call_ty,
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: throwing_call_ty,
    };
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(field_target),
            value: Box::new(int_lit(1)),
            ty: throwing_call_ty,
        },
    }];
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "assignment with throwing call on LHS (e.g. obj().x = 1) must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_assignment_target_index() {
    let throwing_call_ty = TypeId::from_raw(0);
    let arr_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(77)),
        args: Vec::new(),
        ty: throwing_call_ty,
    };
    let idx_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(78)),
        args: Vec::new(),
        ty: throwing_call_ty,
    };
    let index_lhs = HirExpr::Index {
        owner: Box::new(arr_target),
        index: Box::new(idx_target),
        ty: throwing_call_ty,
    };
    let body = vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(index_lhs),
            value: Box::new(int_lit(1)),
            ty: throwing_call_ty,
        },
    }];
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut struct_id_map,
        &mut next_struct_id,
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "assignment with throwing calls in arr()[idx()] LHS must propagate can_throw"
    );
}

#[test]
fn convert_program_preserves_import_module_path_from_atom() {
    use ts_aot_ir_hir::{HirExport, HirImport};
    let module_id = Atom::new_inline("./other");
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.imports.push(HirImport {
        module: module_id,
        name: Atom::new_inline("7"),
        alias: None,
    });
    prog.exports.push(HirExport {
        name: Atom::new_inline("9"),
        alias: None,
    });
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    assert_eq!(mir.imports.len(), 1);
    assert_eq!(mir.imports[0].module, "./other");
    assert_eq!(mir.imports[0].symbol, Atom::new_inline("7"));
    assert_eq!(mir.exports.len(), 1);
    assert_eq!(mir.exports[0].symbol, Atom::new_inline("9"));
}

#[test]
fn convert_function_await_emits_mir_await_expr_without_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Await {
                expr: Box::new(int_lit(1)),
                ty: unit_ty(),
            }),
        }],
        is_async: true,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    match mir.body.block.stmts.last().expect("non-empty body") {
        MirStmt::Return(Some(MirExpr::Await { .. })) => {}
        other => panic!("expected last stmt Return(Some(MirExpr::Await)), got {other:?}"),
    };
    assert!(
        mir.body.locals.is_empty(),
        "await no longer needs a temp local (no state machine), got: {:?}",
        mir.body.locals
    );
}

#[test]
fn convert_function_new_alloc_appears_in_body_locals() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("99"),
                    ty: unit_ty(),
                }),
                args: Vec::new(),
                ty: unit_ty(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let alloc = match mir.body.block.stmts.last().expect("non-empty body") {
        MirStmt::Return(Some(MirExpr::Local(lid))) => *lid,
        other => panic!("expected last stmt Return(Some(Local)), got {other:?}"),
    };
    assert!(
        mir.body.locals.iter().any(|l| l.id == alloc),
        "new alloc {alloc:?} must be in body.locals"
    );
}

#[test]
fn convert_function_temp_locals_drained_only_once() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Await {
                expr: Box::new(int_lit(1)),
                ty: unit_ty(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let local_ids: Vec<u32> = mir.body.locals.iter().map(|l| l.id.raw()).collect();
    let unique: std::collections::HashSet<u32> = local_ids.iter().copied().collect();
    assert_eq!(
        local_ids.len(),
        unique.len(),
        "no duplicate locals (drilled into body.locals)"
    );
}

#[test]
fn convert_function_can_throw_true_when_body_has_throw_stmt() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw { expr: int_lit(0) }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "function containing HirStmt::Throw must surface can_throw=true"
    );
}

#[test]
fn convert_function_can_throw_false_when_body_has_no_throw_stmt() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !mir.effects.can_throw,
        "function without throw must surface can_throw=false"
    );
}

#[test]
fn convert_function_can_throw_recurses_into_nested_blocks() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: HirExpr::Bool(true),
            then: Box::new(HirStmt::Throw { expr: int_lit(0) }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "nested throw inside If must propagate to can_throw"
    );
}

#[test]
fn convert_function_build_params_preserves_param_atom_name() {
    use ts_aot_ir_hir::HirParam;
    let sentinel_symbol = Atom::new_inline("__sentinel__");
    let first_id = Atom::new_inline("first");
    let second_id = Atom::new_inline("second");
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: vec![
            HirParam {
                name: first_id.clone(),
                ty: unit_ty(),
            },
            HirParam {
                name: second_id.clone(),
                ty: unit_ty(),
            },
        ],
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let first_name = mir.params[0].name.clone();
    let second_name = mir.params[1].name.clone();
    assert_ne!(
        first_name, second_name,
        "distinct param names must yield distinct Atoms"
    );
    assert_ne!(
        first_name, sentinel_symbol,
        "MirParam.name must be the source Atom (not coincidentally equal to a pre-existing entry); got {:?}",
        first_name
    );
    assert_eq!(
        first_name.as_str(),
        first_id.as_str(),
        "MirParam.name must equal the source Atom (content-equivalent Atom); got {:?} vs {}",
        first_name,
        first_id
    );
}

#[test]
fn convert_function_with_remap_uses_remap_only_for_call_sites() {
    let f = HirFunction {
        name: Atom::new_inline("7"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: Vec::new(),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut remap = HashMap::new();
    remap.insert(FunctionId::from_raw(0), FunctionId::from_raw(42));
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(5),
        None,
        remap,
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.id,
        FunctionId::from_raw(5),
        "declaration id is the caller-provided value, not remapped"
    );
    let call_callee = match &mir.body.block.stmts[0] {
        MirStmt::Expr(MirExpr::Call { callee, .. }) => *callee,
        other => panic!("expected Call, got {other:?}"),
    };
    assert_eq!(
        call_callee,
        FunctionId::from_raw(42),
        "call site remapped via function_remap"
    );
}

#[test]
fn convert_binop_maps_all_variants() {
    use super::ops::convert_binop;
    let mut cx = ctx();
    assert_eq!(convert_binop(HirBinaryOp::Add, &mut cx), BinaryOp::Add);
    assert_eq!(convert_binop(HirBinaryOp::Sub, &mut cx), BinaryOp::Sub);
    assert_eq!(convert_binop(HirBinaryOp::Mul, &mut cx), BinaryOp::Mul);
    assert_eq!(convert_binop(HirBinaryOp::Div, &mut cx), BinaryOp::Div);
    assert_eq!(convert_binop(HirBinaryOp::Mod, &mut cx), BinaryOp::Mod);
    assert_eq!(convert_binop(HirBinaryOp::Eq, &mut cx), BinaryOp::Eq);
    assert_eq!(convert_binop(HirBinaryOp::Ne, &mut cx), BinaryOp::Ne);
    assert_eq!(convert_binop(HirBinaryOp::Lt, &mut cx), BinaryOp::Lt);
    assert_eq!(convert_binop(HirBinaryOp::Le, &mut cx), BinaryOp::Le);
    assert_eq!(convert_binop(HirBinaryOp::Gt, &mut cx), BinaryOp::Gt);
    assert_eq!(convert_binop(HirBinaryOp::Ge, &mut cx), BinaryOp::Ge);
    assert_eq!(convert_binop(HirBinaryOp::And, &mut cx), BinaryOp::And);
    assert_eq!(convert_binop(HirBinaryOp::Or, &mut cx), BinaryOp::Or);
    assert_eq!(
        convert_binop(HirBinaryOp::BitAnd, &mut cx),
        BinaryOp::BitAnd
    );
    assert_eq!(convert_binop(HirBinaryOp::BitOr, &mut cx), BinaryOp::BitOr);
    assert_eq!(
        convert_binop(HirBinaryOp::BitXor, &mut cx),
        BinaryOp::BitXor
    );
    assert_eq!(convert_binop(HirBinaryOp::Shl, &mut cx), BinaryOp::Shl);
    assert_eq!(convert_binop(HirBinaryOp::Shr, &mut cx), BinaryOp::Shr);
    assert_eq!(convert_binop(HirBinaryOp::Usr, &mut cx), BinaryOp::Eq);
    assert_eq!(convert_binop(HirBinaryOp::In, &mut cx), BinaryOp::Eq);
    assert_eq!(
        convert_binop(HirBinaryOp::InstanceOf, &mut cx),
        BinaryOp::Eq
    );
    assert!(
        cx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "P0005" && d.message.contains("Usr")),
        "Usr/In/InstanceOf must emit a P0005 diagnostic from convert_binop"
    );
}

#[test]
fn convert_binop_unsupported_variants_emit_diagnostic_at_call_site() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Binary {
        op: HirBinaryOp::Usr,
        lhs: Box::new(int_lit(1)),
        rhs: Box::new(int_lit(2)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 for unsupported binary op");
    assert!(diag.message.contains("Usr"));
}

#[test]
fn convert_unaryop_maps_variants() {
    use super::ops::convert_unaryop;
    let mut cx = ctx();
    assert_eq!(convert_unaryop(HirUnaryOp::Neg, &mut cx), UnaryOp::Neg);
    assert_eq!(convert_unaryop(HirUnaryOp::Not, &mut cx), UnaryOp::Not);
    assert_eq!(
        convert_unaryop(HirUnaryOp::BitNot, &mut cx),
        UnaryOp::BitNot
    );
    assert_eq!(convert_unaryop(HirUnaryOp::TypeOf, &mut cx), UnaryOp::Not);
    assert_eq!(convert_unaryop(HirUnaryOp::Void, &mut cx), UnaryOp::Not);
    assert_eq!(convert_unaryop(HirUnaryOp::Delete, &mut cx), UnaryOp::Not);
    assert!(
        cx.diagnostics()
            .iter()
            .any(|d| d.code.as_str() == "P0005" && d.message.contains("TypeOf")),
        "TypeOf/Void/Delete must emit a P0005 diagnostic from convert_unaryop"
    );
}

#[test]
fn convert_expr_typeof_lowers_to_mir_typeof_without_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Unary {
        op: HirUnaryOp::TypeOf,
        expr: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "PR 1.6: TypeOf is now a real op (not NotYetImplemented), got {:?}",
        cx.diagnostics()
    );
    assert!(
        matches!(mir, MirExpr::TypeOf { .. }),
        "TypeOf must lower to MirExpr::TypeOf, got {mir:?}"
    );
}

#[test]
fn convert_program_class_method_with_no_params_is_skipped() {
    use ts_aot_ir_hir::HirClass;
    let class_ty = TypeId::from_raw(5555);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("1"),
        ty: class_ty,
        fields: Vec::new(),
        methods: vec![HirFunction {
            name: Atom::new_inline("100"),
            params: Vec::new(),
            ret: unit_ty(),
            throws: None,
            body: vec![HirStmt::Return { value: None }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }],
        extends: None,
        type_params: Vec::new(),
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    assert!(
        struct_decl.methods.is_empty(),
        "method without receiver parameter must be dropped from the struct, not converted to Method {{ self_param: LocalId(0) }}"
    );
}

#[test]
fn convert_program_exported_function_uses_atom_name_as_export_name() {
    let name_id = Atom::new_inline("render");
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: name_id,
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: true,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    assert_eq!(
        func.export_name.as_deref(),
        Some("render"),
        "export_name must come from the function name (Atom), not FunctionId"
    );
}

#[test]
fn convert_expr_new_lowers_callee_for_side_effects() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut struct_id_map: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let global_ty = TypeId::from_raw(0);
    let callee_fn_id = FunctionId::from_raw(99);
    let expr = HirExpr::New {
        callee: Box::new(HirExpr::Call {
            callee: HirCallee::Function(callee_fn_id),
            args: Vec::new(),
            ty: global_ty,
        }),
        args: Vec::new(),
        ty: global_ty,
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut struct_id_map,
        &mut next_struct_id,
        &mut empty_types(),
        &mut cx,
    );
    let call_callees: Vec<FunctionId> = out
        .iter()
        .filter_map(|s| match s {
            MirStmt::Expr(MirExpr::Call { callee, .. }) => Some(*callee),
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ..
            } => Some(*callee),
            _ => None,
        })
        .collect();
    assert!(
        call_callees.contains(&callee_fn_id),
        "callee's factory call must appear in output before placeholder ctor, got {call_callees:?}"
    );
    assert!(
        call_callees.contains(&PLACEHOLDER_FUNCTION),
        "placeholder ctor call must still appear, got {call_callees:?}"
    );
}

#[test]
fn convert_expr_assignment_to_field_with_call_base_materializes_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(7),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(42)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Call-owner field access with no struct id registered must surface P0012 (missing struct id); got {:?}",
        cx.diagnostics()
    );
    let has_let_for_call = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    assert!(
        has_let_for_call,
        "Call base must be materialized into a temp local via MirStmt::Let"
    );
    let has_assign_to_field = out.iter().any(|s| {
        matches!(
            s,
            MirStmt::Assign {
                target: MirPlace::Field { .. },
                ..
            }
        )
    });
    assert!(
        has_assign_to_field,
        "Field assignment must follow the materialized temp local"
    );
}

#[test]
fn convert_expr_assignment_to_field_with_call_base_keeps_call_in_order() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let let_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    let assign_idx = out.iter().position(|s| matches!(s, MirStmt::Assign { .. }));
    let (Some(li), Some(ai)) = (let_idx, assign_idx) else {
        panic!("expected both materialize-Let and Assign stmts, got {out:?}");
    };
    assert!(
        li < ai,
        "materialize-Let for call base must precede Field Assign, got let@{li}, assign@{ai}"
    );
}

#[test]
fn convert_expr_assignment_lhs_base_materializes_before_rhs_side_effects() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let call_target = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let field_target = HirExpr::Field {
        owner: Box::new(call_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let value_expr = HirExpr::Template {
        tag: None,
        expressions: vec![rhs_call],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(value_expr),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let materialize_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Call { .. }),
                ..
            }
        )
    });
    let rhs_runtime_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                ..
            }
        )
    });
    let (Some(mi), Some(ri)) = (materialize_idx, rhs_runtime_idx) else {
        panic!("expected both materialize-Let and Runtime stmts, got {out:?}");
    };
    assert!(
        mi < ri,
        "LHS base materialize (obj()) must precede RHS side effects (template Runtime); got materialize@{mi}, rhs@{ri}"
    );
}

#[test]
fn span_does_not_block_compile() {
    let _ = Span::new(0, 0);
}

#[test]
fn body_can_throw_propagates_through_if_condition_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: call,
            then: Box::new(HirStmt::Return { value: None }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "If with throwing cond must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_ternary_branches() {
    let throwing_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(99)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(true)),
                then_branch: Box::new(throwing_call),
                else_branch: Box::new(HirExpr::Int(0)),
                ty: unit_ty(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Ternary with throwing then_branch must propagate can_throw (without this arm, function is mis-analyzed as Plain)"
    );
}

#[test]
fn ternary_preserves_short_circuit_branches_not_in_outer_block() {
    let side_effect_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(false)),
                then_branch: Box::new(side_effect_call),
                else_branch: Box::new(HirExpr::Int(0)),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let outer_stmts = &mir.body.block.stmts;
    let outer_has_call_directly = outer_stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
        )
    });
    assert!(
        !outer_has_call_directly,
        "BUG: then_branch side effect was emitted in the outer block (no short-circuit); outer_stmts={outer_stmts:?}"
    );
    let if_idx = outer_stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::If {
                    cond: MirExpr::Bool(false),
                    ..
                }
            )
        })
        .expect("expected MirStmt::If for the Ternary (cond = false)");
    let MirStmt::If {
        then_block,
        else_block,
        ..
    } = &outer_stmts[if_idx]
    else {
        unreachable!()
    };
    let then_has_call = then_block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
                | MirStmt::Assign {
                    value: MirExpr::Call { .. },
                    ..
                }
        )
    });
    assert!(
        then_has_call,
        "then_branch side effect must live inside then_block"
    );
    let else_block = else_block
        .as_ref()
        .expect("Ternary must produce an else block");
    let else_has_call = else_block.stmts.iter().any(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { .. })
                | MirStmt::Let {
                    init: Some(MirExpr::Call { .. }),
                    ..
                }
                | MirStmt::Assign {
                    value: MirExpr::Call { .. },
                    ..
                }
        )
    });
    assert!(
        !else_has_call,
        "else_branch must not contain then_branch call"
    );
}

#[test]
fn sequence_preserves_side_effects_of_intermediate_expressions() {
    let first_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let second_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(8)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Sequence {
                exprs: vec![first_call, second_call],
                ty: unit_ty(),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    let stmts = &mir.body.block.stmts;
    let first_call_pos = stmts.iter().position(|s| {
        matches!(
            s,
            MirStmt::Expr(MirExpr::Call { callee, .. }) if *callee == FunctionId::from_raw(7)
        )
    });
    assert!(
        first_call_pos.is_some(),
        "BUG: intermediate Call in Sequence is dropped; must be emitted as MirStmt::Expr before the return; got stmts={stmts:?}"
    );
    let MirStmt::Return(ret_value) = stmts.last().expect("expected trailing Return") else {
        panic!("expected trailing Return, got {stmts:?}");
    };
    let ret_call = ret_value
        .as_ref()
        .expect("return must carry the last sequence element value");
    assert!(
        matches!(ret_call, MirExpr::Call { callee, .. } if *callee == FunctionId::from_raw(8)),
        "Return must carry the LAST sequence element (call to fn #8), got {ret_call:?}"
    );
}

#[test]
fn body_can_throw_propagates_through_while_condition_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::While {
            cond: call,
            body: Box::new(HirStmt::Return { value: None }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "While with throwing cond must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_for_of_iter_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::ForOf {
            binding: LocalId::from_raw(0),
            iter: call,
            body: Box::new(HirStmt::Return { value: None }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "ForOf with throwing iter must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_switch_discriminant_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Switch {
            disc: call,
            cases: vec![HirSwitchCase::new(
                Some(int_lit(1)),
                vec![HirStmt::Return { value: None }],
            )],
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Switch with throwing discriminant must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_catch_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: Some(ts_aot_ir_hir::HirCatchClause::new(
                None,
                Box::new(HirStmt::Expr { expr: call }),
            )),
            finally: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Try with throwing catch body must propagate can_throw"
    );
}

#[test]
fn body_can_throw_propagates_through_finally_call() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Try {
            body: Box::new(HirStmt::Return { value: None }),
            catch: None,
            finally: Some(Box::new(HirStmt::Expr { expr: call })),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Try with throwing finally body must propagate can_throw"
    );
}

#[test]
fn body_can_throw_await_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Await {
                expr: Box::new(int_lit(0)),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Await must be considered throwing (rejection)"
    );
}

#[test]
fn body_can_throw_new_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::New {
                callee: Box::new(HirExpr::Global {
                    name: Atom::new_inline("Ctor"),
                    ty: unit_ty(),
                }),
                args: Vec::new(),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "New (constructor invocation) must be considered throwing"
    );
}

#[test]
fn body_can_throw_yield_alone_is_throwing() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Yield {
                expr: Some(Box::new(int_lit(0))),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: true,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "Yield must be considered throwing (delegated generator may throw)"
    );
}

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
        init: Some(HirExpr::String(Atom::new_inline("hi"))),
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
        cx.diagnostics()
            .iter()
            .any(|d| d.message.contains("constant")),
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

#[test]
fn infer_throws_is_none_for_call_only_function() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Function(FunctionId::from_raw(0)),
                args: Vec::new(),
                ty: unit_ty(),
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "function with a Call expr must surface can_throw=true (call may throw at runtime)"
    );
    assert!(
        mir.throws.is_none(),
        "function without a Throw statement must NOT be a throwing function; got throws={:?}",
        mir.throws
    );
}

#[test]
fn infer_throws_is_none_for_if_with_throwing_cond_only() {
    let call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::If {
            cond: call,
            then: Box::new(HirStmt::Return { value: None }),
            otherwise: None,
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(mir.effects.can_throw);
    assert!(
        mir.throws.is_none(),
        "If with throwing cond (no Throw) must NOT be a throwing function"
    );
}

#[test]
fn infer_throws_uses_real_source_when_throwing_typed_expr() {
    let custom_err_ty = TypeId::from_raw(99);
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: custom_err_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(custom_err_ty),
        "throws must be derived from the thrown expression's type, not a sentinel"
    );
}

#[test]
fn infer_throws_uses_ternary_ty_not_sentinel() {
    let custom_err_ty = TypeId::from_raw(77);
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw {
            expr: HirExpr::Ternary {
                cond: Box::new(HirExpr::Bool(true)),
                then_branch: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: custom_err_ty,
                }),
                else_branch: Box::new(HirExpr::Int(0)),
                ty: custom_err_ty,
            },
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(custom_err_ty),
        "throw_expr_type must use the Ternary's `ty` (real type), not the TypeId::from_raw(0) sentinel"
    );
}

#[test]
fn infer_throws_respects_declared_over_inferred() {
    let declared_ty = TypeId::from_raw(7);
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: Some(declared_ty),
        body: vec![HirStmt::Return { value: None }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(declared_ty),
        "declared throws must win over inferred (TS spec: explicit annotation wins)"
    );
}

#[test]
fn infer_throws_uses_sentinel_for_primitive_thrown_expr() {
    let f = HirFunction {
        name: Atom::new_inline("1"),
        params: Vec::new(),
        ret: unit_ty(),
        throws: None,
        body: vec![HirStmt::Throw { expr: int_lit(0) }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    };
    let mut cx = ctx();
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &std::sync::Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(TypeId::from_raw(0)),
        "primitive throw (no real source type) must fall back to TypeId(0) sentinel"
    );
}

#[test]
fn convert_program_resolves_field_id_for_non_first_field() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(7777);
    let field_a_ty = TypeId::from_raw(8888);
    let field_b_ty = TypeId::from_raw(8889);
    let field_c_ty = TypeId::from_raw(8890);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Foo"),
        ty: class_ty,
        fields: vec![
            HirField {
                name: Atom::new_inline("a"),
                ty: field_a_ty,
            },
            HirField {
                name: Atom::new_inline("b"),
                ty: field_b_ty,
            },
            HirField {
                name: Atom::new_inline("c"),
                ty: field_c_ty,
            },
        ],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getB"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: class_ty,
        }],
        ret: field_b_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: class_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("b"),
                ty: field_b_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(1),
        "field `b` must resolve to its post-flatten index in the class, not the placeholder 0"
    );
}

#[test]
fn convert_program_resolves_field_id_after_lower_classes_flatten() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let parent_ty = TypeId::from_raw(100);
    let child_ty = TypeId::from_raw(200);
    let parent_field_ty = TypeId::from_raw(101);
    let child_field_ty = TypeId::from_raw(201);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Parent"),
        ty: parent_ty,
        fields: vec![HirField {
            name: Atom::new_inline("p"),
            ty: parent_field_ty,
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("Child"),
        ty: child_ty,
        fields: vec![HirField {
            name: Atom::new_inline("c"),
            ty: child_field_ty,
        }],
        methods: Vec::new(),
        extends: Some(Atom::new_inline("Parent")),
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getC"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: child_ty,
        }],
        ret: child_field_ty,
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: child_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("c"),
                ty: child_field_ty,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));

    let mut cx = ctx();
    lower_classes(&mut prog, &mut ts_aot_core::TypeTable::new(), &mut cx);
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(1),
        "post-lower_classes, Child's `c` lives at index 1 (after inherited `p`)"
    );
}

#[test]
fn convert_program_resolves_field_id_preserves_placeholder_for_unknown_field() {
    use ts_aot_ir_hir::{HirClass, HirField};
    let class_ty = TypeId::from_raw(300);
    let mut prog = HirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(HirDecl::Class(HirClass {
        name: Atom::new_inline("OnlyA"),
        ty: class_ty,
        fields: vec![HirField {
            name: Atom::new_inline("a"),
            ty: TypeId::from_raw(0),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }));
    prog.push_decl(HirDecl::Function(HirFunction {
        name: Atom::new_inline("getMissing"),
        params: vec![HirParam {
            name: Atom::new_inline("o"),
            ty: class_ty,
        }],
        ret: TypeId::from_raw(0),
        throws: None,
        body: vec![HirStmt::Return {
            value: Some(HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: LocalId::from_raw(0),
                    ty: class_ty,
                }),
                field: FieldId::from_raw(0),
                field_name: Atom::new_inline("missing"),
                ty: TypeId::from_raw(0),
            }),
        }],
        is_async: false,
        is_generator: false,
        is_exported: false,
        type_params: Vec::new(),
        async_info: None,
    }));
    let mut cx = ctx();
    let mir = convert_program(&prog, &mut empty_types(), &mut cx);
    let func = mir.functions().next().expect("expected one function");
    let ret = match &func.body.block.stmts[0] {
        MirStmt::Return(Some(v)) => v,
        other => panic!("expected Return, got {other:?}"),
    };
    let MirExpr::Field { field, .. } = ret else {
        panic!("expected MirExpr::Field, got {ret:?}");
    };
    assert_eq!(
        *field,
        FieldId::from_raw(0),
        "unknown field keeps the placeholder and a diagnostic is emitted, not a wrong resolve"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0010"),
        "P0010 must be reported for an unknown field, diagnostics: {:?}",
        cx.diagnostics()
    );
}

#[test]
fn convert_expr_compound_update_postfix_returns_old_value_via_local() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        2,
        "postfix must emit Let then Assign, got {out:?}"
    );

    let MirStmt::Let {
        local: old_temp,
        init: Some(init),
        ..
    } = &out[0]
    else {
        panic!("expected Let init=load(target), got {:?}", out[0]);
    };
    let init_local = match init {
        MirExpr::Local(id) => *id,
        other => panic!("postfix Let init must be a load of the target local, got {other:?}"),
    };
    assert_eq!(
        init_local,
        LocalId::from_raw(0),
        "postfix Let must capture the target's value before assignment"
    );

    let MirStmt::Assign {
        target: place,
        value,
    } = &out[1]
    else {
        panic!("expected Assign, got {:?}", out[1]);
    };
    let MirExpr::Binary { left, right, .. } = value else {
        panic!("postfix Assign value must be Binary(old + rhs), got {value:?}");
    };
    let MirExpr::Local(left_id) = left.as_ref() else {
        panic!("postfix Binary.left must reuse the old temp, got {left:?}");
    };
    assert_eq!(
        *left_id, *old_temp,
        "postfix Binary.left must reference the old temp captured before the Assign"
    );
    let MirExpr::Int { value: rhs_val, .. } = right.as_ref() else {
        panic!("postfix Binary.right must be rhs MirExpr, got {right:?}");
    };
    assert_eq!(*rhs_val, 1);
    assert!(
        matches!(place, ts_aot_ir_mir::MirPlace::Local { id } if *id == LocalId::from_raw(0)),
        "postfix Assign target must be the original target local, got {place:?}"
    );

    let MirExpr::Local(returned) = mir else {
        panic!("postfix CompoundUpdate must return MirExpr::Local(old_temp), got {mir:?}");
    };
    assert_eq!(
        returned, *old_temp,
        "postfix must return the OLD value, not the new value"
    );
}

#[test]
fn convert_expr_compound_update_prefix_returns_new_value_via_local() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(2)),
        post: false,
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());
    assert_eq!(
        out.len(),
        3,
        "prefix must emit Let(old)=target, Let(new)=Binary(old+rhs), Assign, got {out:?}"
    );

    let MirStmt::Let {
        local: old_temp,
        init: Some(old_init),
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=load(target), got {:?}", out[0]);
    };
    let MirExpr::Local(old_init_id) = old_init else {
        panic!("prefix must load old value via MirExpr::Local(target), got {old_init:?}");
    };
    assert_eq!(
        *old_init_id,
        LocalId::from_raw(0),
        "old temp must be initialized by reading the target local"
    );

    let MirStmt::Let {
        local: new_temp,
        init: Some(new_init),
        ..
    } = &out[1]
    else {
        panic!(
            "expected out[1] = Let init=Binary(old + rhs), got {:?}",
            out[1]
        );
    };
    let MirExpr::Binary { left, right, .. } = new_init else {
        panic!("prefix Let init must be Binary(old + rhs), got {new_init:?}");
    };
    let MirExpr::Local(left_id) = left.as_ref() else {
        panic!("prefix Binary.left must reuse the old temp, got {left:?}");
    };
    assert_eq!(
        *left_id, *old_temp,
        "prefix Binary.left must reference the old temp captured before RHS side effects"
    );
    let MirExpr::Int { value: rhs_val, .. } = right.as_ref() else {
        panic!("prefix Binary.right must be rhs MirExpr, got {right:?}");
    };
    assert_eq!(*rhs_val, 2);

    let MirStmt::Assign {
        target: place,
        value,
    } = &out[2]
    else {
        panic!("expected out[2] = Assign, got {:?}", out[2]);
    };
    let MirExpr::Local(assign_src) = value else {
        panic!("prefix Assign value must be MirExpr::Local(new_temp), got {value:?}");
    };
    assert_eq!(
        *assign_src, *new_temp,
        "prefix Assign must write from the materialized new-value temp"
    );
    assert!(
        matches!(place, ts_aot_ir_mir::MirPlace::Local { id } if *id == LocalId::from_raw(0)),
        "prefix Assign target must be the original target local, got {place:?}"
    );

    let MirExpr::Local(returned) = mir else {
        panic!("prefix CompoundUpdate must return MirExpr::Local(new_temp), got {mir:?}");
    };
    assert_eq!(
        returned, *new_temp,
        "prefix must return the materialized NEW value"
    );
}

#[test]
fn convert_expr_compound_update_rhs_call_evaluated_only_once() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_call),
        post: false,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut call_in_init_count = 0;
    let mut assign_value_is_binary = false;
    for stmt in out.iter() {
        match stmt {
            MirStmt::Let {
                init: Some(init), ..
            } if expr_contains_call(init) => {
                call_in_init_count += 1;
            }
            MirStmt::Assign {
                value: MirExpr::Binary { .. },
                ..
            } => assign_value_is_binary = true,
            _ => {}
        }
    }
    assert_eq!(
        call_in_init_count, 1,
        "rhs Call must appear in exactly one Let init (the materialized new value), got {out:?}"
    );
    assert!(
        !assign_value_is_binary,
        "Assign value must not be a Binary (which would re-run rhs on every place eval), got {out:?}"
    );

    let MirStmt::Assign { value, .. } = &out[2] else {
        panic!("expected Assign at index 2, got {:?}", out[2]);
    };
    let MirStmt::Let {
        local: new_temp, ..
    } = &out[1]
    else {
        panic!("expected Let new_temp at index 1, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = value else {
        panic!(
            "Assign value must be MirExpr::Local pointing at the materialized new temp, got {value:?}"
        );
    };
    assert_eq!(
        *assign_src, *new_temp,
        "Assign must write from the materialized new-value temp"
    );
}

fn expr_contains_call(e: &MirExpr) -> bool {
    match e {
        MirExpr::Call { .. } => true,
        MirExpr::Binary { left, right, .. } => {
            expr_contains_call(left) || expr_contains_call(right)
        }
        MirExpr::Unary { expr, .. } => expr_contains_call(expr),
        MirExpr::Field { base, .. } => expr_contains_call(base),
        MirExpr::Index { base, index, .. } => expr_contains_call(base) || expr_contains_call(index),
        MirExpr::Await { expr, .. } => expr_contains_call(expr),
        MirExpr::Yield { expr, .. } => expr.as_ref().is_some_and(|e| expr_contains_call(e)),
        _ => false,
    }
}

#[test]
fn convert_block_expr_compound_update_emits_local_expr_stmt_not_binary() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Expr {
        expr: HirExpr::CompoundUpdate {
            target: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),
            }),
            op: HirBinaryOp::Add,
            rhs: Box::new(int_lit(1)),
            post: true,
            ty: unit_ty(),
        },
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());

    let trailing = mir_block
        .stmts
        .iter()
        .rev()
        .find_map(|s| match s {
            MirStmt::Expr(e) => Some(e),
            _ => None,
        })
        .expect("expression statement must emit MirStmt::Expr");
    assert!(
        matches!(trailing, MirExpr::Local(_)),
        "statement-level compound update must end in a load of the materialized temp, not a Binary that re-runs rhs, got {trailing:?}"
    );
}

#[test]
fn convert_expr_compound_update_postfix_index_target_materializes_base_and_index() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(7)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(9)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut arr_call_inits = 0;
    let mut i_call_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(7) {
                arr_call_inits += 1;
            } else if *callee == FunctionId::from_raw(9) {
                i_call_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_call_inits, 1,
        "arr() must be materialized exactly once (in target base), got {out:?}"
    );
    assert_eq!(
        i_call_inits, 1,
        "i() must be materialized exactly once (in target index), got {out:?}"
    );

    let assign_target = match out.last() {
        Some(MirStmt::Assign { target, .. }) => target,
        other => panic!("expected last stmt to be Assign, got {other:?}"),
    };
    fn assert_place_is_pure(place: &MirPlace, path: &str, out: &[MirStmt]) {
        match place {
            MirPlace::Local { .. } => {}
            MirPlace::Field { base, .. } => {
                assert_place_base_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirPlace::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
        }
    }
    fn assert_place_base_is_pure(base: &MirPlaceBase, path: &str, out: &[MirStmt]) {
        match base {
            MirPlaceBase::Local(_) => {}
            MirPlaceBase::Field { base, .. } => {
                assert_place_base_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirPlaceBase::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
            MirPlaceBase::Chain { base, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.chain-base"), out);
            }
        }
    }
    fn assert_mir_expr_is_pure(expr: &MirExpr, path: &str, out: &[MirStmt]) {
        match expr {
            MirExpr::Local(_) => {}
            MirExpr::Field { base, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.field-base"), out);
            }
            MirExpr::Index { base, index, .. } => {
                assert_mir_expr_is_pure(base, &format!("{path}.base"), out);
                assert_mir_expr_is_pure(index, &format!("{path}.index"), out);
            }
            other => panic!(
                "Assign target subtree at {path} must be a pure Local/Field/Index, got {other:?}; full out: {out:?}"
            ),
        }
    }
    assert_place_is_pure(assign_target, "Assign.target", out);
}

#[test]
fn convert_expr_compound_update_prefix_index_target_materializes_base_and_index() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(11)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(13)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: false,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut arr_inits = 0;
    let mut i_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(11) {
                arr_inits += 1;
            } else if *callee == FunctionId::from_raw(13) {
                i_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_inits, 1,
        "arr() in prefix ++ must also be materialized once, got {out:?}"
    );
    assert_eq!(
        i_inits, 1,
        "i() in prefix ++ must also be materialized once, got {out:?}"
    );
}

#[test]
fn convert_expr_compound_update_postfix_index_then_field_target_materializes_all() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(17)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(19)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let index_target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),
    };
    let target = HirExpr::Field {
        owner: Box::new(index_target),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("0"),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: true,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "Field-owner for arr()[i()].field++ with no struct id registered must surface P0012; got {:?}",
        cx.diagnostics()
    );

    let mut arr_inits = 0;
    let mut i_inits = 0;
    for stmt in out.iter() {
        if let MirStmt::Let {
            init: Some(MirExpr::Call { callee, .. }),
            ..
        } = stmt
        {
            if *callee == FunctionId::from_raw(17) {
                arr_inits += 1;
            } else if *callee == FunctionId::from_raw(19) {
                i_inits += 1;
            }
        }
    }
    assert_eq!(
        arr_inits, 1,
        "nested arr()[i()].field++ must materialize arr() once, got {out:?}"
    );
    assert_eq!(
        i_inits, 1,
        "nested arr()[i()].field++ must materialize i() once, got {out:?}"
    );
}

#[test]
fn convert_expr_compound_update_index_target_plus_call_rhs_each_call_once() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(21)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(23)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(25)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_call),
        post: false,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let mut counts: HashMap<u32, usize> = HashMap::new();
    fn visit(e: &MirExpr, counts: &mut HashMap<u32, usize>) {
        if let MirExpr::Call { callee, .. } = e {
            let key = callee.raw();
            counts.entry(key).and_modify(|c| *c += 1).or_insert(1);
        }
        match e {
            MirExpr::Binary { left, right, .. } => {
                visit(left, counts);
                visit(right, counts);
            }
            MirExpr::Field { base, .. } => visit(base, counts),
            MirExpr::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
            MirExpr::Unary { expr, .. } => visit(expr, counts),
            MirExpr::Call { args, .. } => {
                for a in args {
                    visit(a, counts);
                }
            }
            MirExpr::Await { expr, .. } => visit(expr, counts),
            MirExpr::Yield { expr, .. } => {
                if let Some(e) = expr.as_ref() {
                    visit(e, counts);
                }
            }
            _ => {}
        }
    }
    fn visit_place(p: &MirPlace, counts: &mut HashMap<u32, usize>) {
        match p {
            MirPlace::Local { .. } => {}
            MirPlace::Field { base, .. } => visit_place_base(base, counts),
            MirPlace::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
        }
    }
    fn visit_place_base(b: &MirPlaceBase, counts: &mut HashMap<u32, usize>) {
        match b {
            MirPlaceBase::Local(_) => {}
            MirPlaceBase::Field { base, .. } => visit_place_base(base, counts),
            MirPlaceBase::Index { base, index, .. } => {
                visit(base, counts);
                visit(index, counts);
            }
            MirPlaceBase::Chain { base, .. } => visit(base, counts),
        }
    }
    for stmt in out.iter() {
        match stmt {
            MirStmt::Let {
                init: Some(init), ..
            } => visit(init, &mut counts),
            MirStmt::Assign { target, value } => {
                visit_place(target, &mut counts);
                visit(value, &mut counts);
            }
            _ => {}
        }
    }
    assert_eq!(
        counts.get(&21).copied().unwrap_or(0),
        1,
        "arr() must run once, got {:?}; out: {out:?}",
        counts
    );
    assert_eq!(
        counts.get(&23).copied().unwrap_or(0),
        1,
        "i() must run once, got {:?}; out: {out:?}",
        counts
    );
    assert_eq!(
        counts.get(&25).copied().unwrap_or(0),
        1,
        "rhs f() must run once, got {:?}; out: {out:?}",
        counts
    );
}

#[test]
fn convert_expr_compound_update_loads_old_value_before_rhs_runtime_stmt() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let f_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(101)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let rhs_template = HirExpr::Template {
        tag: None,
        expressions: vec![f_call],
        cooked_parts: vec![None, None],
        raw_parts: vec![None, None],
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(rhs_template),
        post: false,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let old_let_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Let {
                init: Some(MirExpr::Local(id)),
                ..
            } if *id == LocalId::from_raw(0)
        )
    });
    let rhs_runtime_idx = out.iter().position(|s| {
        matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                ..
            }
        )
    });
    let (Some(li), Some(ri)) = (old_let_idx, rhs_runtime_idx) else {
        panic!(
            "expected both `Let old=target` and `MirStmt::Runtime(StringConcat)` to be emitted, got {out:?}"
        );
    };
    assert!(
        li < ri,
        "JS/TS compound assignment must read LHS (Let old=target) BEFORE evaluating the RHS (MirStmt::Runtime for template); otherwise an RHS that mutates the target would corrupt `old`. got let@{li}, rhs_runtime@{ri}; out: {out:?}"
    );

    let f_call_in_runtime_args = out.iter().any(|s| {
        if let MirStmt::Runtime {
            args,
            op: RuntimeOp::StringConcat,
            ..
        } = s
        {
            args.iter()
                .any(|a| matches!(a, MirExpr::Call { callee, .. } if callee.raw() == 101))
        } else {
            false
        }
    });
    assert!(
        f_call_in_runtime_args,
        "the RHS `f()` must end up inside the StringConcat Runtime stmt (i.e. as an arg), not duplicated elsewhere; got {out:?}"
    );

    let binary_left_uses_old_temp = out.iter().any(|s| {
        if let MirStmt::Let {
            init: Some(MirExpr::Binary { left, .. }),
            ..
        } = s
        {
            matches!(left.as_ref(), MirExpr::Local(_))
        } else {
            false
        }
    });
    assert!(
        binary_left_uses_old_temp,
        "the new-value Binary.left must reference the old temp (so the computed value uses the value read BEFORE the RHS mutation), not the live target; got {out:?}"
    );
}

#[test]
fn convert_expr_assignment_value_temp_carries_rhs_ty_not_type_zero() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let rhs_ty = TypeId::from_raw(17);
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(505)),
        args: Vec::new(),
        ty: rhs_ty,
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs_call),
        ty: rhs_ty,
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let MirStmt::Let {
        local: value_temp,
        ty: let_ty,
        ..
    } = &out[0]
    else {
        panic!("expected out[0] = Let init=rhs, got {:?}", out[0]);
    };
    assert_eq!(
        *let_ty, rhs_ty,
        "Let init for value_temp must declare the RHS type ({rhs_ty:?}), not TypeId(0) — the prior code used TypeId(0) which silently mis-typed the materialized local"
    );

    let MirStmt::Assign {
        value: assign_value,
        ..
    } = &out[1]
    else {
        panic!("expected out[1] = Assign, got {:?}", out[1]);
    };
    let MirExpr::Local(assign_src) = assign_value else {
        panic!("Assign value must be MirExpr::Local(value_temp), got {assign_value:?}");
    };
    assert_eq!(
        *assign_src, *value_temp,
        "Assign must read from the same value_temp declared with the correct ty"
    );
}

#[test]
fn convert_expr_assignment_rhs_call_materialized_once_for_statement_and_return() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(303)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(rhs_call),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors());

    let rhs_call_in_let_inits = out
        .iter()
        .filter(|s| {
            if let MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ..
            } = s
            {
                callee.raw() == 303
            } else {
                false
            }
        })
        .count();
    assert_eq!(
        rhs_call_in_let_inits, 1,
        "rhs Call must appear in exactly one Let init (the materialized value temp), got {out:?}"
    );

    let rhs_call_in_assign_values = out
        .iter()
        .filter(|s| {
            if let MirStmt::Assign {
                value: MirExpr::Call { callee, .. },
                ..
            } = s
            {
                callee.raw() == 303
            } else {
                false
            }
        })
        .count();
    assert_eq!(
        rhs_call_in_assign_values, 0,
        "Assign value must NOT be a Call (would re-run rhs in statement-context Expr), got {out:?}"
    );
}

#[test]
fn convert_block_expr_plain_assignment_returns_local_not_rhs() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let rhs_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(404)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let block = HirBlock(vec![HirStmt::Expr {
        expr: HirExpr::Assignment {
            target: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),
            }),
            value: Box::new(rhs_call),
            ty: unit_ty(),
        },
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut empty_types(), &mut cx);
    assert!(!cx.has_errors());

    let rhs_call_count: usize = mir_block
        .stmts
        .iter()
        .map(|s| count_calls_in_stmt(s, 404))
        .sum();
    assert_eq!(
        rhs_call_count, 1,
        "statement-level `a = sideEffect()` must invoke sideEffect exactly once across the whole block (Assign + Expr trailing), got {mir_block:?}"
    );

    let trailing = mir_block
        .stmts
        .iter()
        .rev()
        .find_map(|s| match s {
            MirStmt::Expr(e) => Some(e),
            _ => None,
        })
        .expect("expression statement must emit MirStmt::Expr");
    assert!(
        matches!(trailing, MirExpr::Local(_)),
        "statement-level plain assignment must end in MirStmt::Expr(Local(value_temp)), not a re-evaluation of the RHS expression, got {trailing:?}"
    );
}

fn count_calls_in_stmt(s: &MirStmt, target: u32) -> usize {
    fn visit_expr(e: &MirExpr, target: u32) -> usize {
        let mut count = 0;
        if let MirExpr::Call { callee, .. } = e
            && callee.raw() == target
        {
            count += 1;
        }
        match e {
            MirExpr::Binary { left, right, .. } => {
                count += visit_expr(left, target);
                count += visit_expr(right, target);
            }
            MirExpr::Field { base, .. } => count += visit_expr(base, target),
            MirExpr::Index { base, index, .. } => {
                count += visit_expr(base, target);
                count += visit_expr(index, target);
            }
            MirExpr::Unary { expr, .. } => count += visit_expr(expr, target),
            MirExpr::Call { args, .. } => {
                for a in args {
                    count += visit_expr(a, target);
                }
            }
            MirExpr::Await { expr, .. } => count += visit_expr(expr, target),
            MirExpr::Yield { expr, .. } => {
                if let Some(inner) = expr.as_ref() {
                    count += visit_expr(inner, target);
                }
            }
            _ => {}
        }
        count
    }
    match s {
        MirStmt::Let {
            init: Some(init), ..
        } => visit_expr(init, target),
        MirStmt::Assign { value, .. } => visit_expr(value, target),
        MirStmt::Return(Some(e)) => visit_expr(e, target),
        MirStmt::ReturnResultErr { error, .. } => visit_expr(error, target),
        MirStmt::Throw { error, .. } => visit_expr(error, target),
        MirStmt::If { cond, .. } => visit_expr(cond, target),
        MirStmt::While { cond, .. } => visit_expr(cond, target),
        MirStmt::ForOf { iterable, .. } => visit_expr(iterable, target),
        MirStmt::ForIn { object, .. } => visit_expr(object, target),
        MirStmt::Runtime { args, .. } => args.iter().map(|a| visit_expr(a, target)).sum(),
        MirStmt::Expr(e) => visit_expr(e, target),
        _ => 0,
    }
}

#[test]
fn convert_expr_assignment_field_target_with_call_base_materializes_call_with_call_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let obj_ty = TypeId::from_raw(31);
    let obj_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(606)),
        args: Vec::new(),
        ty: obj_ty,
    };
    let field_target = HirExpr::Field {
        owner: Box::new(obj_call),
        field: FieldId::from_raw(0),
        field_name: Atom::new_inline("x"),
        ty: obj_ty,
    };
    let expr = HirExpr::Assignment {
        target: Box::new(field_target),
        value: Box::new(int_lit(7)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "non-registered struct id for Call-owner with ty {obj_ty:?} must surface P0012; got {:?}",
        cx.diagnostics()
    );

    let materialize_let = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ty,
                ..
            } if callee.raw() == 606 => Some(*ty),
            _ => None,
        })
        .expect("expected Let init=Call(obj) from materialize callback");
    assert_eq!(
        materialize_let, obj_ty,
        "MirStmt::Let for materialized obj() must declare the Call's ty ({obj_ty:?}), not TypeId(0) — downstream consumers see the wrong type otherwise"
    );
}

#[test]
fn convert_expr_compound_update_index_target_materializes_arr_call_with_arr_ty() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let arr_ty = TypeId::from_raw(53);
    let arr_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(707)),
        args: Vec::new(),
        ty: arr_ty,
    };
    let i_call = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(709)),
        args: Vec::new(),
        ty: unit_ty(),
    };
    let target = HirExpr::Index {
        owner: Box::new(arr_call),
        index: Box::new(i_call),
        ty: unit_ty(),
    };
    let expr = HirExpr::CompoundUpdate {
        target: Box::new(target),
        op: HirBinaryOp::Add,
        rhs: Box::new(int_lit(1)),
        post: false,
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(!cx.has_errors(), "arr()[i()]++ must not error");

    let arr_materialize_let_ty = out
        .iter()
        .find_map(|s| match s {
            MirStmt::Let {
                init: Some(MirExpr::Call { callee, .. }),
                ty,
                ..
            } if callee.raw() == 707 => Some(*ty),
            _ => None,
        })
        .expect("expected Let init=Call(arr) from ensure_place_pure_components");
    assert_eq!(
        arr_materialize_let_ty, arr_ty,
        "MirStmt::Let for materialized arr() must declare the Call's ty ({arr_ty:?}), not TypeId(0)"
    );
}

#[test]
fn resolve_field_id_call_owner_with_registered_struct_id_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let call_ret_ty = TypeId::from_raw(91);
    let sid = ts_aot_core::StructId::from_raw(0);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(call_ret_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("answer");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(42));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::Call {
        callee: HirCallee::Function(FunctionId::from_raw(0)),
        args: Vec::new(),
        ty: call_ret_ty,
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(42),
        "Call-owner with a registered struct id must resolve the field id by looking up (sid, field_name); got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "a registered struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_non_typed_owner_emits_p0011() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    c.set_field_id_lookup(HashMap::new());

    let owner = HirExpr::Int(0);
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(99),
        "non-typed owner must fall back to placeholder after emitting P0011"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0011"),
        "non-typed owner (Int) must surface P0011; got {:?}",
        cx.diagnostics()
    );
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "P0012 must not be reported when the failure is the owner type, not the missing struct id"
    );
}

#[test]
fn resolve_field_id_type_assertion_owner_with_registered_target_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let target_ty = TypeId::from_raw(101);
    let sid = ts_aot_core::StructId::from_raw(1);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(target_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("tag");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(7));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::TypeAssertion {
        expr: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),
        }),
        target: target_ty,
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(7),
        "(obj as T).field must resolve via TypeAssertion's target type when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "TypeAssertion owner with registered target struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_type_assertion_owner_without_registered_target_emits_p0012() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    c.set_field_id_lookup(HashMap::new());

    let owner = HirExpr::TypeAssertion {
        expr: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),
        }),
        target: TypeId::from_raw(202),
    };
    let resolved = c.resolve_field_id(
        &owner,
        &Atom::new_inline("x"),
        FieldId::from_raw(99),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(99),
        "TypeAssertion owner with unregistered target must fall back to placeholder after emitting P0012"
    );
    assert!(
        cx.diagnostics().iter().any(|d| d.code.as_str() == "P0012"),
        "TypeAssertion owner whose target type has no registered struct id must surface P0012, not P0011; got {:?}",
        cx.diagnostics()
    );
    assert!(
        !cx.diagnostics().iter().any(|d| d.code.as_str() == "P0011"),
        "P0011 must not be reported when the owner is typed but the target struct id is missing"
    );
}

#[test]
fn resolve_field_id_assignment_owner_with_registered_ty_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let assign_ty = TypeId::from_raw(103);
    let sid = ts_aot_core::StructId::from_raw(2);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(assign_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("payload");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(13));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::Assignment {
        target: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: TypeId::from_raw(0),
        }),
        value: Box::new(HirExpr::Local {
            id: LocalId::from_raw(1),
            ty: assign_ty,
        }),
        ty: assign_ty,
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(13),
        "(obj = makeC()).field must resolve via Assignment's ty when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "Assignment owner with registered ty struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn resolve_field_id_compound_update_owner_with_registered_ty_resolves_field() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let target_ty = TypeId::from_raw(104);
    let sid = ts_aot_core::StructId::from_raw(3);
    let mut struct_ids: HashMap<TypeId, ts_aot_core::StructId> = HashMap::new();
    struct_ids.insert(target_ty, sid);
    let mut field_id_lookup: HashMap<(ts_aot_core::StructId, Atom), FieldId> = HashMap::new();
    let field_name = Atom::new_inline("count");
    field_id_lookup.insert((sid, field_name.clone()), FieldId::from_raw(21));
    c.set_field_id_lookup(field_id_lookup);

    let owner = HirExpr::CompoundUpdate {
        target: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: target_ty,
        }),
        op: HirBinaryOp::Add,
        rhs: Box::new(HirExpr::Int(1)),
        post: false,
        ty: target_ty,
    };
    let resolved = c.resolve_field_id(
        &owner,
        &field_name,
        FieldId::from_raw(u32::MAX),
        &struct_ids,
        &mut cx,
    );
    assert_eq!(
        resolved,
        FieldId::from_raw(21),
        "(obj += 1).field must resolve via CompoundUpdate's ty when the struct id is registered; got placeholder instead"
    );
    assert!(
        !cx.has_errors(),
        "CompoundUpdate owner with registered ty struct id + present field must not emit any diagnostic, got {:?}",
        cx.diagnostics()
    );
}

fn object_method_call(field_name: &str) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline("Object"),
                ty: unit_ty(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),
        })),
        args: Vec::new(),
        ty: unit_ty(),
    }
}

fn local_method_call(field_name: &str) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),
        })),
        args: Vec::new(),
        ty: unit_ty(),
    }
}

fn e0404_count(diagnostics: &ts_aot_core::DiagnosticBag) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E0404")
        .count()
}

#[test]
fn e0404_emits_for_object_keys_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mir = c.convert_expr(
        &object_method_call("keys"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "E0404 path must lower to MirExpr::Unit, got {mir:?}"
    );
    assert_eq!(
        e0404_count(cx.diagnostics()),
        1,
        "Object.keys() must emit exactly one E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_emits_for_object_get_prototype_of_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &object_method_call("getPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(e0404_count(cx.diagnostics()), 1);
}

#[test]
fn e0404_emits_for_object_set_prototype_of_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &object_method_call("setPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(e0404_count(cx.diagnostics()), 1);
}

#[test]
fn e0404_not_emitted_for_local_receiver_keys_call() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &local_method_call("keys"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        e0404_count(cx.diagnostics()),
        0,
        "myMap.keys() with local receiver must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_not_emitted_for_local_receiver_user_defined_method() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &local_method_call("getPrototypeOf"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        e0404_count(cx.diagnostics()),
        0,
        "user-defined `.getPrototypeOf` on local receiver must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}

#[test]
fn e0404_not_emitted_for_object_local_global_other_method() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    c.convert_expr(
        &object_method_call("assign"),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert_eq!(
        e0404_count(cx.diagnostics()),
        0,
        "Object.assign() (not in banned set) must not trigger E0404, got {:?}",
        cx.diagnostics()
    );
}
