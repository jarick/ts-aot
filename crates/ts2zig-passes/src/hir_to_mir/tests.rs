use std::collections::HashMap;

use ts2zig_core::{Atom, FieldId, FunctionId, LocalId, ModuleId, Span, TypeId, Visibility};
use ts2zig_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
    HirSwitchCase, HirUnaryOp,
};
use ts2zig_ir_mir::{BinaryOp, FunctionKind, MirExpr, MirPlace, MirStmt, RuntimeOp, UnaryOp};

use super::{ExprConverter, HirBlock, PLACEHOLDER_FUNCTION, convert_function, convert_program};
use crate::PassContext;

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

fn empty_struct_ids() -> std::collections::HashMap<ts2zig_core::TypeId, ts2zig_core::StructId> {
    std::collections::HashMap::new()
}

fn empty_next_struct() -> u32 {
    0
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
fn push_await_increments_state() {
    let mut c = ExprConverter::new();
    let (d1, s1) = c.push_await();
    let (d2, s2) = c.push_await();
    assert_ne!(d1, d2);
    assert_eq!(s1, 1);
    assert_eq!(s2, 2);
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
fn resolve_callee_indirect_is_placeholder_and_diagnostics() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let fid = c.resolve_callee(&HirCallee::Indirect(Box::new(int_lit(1))), &mut cx);
    assert_eq!(fid, PLACEHOLDER_FUNCTION);
    assert!(cx.has_errors());
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
        parts: vec![int_lit(1), int_lit(2)],
        ty: unit_ty(),
    };
    let mir = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    let dest_id = match &out[0] {
        MirStmt::Runtime { dest: Some(d), .. } => *d,
        other => panic!("expected Runtime with dest, got {other:?}"),
    };
    assert_eq!(mir, MirExpr::Local(dest_id));
}

#[test]
fn convert_expr_await_emits_await_stmt() {
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
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], MirStmt::Await { next_state: 1, .. }));
    assert!(matches!(mir, MirExpr::Local(_)));
}

#[test]
fn convert_expr_closure_returns_unit_and_diagnostics() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Closure {
        id: LocalId::from_raw(0),
        captures: Vec::new(),
        ty: unit_ty(),
    };
    assert_eq!(
        c.convert_expr(
            &expr,
            out,
            &mut empty_struct_ids(),
            &mut empty_next_struct(),
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
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(
        out[0],
        MirStmt::Assign {
            target: ts2zig_ir_mir::MirPlace::Local { .. },
            ..
        }
    ));
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
        &mut cx,
    );
    assert!(
        matches!(mir, MirExpr::Int { value: 7, .. }),
        "assignment must yield the assigned value, got {mir:?}"
    );
}

#[test]
fn convert_expr_assignment_value_template_emits_runtime_before_assign() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let target = HirExpr::Local {
        id: LocalId::from_raw(0),
        ty: unit_ty(),
    };
    let value = HirExpr::Template {
        tag: None,
        parts: vec![int_lit(7)],
        ty: unit_ty(),
    };
    let expr = HirExpr::Assignment {
        target: Box::new(target),
        value: Box::new(value),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert_eq!(out.len(), 2);
    assert!(matches!(out[0], MirStmt::Runtime { .. }));
    assert!(matches!(out[1], MirStmt::Assign { .. }));
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
        &mut cx,
    );
    assert!(cx.has_errors());
    assert!(out.is_empty());
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
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(
        out[0],
        MirStmt::Assign {
            target: ts2zig_ir_mir::MirPlace::Field { .. },
            ..
        }
    ));
    assert!(!cx.has_errors());
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
        &mut cx,
    );
    assert_eq!(out.len(), 1);
    match &out[0] {
        MirStmt::Assign { target, .. } => match target {
            ts2zig_ir_mir::MirPlace::Field { base, field, .. } => {
                assert_eq!(*field, FieldId::from_raw(3));
                assert!(matches!(**base, ts2zig_ir_mir::MirPlaceBase::Index { .. }));
            }
            other => panic!("expected Field place with Index base, got {other:?}"),
        },
        other => panic!("expected Assign, got {other:?}"),
    }
    assert!(!cx.has_errors());
}

