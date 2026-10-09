use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

use super::util::hir_expr_type_id;

/// Result type of an array `length`/`len` access: `i64`, or `i64 | undefined`
/// when the receiver is itself optional (`arr?.length`).
fn array_len_result_ty(types: &mut TypeTable, is_optional: bool) -> TypeId {
    let i64_ty = types.intern(&Type::I64);
    if is_optional {
        types.intern(&Type::Optional { inner: i64_ty })
    } else {
        i64_ty
    }
}

/// Classifies an array-length receiver: `Some(true)` for an optional array
/// (`arr?.length`), `Some(false)` for a plain array, `None` otherwise. The
/// optional receiver is kept optional so the backend can short-circuit.
fn array_receiver_is_optional(owner: &HirExpr, types: &TypeTable) -> Option<bool> {
    let owner_ty = hir_expr_type_id(owner)?;
    match types.resolve(owner_ty) {
        Some(Type::Optional { inner }) => {
            matches!(types.resolve(*inner), Some(Type::Array { .. })).then_some(true)
        }
        _ => matches!(types.resolve(owner_ty), Some(Type::Array { .. })).then_some(false),
    }
}

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_array_len_property_dispatch(
        &mut self,
        owner: &HirExpr,
        field_name: &str,
        _ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if field_name != "len" && field_name != "length" {
            return None;
        }
        let is_optional = array_receiver_is_optional(owner, types)?;
        let receiver_mir = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let result_ty = array_len_result_ty(types, is_optional);
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayLen,
            args: vec![receiver_mir],
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_array_instance_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        _ty: TypeId,
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
            owner: receiver,
            field_name,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if field_name.as_str() != "len" && field_name.as_str() != "length" {
            return None;
        }
        let is_optional = array_receiver_is_optional(receiver, types)?;
        if !args.is_empty() {
            ctx.error(
                "E0406",
                format!(
                    "Array.{}() requires no arguments; got {}",
                    field_name.as_str(),
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let receiver_mir = self.convert_expr(
            receiver,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let result_ty = array_len_result_ty(types, is_optional);
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayLen,
            args: vec![receiver_mir],
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
