use ts_aot_core::TypeTable;
use ts_aot_ir_hir::{HirCallee, HirExpr};

use super::substitute::{TypeParamMap, TypeSubstitutionResult};
use super::substitute_decl::substitute_body;
use super::substitute_ty::{substitute_param, substitute_type};
use ts_aot_ir_hir::ObjectLiteralField;

pub fn substitute_expr(
    expr: &HirExpr,
    mapping: &TypeParamMap,
    types: &mut TypeTable,
    result: &mut TypeSubstitutionResult,
) -> HirExpr {
    match expr {
        HirExpr::Unit(_)
        | HirExpr::Bool(_, _)
        | HirExpr::Int(_, _)
        | HirExpr::Float(_, _)
        | HirExpr::String(_, _)
        | HirExpr::Null(_)
        | HirExpr::Undefined(_) => expr.clone(),
        HirExpr::RegExp {
            pattern,
            flags,
            ty,
            span,
        } => HirExpr::RegExp {
            span: *span,
            pattern: pattern.clone(),
            flags: flags.clone(),
            ty: substitute_type(*ty, mapping, types, result),
        },
        HirExpr::BigInt { value, ty, span } => HirExpr::BigInt {
            value: value.clone(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Import { source, ty, span } => HirExpr::Import {
            source: Box::new(substitute_expr(source, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Local { id, ty, span } => HirExpr::Local {
            id: *id,
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Global { name, ty, span } => HirExpr::Global {
            name: name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Field {
            owner,
            field,
            field_name,
            ty,
            span,
        } => HirExpr::Field {
            owner: Box::new(substitute_expr(owner, mapping, types, result)),
            field: *field,
            field_name: field_name.clone(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Index {
            owner,
            index,
            ty,
            span,
        } => HirExpr::Index {
            owner: Box::new(substitute_expr(owner, mapping, types, result)),
            index: Box::new(substitute_expr(index, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Call {
            callee,
            args,
            ty,
            span,
        } => HirExpr::Call {
            callee: substitute_callee(callee, mapping, types, result),
            args: args
                .iter()
                .map(|a| substitute_expr(a, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Binary {
            op,
            lhs,
            rhs,
            ty,
            span,
        } => HirExpr::Binary {
            op: *op,
            lhs: Box::new(substitute_expr(lhs, mapping, types, result)),
            rhs: Box::new(substitute_expr(rhs, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Unary { op, expr, ty, span } => HirExpr::Unary {
            op: *op,
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::StructLiteral { ty, fields, span } => HirExpr::StructLiteral {
            ty: substitute_type(*ty, mapping, types, result),
            fields: fields
                .iter()
                .map(|(fid, v)| (*fid, substitute_expr(v, mapping, types, result)))
                .collect(),
            span: *span,
        },
        HirExpr::ArrayLiteral { elements, ty, span } => HirExpr::ArrayLiteral {
            elements: elements
                .iter()
                .map(|e| substitute_expr(e, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::ObjectLiteral { fields, ty, span } => HirExpr::ObjectLiteral {
            fields: fields
                .iter()
                .map(|f| match f {
                    ObjectLiteralField::Property { name, value } => ObjectLiteralField::Property {
                        name: name.clone(),
                        value: substitute_expr(value, mapping, types, result),
                    },
                    ObjectLiteralField::Spread(value) => {
                        ObjectLiteralField::Spread(substitute_expr(value, mapping, types, result))
                    }
                })
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Ternary {
            cond,
            then_branch,
            else_branch,
            ty,
            span,
        } => HirExpr::Ternary {
            cond: Box::new(substitute_expr(cond, mapping, types, result)),
            then_branch: Box::new(substitute_expr(then_branch, mapping, types, result)),
            else_branch: Box::new(substitute_expr(else_branch, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Sequence { exprs, ty, span } => HirExpr::Sequence {
            exprs: exprs
                .iter()
                .map(|e| substitute_expr(e, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Closure {
            id,
            params,
            captures,
            body,
            ty,
            span,
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
            span: *span,
        },
        HirExpr::Await { expr, ty, span } => HirExpr::Await {
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Yield { expr, ty, span } => HirExpr::Yield {
            expr: expr
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, mapping, types, result))),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::Template {
            tag,
            expressions,
            cooked_parts,
            raw_parts,
            ty,
            span,
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
            span: *span,
        },
        HirExpr::New {
            callee,
            args,
            ty,
            span,
        } => HirExpr::New {
            callee: Box::new(substitute_expr(callee, mapping, types, result)),
            args: args
                .iter()
                .map(|a| substitute_expr(a, mapping, types, result))
                .collect(),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::OptionalChain { base, ty, span } => HirExpr::OptionalChain {
            base: Box::new(substitute_expr(base, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::TypeAssertion { expr, target, span } => HirExpr::TypeAssertion {
            expr: Box::new(substitute_expr(expr, mapping, types, result)),
            target: substitute_type(*target, mapping, types, result),
            span: *span,
        },
        HirExpr::Assignment {
            target,
            value,
            ty,
            span,
        } => HirExpr::Assignment {
            target: Box::new(substitute_expr(target, mapping, types, result)),
            value: Box::new(substitute_expr(value, mapping, types, result)),
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
        },
        HirExpr::CompoundUpdate {
            target,
            op,
            rhs,
            post,
            ty,
            span,
        } => HirExpr::CompoundUpdate {
            target: Box::new(substitute_expr(target, mapping, types, result)),
            op: *op,
            rhs: Box::new(substitute_expr(rhs, mapping, types, result)),
            post: *post,
            ty: substitute_type(*ty, mapping, types, result),
            span: *span,
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