#[test]
fn convert_expr_optional_chain_emits_diagnostic() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::OptionalChain {
        base: Box::new(HirExpr::Local {
            id: LocalId::from_raw(0),
            ty: unit_ty(),
        }),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert!(cx.has_errors());
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 for optional chain");
    assert!(diag.message.contains("optional chaining"));
}

#[test]
fn convert_block_empty_produces_empty() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let (block, locals) = c.convert_block(&HirBlock(Vec::new()), &mut cx);
    assert!(block.is_empty());
    assert!(locals.is_empty());
    assert!(!cx.has_errors());
}

#[test]
fn convert_block_direct_drains_await_temp_locals() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(HirExpr::Await {
            expr: Box::new(int_lit(1)),
            ty: unit_ty(),
        }),
    }]);
    let (_, locals) = c.convert_block(&block, &mut cx);
    let await_dest = match locals.as_slice() {
        [single] => single.id,
        _ => panic!(
            "expected exactly 1 local from await, got {}: {locals:?}",
            locals.len()
        ),
    };
    assert!(
        locals.iter().any(|l| l.id == await_dest && l.mutable),
        "await dest {await_dest:?} must be in convert_block's locals (mutable)"
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
    let (_, locals) = c.convert_block(&block, &mut cx);
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
    let (mir_block, locals) = c.convert_block(&block, &mut cx);
    assert_eq!(mir_block.len(), 1);
    assert_eq!(locals.len(), 1);
    assert_eq!(locals[0].name, Atom::new_inline("11"));
}

#[test]
fn convert_block_expr_emits_expr_stmt() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Expr { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Expr(_)));
}

#[test]
fn convert_block_return_emits_return() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Return {
        value: Some(int_lit(0)),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    let MirStmt::While { cond, .. } = &mir_block.stmts[1] else {
        panic!("expected MirStmt::While at index 1");
    };
    assert!(matches!(*cond, MirExpr::Bool(false)));
    assert!(!matches!(*cond, MirExpr::Bool(true)));
}

#[test]
fn convert_block_while_template_cond_runs_template_each_iteration() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Template {
        tag: None,
        parts: vec![int_lit(1), int_lit(2)],
        ty: unit_ty(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    let outer_while_idx = mir_block
        .stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected outer MirStmt::While");
    let outer_while = match &mir_block.stmts[outer_while_idx] {
        MirStmt::While { cond, body } => (cond, body),
        other => panic!("expected MirStmt::While, got {other:?}"),
    };
    assert!(matches!(*outer_while.0, MirExpr::Local(_)));
    let inner_while_idx = outer_while
        .1
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::While {
                    cond: MirExpr::Bool(true),
                    ..
                }
            )
        })
        .expect("expected inner MirStmt::While at index 0 of outer-while body");
    let inner_body = match &outer_while.1.stmts[inner_while_idx] {
        MirStmt::While { body: ib, .. } => &ib.stmts,
        other => panic!("expected inner MirStmt::While, got {other:?}"),
    };
    assert!(
        inner_body
            .iter()
            .any(|s| matches!(s, MirStmt::Expr(MirExpr::Int { value: 0, .. }))),
        "original body stmts must remain in inner-while body, got {:?}",
        inner_body
    );
    let template_runtime_idx = outer_while
        .1
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    ..
                }
            )
        })
        .expect("template runtime stmt must be present in outer-while body");
    assert!(
        template_runtime_idx > inner_while_idx,
        "template runtime stmt (idx {}) must appear AFTER the inner-while wrapper (idx {}) so cond re-evaluates each iteration; got stmts {:?}",
        template_runtime_idx,
        inner_while_idx,
        outer_while.1.stmts
    );
}

