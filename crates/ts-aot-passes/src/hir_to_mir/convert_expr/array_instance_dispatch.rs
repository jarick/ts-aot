use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

use super::util::hir_expr_type_id;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_array_len_property_dispatch(
        &mut self,
        owner: &HirExpr,
        field_name: &str,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if field_name != "len" && field_name != "length" {
            return None;
        }
        let owner_ty = hir_expr_type_id(owner)?;
        let owner_ty = match types.resolve(owner_ty) {
            Some(Type::Optional { inner }) => *inner,
            _ => owner_ty,
        };
        if !matches!(types.resolve(owner_ty), Some(Type::Array { .. })) {
            return None;
        }
        let receiver_mir = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayLen,
            args: vec![receiver_mir],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_array_instance_method_dispatch(
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
            owner: receiver,
            field_name,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        let owner_ty = hir_expr_type_id(receiver)?;
        let owner_ty = match types.resolve(owner_ty) {
            Some(Type::Optional { inner }) => *inner,
            _ => owner_ty,
        };
        if !matches!(types.resolve(owner_ty), Some(Type::Array { .. })) {
            return None;
        }
        if field_name.as_str() != "len" && field_name.as_str() != "length" {
            return None;
        }
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
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayLen,
            args: vec![receiver_mir],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
