pub use std::collections::HashMap;
pub use std::sync::Arc;

pub use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, Span, StructId, Type, TypeId, TypeTable,
    Visibility,
};
pub use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirProgram, HirStmt,
    HirSwitchCase, HirUnaryOp, ObjectLiteralField,
};
pub use ts_aot_ir_mir::{
    BinaryOp, ConstValue, FunctionKind, MirBlock, MirExpr, MirPlace, MirPlaceBase, MirStmt,
    RuntimeOp, UnaryOp,
};

pub(crate) use super::super::ops::{convert_binop, convert_unaryop};
pub(crate) use super::super::{
    ExprConverter, HirBlock, PLACEHOLDER_FUNCTION, convert_function, convert_program,
};
pub(crate) use crate::PassContext;
pub(crate) use crate::lower_classes;

pub fn ctx() -> PassContext {
    PassContext::new()
}

pub fn int_lit(v: i64) -> HirExpr {
    HirExpr::Int(v, Span::default())
}

pub fn unit_ty() -> TypeId {
    TypeId::from_raw(0)
}

pub fn empty_hir() -> HirProgram {
    HirProgram::new(ModuleId::from_raw(0))
}

pub fn empty_struct_ids() -> std::collections::HashMap<ts_aot_core::TypeId, ts_aot_core::StructId> {
    std::collections::HashMap::new()
}

pub fn empty_next_struct() -> u32 {
    0
}

pub fn empty_types() -> TypeTable {
    TypeTable::new()
}

pub fn empty_field_id_lookup() -> HashMap<(ts_aot_core::StructId, Atom), FieldId> {
    HashMap::new()
}

pub fn run_convert(
    f: &HirFunction,
    id: FunctionId,
    export_name: Option<String>,
    remap: HashMap<FunctionId, FunctionId>,
    cx: &mut PassContext,
) -> ts_aot_ir_mir::MirFunctionDecl {
    convert_function(
        f,
        id,
        export_name,
        remap,
        &Arc::new(HashMap::new()),
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &empty_field_id_lookup(),
        &mut empty_types(),
        cx,
    )
}

pub fn expr_contains_call(e: &MirExpr) -> bool {
    match e {
        MirExpr::Call { .. } | MirExpr::IndirectCall { .. } => true,
        MirExpr::Binary { left, right, .. } => {
            expr_contains_call(left) || expr_contains_call(right)
        }
        MirExpr::Unary { expr, .. } => expr_contains_call(expr),
        MirExpr::Field { base, .. } => expr_contains_call(base),
        MirExpr::Index { base, index, .. } => expr_contains_call(base) || expr_contains_call(index),
        MirExpr::Await { expr, .. } => expr_contains_call(expr),
        MirExpr::Yield { expr, .. } => expr.as_ref().is_some_and(|e| expr_contains_call(e)),
        MirExpr::StructLiteral { fields, .. } => fields.iter().any(|(_, v)| expr_contains_call(v)),
        MirExpr::ResultOk { value, .. } => expr_contains_call(value),
        MirExpr::ResultErr { error, .. } => expr_contains_call(error),
        MirExpr::OptionalChain { base, .. } => expr_contains_call(base),
        MirExpr::TypeOf { expr, .. } => expr_contains_call(expr),
        MirExpr::Import { source, .. } => expr_contains_call(source),
        _ => false,
    }
}