#[test]
fn convert_block_while_continue_re_evaluates_cond_via_inner_wrapper() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let cond = HirExpr::Template {
        tag: None,
        parts: vec![int_lit(1)],
        ty: unit_ty(),
    };
    let block = HirBlock(vec![HirStmt::While {
        cond,
        body: Box::new(HirStmt::Continue { label: None }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    assert!(
        outer_while.stmts.iter().any(|s| matches!(
            s,
            MirStmt::Runtime {
                op: RuntimeOp::StringConcat,
                ..
            }
        )),
        "cond_stmts must run after the inner wrapper so Continue still re-evaluates cond, got {:?}",
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
fn convert_block_dowhile_template_cond_runs_each_iteration() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::DoWhile {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        cond: HirExpr::Template {
            tag: None,
            parts: vec![int_lit(1), int_lit(2)],
            ty: unit_ty(),
        },
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    let while_stmt = match &mir_block.stmts[2] {
        MirStmt::While { body, .. } => body,
        other => panic!("expected While at index 2, got {other:?}"),
    };
    let inner_while_idx = while_stmt
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::While {
                    cond: MirExpr::Bool(true),
                    ..
                }
            )
        })
        .expect("expected inner MirStmt::While in do-while body");
    let template_runtime_idx = while_stmt
        .stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    ..
                }
            )
        })
        .expect("template runtime stmt must be present in do-while body");
    assert!(
        template_runtime_idx > inner_while_idx,
        "template runtime stmt must appear AFTER the inner-while wrapper (so cond re-evaluates each iter even on Continue); got stmts={:?}",
        while_stmt.stmts
    );
}

#[test]
fn convert_block_while_template_cond_runtime_runs_before_loop() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::While {
        cond: HirExpr::Template {
            tag: None,
            parts: vec![int_lit(1)],
            ty: unit_ty(),
        },
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    let stmts = &mir_block.stmts;
    let initial_runtime_idx = stmts
        .iter()
        .position(|s| {
            matches!(
                s,
                MirStmt::Runtime {
                    op: RuntimeOp::StringConcat,
                    ..
                }
            )
        })
        .expect("template runtime stmt must be present");
    let outer_while_idx = stmts
        .iter()
        .position(|s| matches!(s, MirStmt::While { .. }))
        .expect("expected outer MirStmt::While");
    assert!(
        initial_runtime_idx < outer_while_idx,
        "template runtime stmt must appear BEFORE the outer MirStmt::While (cond re-evaluates each iter from populated temp); got stmts={:?}",
        stmts
    );
    let outer_while_cond = stmts
        .iter()
        .find_map(|s| match s {
            MirStmt::While { cond, .. } => Some(cond),
            _ => None,
        })
        .expect("outer MirStmt::While already validated by outer_while_idx");
    assert!(
        matches!(*outer_while_cond, MirExpr::Local(_)),
        "While.cond must be the Local(temp) holding the template result, got {outer_while_cond:?}"
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
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
    let (mir_block, locals) = c.convert_block(&block, &mut cx);
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
    let (mir_block, locals) = c.convert_block(&block, &mut cx);
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
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Break));
    assert!(matches!(mir_block.stmts[1], MirStmt::Continue));
}

#[test]
fn convert_block_throw_emits_throw() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Throw { expr: int_lit(0) }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    assert!(matches!(mir_block.stmts[0], MirStmt::Throw { .. }));
}

#[test]
fn convert_block_switch_emits_diagnostic_no_stmts() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Switch {
        disc: HirExpr::Int(0),
        cases: Vec::new(),
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    assert!(mir_block.is_empty());
    assert!(cx.has_errors());
}

