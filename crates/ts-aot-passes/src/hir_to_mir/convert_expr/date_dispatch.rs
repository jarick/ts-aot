use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::{is_global_date_reference, is_string_typed_source};

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_date_static_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner: date_owner,
            field_name: date_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if !is_global_date_reference(date_owner) {
            return None;
        }
        let (op, expected_arity) = match date_field.as_str() {
            "now" => (RuntimeOp::DateNow, 0usize),
            "parse" => (RuntimeOp::DateParse, 1usize),
            _ => return None,
        };
        if args.len() != expected_arity {
            ctx.error(
                "E0406",
                format!(
                    "Date.{} requires exactly {} argument(s); got {}",
                    date_field.as_str(),
                    expected_arity,
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if date_field.as_str() == "parse" && !is_string_typed_source(&args[0], types) {
            ctx.error(
                "E0406",
                "Date.parse() argument must be a string (per ECMAScript spec); got non-string \
                 expression. The runtime call __ts_aot_date_parse was not emitted. Coerce the \
                 argument to a string (e.g. String(n)) before calling Date.parse()."
                    .to_string(),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_date_instance_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner: date_method_owner,
            field_name: date_method_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        let owner_ty = hir_expr_type_id(date_method_owner)?;
        if !matches!(types.resolve(owner_ty), Some(Type::Date)) {
            return None;
        }
        let op = match date_method_field.as_str() {
            "getTime" => Some(RuntimeOp::DateGetTime),
            "valueOf" => Some(RuntimeOp::DateValueOf),
            "getFullYear" => Some(RuntimeOp::DateGetFullYear),
            "getMonth" => Some(RuntimeOp::DateGetMonth),
            "getDate" => Some(RuntimeOp::DateGetDate),
            "getHours" => Some(RuntimeOp::DateGetHours),
            "getMinutes" => Some(RuntimeOp::DateGetMinutes),
            "getSeconds" => Some(RuntimeOp::DateGetSeconds),
            "getMilliseconds" => Some(RuntimeOp::DateGetMilliseconds),
            "toISOString" => Some(RuntimeOp::DateToIsoString),
            _ => None,
        }?;
        if !args.is_empty() {
            ctx.error(
                "E0406",
                format!(
                    "Date.prototype.{} requires 0 explicit arguments (the receiver is the sole \
                     argument passed to the Date runtime call); got {} extra argument(s)",
                    date_method_field.as_str(),
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let receiver_mir = self.convert_expr(
            date_method_owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: vec![receiver_mir],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