pub fn count_calls_in_stmt(s: &MirStmt, target: u32) -> usize {
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
            MirExpr::IndirectCall { callee, args, .. } => {
                count += visit_expr(callee, target);
                for a in args {
                    count += visit_expr(a, target);
                }
            }
            MirExpr::StructLiteral { fields, .. } => {
                for (_, v) in fields {
                    count += visit_expr(v, target);
                }
            }
            MirExpr::ResultOk { value, .. } => count += visit_expr(value, target),
            MirExpr::ResultErr { error, .. } => count += visit_expr(error, target),
            MirExpr::OptionalChain { base, .. } => count += visit_expr(base, target),
            MirExpr::TypeOf { expr, .. } => count += visit_expr(expr, target),
            MirExpr::Import { source, .. } => count += visit_expr(source, target),
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
    fn visit_block(block: &MirBlock, target: u32) -> usize {
        block
            .stmts
            .iter()
            .map(|s| count_calls_in_stmt(s, target))
            .sum()
    }
    match s {
        MirStmt::Let {
            init: Some(init), ..
        } => visit_expr(init, target),
        MirStmt::Assign { value, .. } => visit_expr(value, target),
        MirStmt::Return(Some(e)) => visit_expr(e, target),
        MirStmt::ReturnResultErr { error, .. } => visit_expr(error, target),
        MirStmt::Throw { error, .. } => visit_expr(error, target),
        MirStmt::If {
            cond,
            then_block,
            else_block,
        } => {
            let mut count = visit_expr(cond, target);
            count += visit_block(then_block, target);
            if let Some(else_block) = else_block {
                count += visit_block(else_block, target);
            }
            count
        }
        MirStmt::While { cond, body } => visit_expr(cond, target) + visit_block(body, target),
        MirStmt::DoWhile { body, cond } => visit_block(body, target) + visit_expr(cond, target),
        MirStmt::ForOf { iterable, body, .. } => {
            visit_expr(iterable, target) + visit_block(body, target)
        }
        MirStmt::ForIn { object, body, .. } => {
            visit_expr(object, target) + visit_block(body, target)
        }
        MirStmt::Switch {
            disc,
            cases,
            default,
        } => {
            let mut count = visit_expr(disc, target);
            for case in cases {
                count += visit_block(&case.body, target);
            }
            if let Some(default) = default {
                count += visit_block(default, target);
            }
            count
        }
        MirStmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            let mut count = visit_block(body, target);
            if let Some(catch) = catch {
                count += visit_block(catch, target);
            }
            if let Some(finally) = finally {
                count += visit_block(finally, target);
            }
            count
        }
        MirStmt::Runtime { args, .. } => args.iter().map(|a| visit_expr(a, target)).sum(),
        MirStmt::Expr(e) => visit_expr(e, target),
        _ => 0,
    }
}

pub fn diag_count(diagnostics: &ts_aot_core::DiagnosticBag, code: &str) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code.as_str() == code)
        .count()
}

pub fn global_method_call(namespace: &str, field_name: &str, args: Vec<HirExpr>) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Global {
                name: Atom::new_inline(namespace),
                ty: unit_ty(),

                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),

            span: Span::default(),
        })),
        args,
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    }
}

pub fn object_method_call(field_name: &str) -> HirExpr {
    global_method_call("Object", field_name, Vec::new())
}

pub fn object_method_call_with_arg(field_name: &str, arg: HirExpr) -> HirExpr {
    global_method_call("Object", field_name, vec![arg])
}

pub fn object_method_call_with_args(field_name: &str, args: Vec<HirExpr>) -> HirExpr {
    global_method_call("Object", field_name, args)
}

pub fn array_method_call_with_arg(field_name: &str, arg: HirExpr) -> HirExpr {
    global_method_call("Array", field_name, vec![arg])
}

pub fn math_method_call_with_arg(field_name: &str, arg: HirExpr) -> HirExpr {
    global_method_call("Math", field_name, vec![arg])
}

pub fn math_method_call_with_2_args(field_name: &str, arg1: HirExpr, arg2: HirExpr) -> HirExpr {
    global_method_call("Math", field_name, vec![arg1, arg2])
}

pub fn box_constructor_call(ns: &str, arg: HirExpr, ty: TypeId) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Global {
            name: Atom::new_inline(ns),
            ty,
            span: Span::default(),
        })),
        args: vec![arg],
        ty,
        type_args: vec![],
        span: Span::default(),
    }
}

pub fn receiver_has_own_property_call(
    types: &mut TypeTable,
    receiver_ty: TypeId,
    key: &str,
) -> HirExpr {
    let bool_ty = types.intern(&Type::Bool);
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: receiver_ty,

                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline("hasOwnProperty"),
            ty: bool_ty,

            span: Span::default(),
        })),
        args: vec![HirExpr::String(Atom::new_inline(key), Span::default())],
        ty: bool_ty,
        type_args: vec![],

        span: Span::default(),
    }
}

pub fn local_method_call(field_name: &str) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: LocalId::from_raw(0),
                ty: unit_ty(),

                span: Span::default(),
            }),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),

            span: Span::default(),
        })),
        args: Vec::new(),
        ty: unit_ty(),
        type_args: vec![],

        span: Span::default(),
    }
}

pub fn string_method_call_with_args(field_name: &str, args: Vec<HirExpr>) -> HirExpr {
    global_method_call("String", field_name, args)
}