#[test]
fn convert_block_try_emits_diagnostic_no_stmts() {
    let mut c = ExprConverter::new();
    let mut cx = ctx();
    let block = HirBlock(vec![HirStmt::Try {
        body: Box::new(HirStmt::Expr { expr: int_lit(0) }),
        catch: None,
        finally: None,
    }]);
    let (mir_block, _) = c.convert_block(&block, &mut cx);
    assert!(mir_block.is_empty());
    assert!(cx.has_errors());
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
    let mir = convert_program(&hir, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
fn convert_program_assigns_distinct_struct_ids() {
    use ts2zig_ir_hir::{HirClass, HirField};
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
    let mut struct_literal_ids: Vec<ts2zig_core::StructId> = Vec::new();
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
    use ts2zig_ir_hir::{HirClass, HirField, HirParam};
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
    let mir = convert_program(&prog, &mut cx);
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
    use ts2zig_ir_hir::{HirClass, HirField};
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
    let mir = convert_program(&prog, &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let mut new_id: Option<ts2zig_core::StructId> = None;
    let mut literal_id: Option<ts2zig_core::StructId> = None;
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
    use ts2zig_ir_hir::{HirClass, HirField};
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
    let mir = convert_program(&prog, &mut cx);
    let struct_decl = mir.structs().next().expect("expected one struct");
    let class_struct_id = struct_decl.id;
    let func = mir.functions().next().expect("expected one function");
    let mut found_new_id: Option<ts2zig_core::StructId> = None;
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
    let mut struct_id_map: HashMap<TypeId, ts2zig_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &mut struct_id_map,
        &mut next_struct_id,
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
    let mut struct_id_map: HashMap<TypeId, ts2zig_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &mut struct_id_map,
        &mut next_struct_id,
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
    let mut struct_id_map: HashMap<TypeId, ts2zig_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &mut struct_id_map,
        &mut next_struct_id,
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
    let mut struct_id_map: HashMap<TypeId, ts2zig_core::StructId> = HashMap::new();
    let mut next_struct_id: u32 = 0;
    let mir = convert_function(
        &f,
        FunctionId::from_raw(0),
        None,
        HashMap::new(),
        &mut struct_id_map,
        &mut next_struct_id,
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "assignment with throwing calls in arr()[idx()] LHS must propagate can_throw"
    );
}

#[test]
fn convert_program_preserves_import_module_path_from_atom() {
    use ts2zig_ir_hir::{HirExport, HirImport};
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
    let mir = convert_program(&prog, &mut cx);
    assert_eq!(mir.imports.len(), 1);
    assert_eq!(mir.imports[0].module, "./other");
    assert_eq!(mir.imports[0].symbol, Atom::new_inline("7"));
    assert_eq!(mir.exports.len(), 1);
    assert_eq!(mir.exports[0].symbol, Atom::new_inline("9"));
}

#[test]
fn convert_function_await_dest_appears_in_body_locals() {
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    let await_dest = match mir.body.block.stmts.last().expect("non-empty body") {
        MirStmt::Return(Some(MirExpr::Local(lid))) => *lid,
        other => panic!("expected last stmt Return(Some(Local)), got {other:?}"),
    };
    assert!(
        mir.body.locals.iter().any(|l| l.id == await_dest),
        "await dest {await_dest:?} must be in body.locals"
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "nested throw inside If must propagate to can_throw"
    );
}

#[test]
fn convert_function_build_params_preserves_param_atom_name() {
    use ts2zig_ir_hir::HirParam;
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
fn convert_unaryop_unsupported_variants_emit_diagnostic_at_call_site() {
    let mut c = ExprConverter::new();
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expr = HirExpr::Unary {
        op: HirUnaryOp::TypeOf,
        expr: Box::new(int_lit(1)),
        ty: unit_ty(),
    };
    let _ = c.convert_expr(
        &expr,
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    let diag = cx
        .diagnostics()
        .iter()
        .find(|d| d.code.as_str() == "P0005")
        .expect("expected P0005 for unsupported unary op");
    assert!(diag.message.contains("TypeOf"));
}

#[test]
fn convert_program_class_method_with_no_params_is_skipped() {
    use ts2zig_ir_hir::HirClass;
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
    let mut struct_id_map: HashMap<TypeId, ts2zig_core::StructId> = HashMap::new();
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
    let _ = c.convert_expr(&expr, out, &mut struct_id_map, &mut next_struct_id, &mut cx);
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
        &mut cx,
    );
    assert!(!cx.has_errors(), "obj().x = v must not error");
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
    let value_expr = HirExpr::Template {
        tag: None,
        parts: vec![int_lit(1)],
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert!(
        mir.effects.can_throw,
        "If with throwing cond must propagate can_throw"
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
            catch: Some(ts2zig_ir_hir::HirCatchClause::new(
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
    let mir = convert_program(&prog, &mut cx);
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(custom_err_ty),
        "throws must be derived from the thrown expression's type, not a sentinel"
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
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
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut cx,
    );
    assert_eq!(
        mir.throws,
        Some(TypeId::from_raw(0)),
        "primitive throw (no real source type) must fall back to TypeId(0) sentinel"
    );
}
