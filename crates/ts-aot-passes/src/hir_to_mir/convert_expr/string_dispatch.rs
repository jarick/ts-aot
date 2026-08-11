use std::collections::HashMap;

use ts_aot_core::{Span, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::HirCallee;
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::is_string_typed_source;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_string_instance_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[ts_aot_ir_hir::HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        use ts_aot_ir_hir::HirExpr;
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner: method_owner,
            field_name: method_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if !is_string_typed_source(method_owner, types) {
            return None;
        }
        let op = match method_field.as_str() {
            "indexOf" => Some(RuntimeOp::StringIndexOf),
            "charAt" => Some(RuntimeOp::StringCharAt),
            _ => None,
        }?;
        let (min_arity, max_arity) = match op {
            RuntimeOp::StringIndexOf => (1, 2),
            RuntimeOp::StringCharAt => (1, 1),
            _ => unreachable!("string method arity"),
        };
        if args.len() < min_arity || args.len() > max_arity {
            ctx.error(
                "E0406",
                format!(
                    "String.prototype.{} requires {}..={} argument(s); got {}",
                    method_field.as_str(),
                    min_arity,
                    max_arity,
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let receiver_mir = self.convert_expr(
            method_owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let converted_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let mut full_args = Vec::with_capacity(1 + max_arity);
        full_args.push(receiver_mir);
        full_args.extend(converted_args);
        if op == RuntimeOp::StringIndexOf && full_args.len() == 2 {
            full_args.push(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            });
        }
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: full_args,
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