pub fn string_instance_method_call_with_args(
    owner: HirExpr,
    field_name: &str,
    args: Vec<HirExpr>,
) -> HirExpr {
    HirExpr::Call {
        callee: HirCallee::Indirect(Box::new(HirExpr::Field {
            owner: Box::new(owner),
            field: FieldId::from_raw(0),
            field_name: Atom::new_inline(field_name),
            ty: unit_ty(),
            span: Span::default(),
        })),
        args,
        ty: unit_ty(),
        type_args: vec![],
        span: Span::default(),
    }
}

pub fn function_method_call_with_args(
    fn_name: &str,
    field_name: &str,
    args: Vec<HirExpr>,
) -> HirExpr {
    global_method_call(fn_name, field_name, args)
}

pub fn int_array_literal(elements: Vec<i64>) -> HirExpr {
    HirExpr::ArrayLiteral {
        elements: elements
            .into_iter()
            .map(|v| HirExpr::Int(v, Span::default()))
            .collect(),
        ty: unit_ty(),
        span: Span::default(),
    }
}

pub fn function_call_with_non_nullish_thisarg_emits_e0406(
    thisarg: HirExpr,
    method: &str,
    extra_args: Vec<HirExpr>,
) {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let mut full_args = Vec::with_capacity(1 + extra_args.len());
    full_args.push(thisarg);
    full_args.extend(extra_args);
    let mir = c.convert_expr(
        &function_method_call_with_args("f", method, full_args),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    let e0406_matches: Vec<_> = cx
        .diagnostics()
        .iter()
        .filter(|d| d.code.as_str() == "E0406")
        .collect();
    assert_eq!(
        e0406_matches.len(),
        1,
        "f.{method}(non-nullish thisArg, ...) must emit exactly one E0406 with the \
         AOT no-`this`-binding message, got diagnostics: {:?}",
        cx.diagnostics()
    );
    let e0406 = e0406_matches[0];
    assert!(
        e0406.message.contains("no `this` binding"),
        "f.{method}(non-nullish thisArg) E0406 must use the Array.from-style no-`this`-binding \
         message, got: {:?}",
        e0406.message
    );
    assert!(
        matches!(mir, MirExpr::Unit),
        "f.{method}(non-nullish thisArg, ...) must return MirExpr::Unit (no MirExpr::Call \
         dispatched because the AOT target cannot bind a non-nullish thisArg), got {mir:?}"
    );
    assert!(
        out.is_empty(),
        "f.{method}(non-nullish thisArg, ...) must NOT materialize the rejected thisArg; \
         the dispatch emits E0406 and returns Unit without lowering any MirStmt. got: {out:?}"
    );
}

pub fn function_call_with_nullish_thisarg_dispatches_call(
    thisarg: HirExpr,
    method: &str,
    extra_args: Vec<HirExpr>,
) {
    let mut c = ExprConverter::new();
    c.name_to_function = Arc::new(HashMap::from([(
        Atom::new_inline("f"),
        FunctionId::from_raw(11),
    )]));
    let out = &mut Vec::new();
    let mut cx = ctx();
    let expected_forwarded = extra_args.len();
    let mut full_args = Vec::with_capacity(1 + extra_args.len());
    full_args.push(thisarg);
    full_args.extend(extra_args);
    let mir = c.convert_expr(
        &function_method_call_with_args("f", method, full_args),
        out,
        &mut empty_struct_ids(),
        &mut empty_next_struct(),
        &mut empty_types(),
        &mut cx,
    );
    assert!(
        !cx.has_errors(),
        "f.{method}(nullish thisArg, ...) must NOT emit any diagnostic (only the three HIR \
         nullish forms Unit / Null / Undefined are accepted as thisArg in AOT; the void \
         expression `void 0` lowers to Unit, `null` to Null, `undefined` to Undefined), \
         got: {:?}",
        cx.diagnostics()
    );
    match &mir {
        MirExpr::Call { callee, args, .. } => {
            assert_eq!(
                callee.raw(),
                11,
                "f.{method}(nullish thisArg, ...) must dispatch to f (FunctionId::from_raw(11)), \
                 got callee FunctionId raw = {}",
                callee.raw()
            );
            assert_eq!(
                args.len(),
                expected_forwarded,
                "f.{method}(nullish thisArg, ...) must forward exactly the extra args and \
                 OMIT the thisArg (the AOT dispatch strips args[0] before the call); \
                 expected {} forwarded args, got {} (full mir: {mir:?})",
                expected_forwarded,
                args.len()
            );
        }
        other => panic!(
            "f.{method}(nullish thisArg, ...) must dispatch to MirExpr::Call with callee \
             FunctionId::from_raw(11), got {other:?}"
        ),
    }
}
