use std::collections::HashMap;

use ts_aot_core::{StructId, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

mod array_buffer_dispatch;
mod binary;
mod call;
mod control;
mod date_dispatch;
mod extras;
mod fallback_dispatch;
mod function_dispatch;
mod globals;
mod json_dispatch;
mod literals;
mod place;
mod runtime_dispatch;
mod string_dispatch;
mod symbol_dispatch;
mod template;
mod typed_array_dispatch;
mod util;

impl ExprConverter {
    pub(super) fn convert_expr(
        &mut self,
        e: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        match e {
            HirExpr::Unit(_) => MirExpr::Unit,
            HirExpr::Bool(b, _) => MirExpr::Bool(*b),
            HirExpr::Int(v, _) => MirExpr::Int {
                value: i128::from(*v),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Float(bits, _) => MirExpr::Float {
                value: f64::from_bits(*bits),
                ty: TypeId::from_raw(0),
            },
            HirExpr::String(id, _) => MirExpr::String {
                id: id.clone(),
                ty: TypeId::from_raw(0),
            },
            HirExpr::Null(_) => MirExpr::Null {
                ty: TypeId::from_raw(0),
            },
            HirExpr::Undefined(_) => MirExpr::Unit,
            HirExpr::Local { id, .. } => self.map_local(*id),
            HirExpr::Global { name, .. } => MirExpr::Global(name.clone()),
            HirExpr::Field {
                owner,
                field,
                field_name,
                ty,
                ..
            } => {
                let resolved_field =
                    self.resolve_field_id(owner, field_name, *field, shared_struct_ids, ctx);
                MirExpr::Field {
                    base: Box::new(self.convert_expr(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )),
                    field: resolved_field,
                    ty: *ty,
                }
            }
            HirExpr::Index {
                owner, index, ty, ..
            } => MirExpr::Index {
                base: Box::new(self.convert_expr(
                    owner,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                index: Box::new(self.convert_expr(
                    index,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )),
                ty: *ty,
            },
            HirExpr::Call {
                callee,
                args,
                type_args,
                ty,
                ..
            } => self.convert_call(
                callee,
                args,
                type_args,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Binary {
                op, lhs, rhs, ty, ..
            } => self.convert_binary(
                *op,
                lhs,
                rhs,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Unary { op, expr, ty, .. } => self.convert_unary(
                *op,
                expr,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::StructLiteral { ty, fields, .. } => self.convert_struct_literal(
                *ty,
                fields,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::ObjectLiteral { .. } => self.convert_object_literal(ctx),
            HirExpr::Sequence { exprs, .. } => self.convert_sequence(
                exprs,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::RegExp {
                pattern, flags, ty, ..
            } => self.convert_regexp(pattern.as_str(), flags.as_str(), *ty),
            HirExpr::BigInt { value, ty, .. } => self.convert_bigint(value.as_str(), *ty),
            HirExpr::Import { source, ty, .. } => self.convert_import(
                source,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Ternary {
                cond,
                then_branch,
                else_branch,
                ty,
                ..
            } => self.convert_ternary(
                cond,
                then_branch,
                else_branch,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::ArrayLiteral { elements, ty, .. } => self.convert_array_literal(
                elements,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Closure { ty, .. } => self.convert_closure(*ty, ctx),
            HirExpr::Await { expr, ty, .. } => self.convert_await(
                expr,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Yield { expr, ty, .. } => self.convert_yield(
                expr.as_deref(),
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Template {
                tag,
                expressions,
                cooked_parts,
                ty,
                ..
            } => self.convert_template(
                tag.as_deref(),
                expressions,
                cooked_parts,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::New {
                callee, args, ty, ..
            } => self.convert_new(
                callee,
                args,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::OptionalChain { base, .. } => self.convert_optional_chain(
                base,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::TypeAssertion { expr, target, .. } => self.convert_type_assertion(
                expr,
                *target,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::Assignment {
                target, value, ty, ..
            } => self.convert_assignment(
                target,
                value,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
            HirExpr::CompoundUpdate {
                target,
                op,
                rhs,
                post,
                ty,
                ..
            } => self.convert_compound_update(
                target,
                *op,
                rhs,
                *post,
                *ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
        }
    }
}
