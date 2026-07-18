use ts_aot_core::TypeTable;
use ts_aot_ir_hir::{HirCallee, HirExpr};

use super::substitute::{TypeParamMap, TypeSubstitutionResult};
use super::substitute_decl::substitute_body;
use super::substitute_ty::{substitute_param, substitute_type};

pub fn substitute_expr(
    expr: &HirExpr,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirExpr {
    match expr {
        HirExpr::Unit
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Null
        | HirExpr::Undefined => expr.clone(),
        HirExpr::Local { id, ty } => HirExpr::Local {
            id: *id,
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Global { name, ty } => HirExpr::Global {
            name: name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Field {
            owner,
            field,
            field_name,
            ty,
        } => HirExpr::Field {
            owner: Box::new(substitute_expr(owner, mapping, types, result)),
            field: *field,
            field_name: field_name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Index { owner, index, ty } => HirExpr::Index {
            owner: Box::new(substitute_expr(owner, mapping, types, result)),
            index: Box::new(substitute_expr(index, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Call { callee, args, ty } => HirExpr::Call {
            callee: substitute_callee(callee, mapping, types, result),
            args: args
                .iter()
                .map(|a| substitute_expr(a, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Binary { op, lhs, rhs, ty } => HirExpr::Binary {
            op: *op,
            lhs: Box::new(substitute_expr(lhs, mapping, types, result)),
            rhs: Box::new(substitute_expr(rhs, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Unary { op, expr, ty } => HirExpr::Unary {
            op: *op,
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::StructLiteral { ty, fields } => HirExpr::StructLiteral {
            ty: substitute_type(*ty, mapping, types, result),
            fields: fields
                .iter()
                .map(|(fid, v)| (*fid, substitute_expr(v, mapping, types, result)))
                .collect(),
        },
        HirExpr::ArrayLiteral { elements, ty } => HirExpr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|e| substitute_expr(e, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Closure {
            id,
            params,
            captures,
            body,
            ty,
        } => HirExpr::Closure {
            id: *id,
            params: params
                .iter()
                .map(|p| substitute_param(p, mapping, types, result))
                .collect(),
            captures: captures
                .iter()
                .map(|c| substitute_expr(c, mapping, types, result))
                .collect(),
            body: substitute_body(body, mapping, types, result),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Await { expr, ty } => HirExpr::Await {
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Yield { expr, ty } => HirExpr::Yield {
            expr: expr
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, mapping, types, result))),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::Template {
            tag,
            expressions,
            cooked_parts,
            raw_parts,
            ty,
        } => HirExpr::Template {
            tag: tag
                .as_ref()
                .map(|t| Box::new(substitute_expr(t, mapping, types, result))),
            expressions: expressions
                .iter()
                .map(|p| substitute_expr(p, mapping, types, result))
                .collect(),
            cooked_parts: cooked_parts.clone(),
            raw_parts: raw_parts.clone(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::New { callee, args, ty } => HirExpr::New {
            callee: Box::new(substitute_expr(callee, mapping, types, result)),
            args: args
                .iter()
                .map(|a| substitute_expr(a, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::OptionalChain { base, ty } => HirExpr::OptionalChain {
            base: Box::new(substitute_expr(base, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::TypeAssertion { expr, target } => HirExpr::TypeAssertion {
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            target: substitute_type(*target, mapping, types, result),
        },
        HirExpr::Assignment { target, value, ty } => HirExpr::Assignment {
            target: Box::new(substitute_expr(target, mapping, types, result)),
            value: Box::new(substitute_expr(value, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::CompoundUpdate {
            target,
            op,
            rhs,
            post,
            ty,
        } => HirExpr::CompoundUpdate {
            target: Box::new(substitute_expr(target, mapping, types, result)),
            op: *op,
            rhs: Box::new(substitute_expr(rhs, mapping, types, result)),
            post: *post,
            ty: substitute_type(*ty, mapping, types, result),
        },
    }
}

pub fn substitute_callee(
    callee: &HirCallee,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirCallee {
    match callee {
        HirCallee::Function(id) => HirCallee::Function(*id),
        HirCallee::Indirect(expr) => {
            HirCallee::Indirect(Box::new(substitute_expr(expr, mapping, types, result)))
        }
        HirCallee::Closure(id) => HirCallee::Closure(*id),
        HirCallee::Runtime { name, ty } => HirCallee::Runtime {
            name: name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
        },
    }
}
