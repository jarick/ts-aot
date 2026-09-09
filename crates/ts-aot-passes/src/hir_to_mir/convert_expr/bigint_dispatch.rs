use std::collections::HashMap;

use ts_aot_core::{StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(super) fn try_bigint_new_dispatch(
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
        let HirExpr::Global { name, .. } = inner.as_ref() else {
            return None;
        };
        if name.as_str() != "BigInt" {
            return None;
        }
        if args.len() != 1 {
            ctx.error(
                "P0005",
                format!(
                    "BigInt(x) constructor takes exactly one argument, got {}",
                    args.len()
                ),
                inner.span(),
            );
            return Some(MirExpr::Unit);
        }
        let arg_ty = args[0].ty();
        if !matches!(types.resolve(arg_ty), Some(Type::String)) {
            ctx.error(
                "P0005",
                format!(
                    "BigInt(x) currently only supports string arguments; got {:?}. \
                     BigInt(number) is planned for a follow-up PR (per spec, BigInt(2.5) must \
                     throw RangeError for non-integer, and BigInt(2) must round to BigInt; the \
                     current dispatch enforces string-only to keep the emit sound)",
                    types.resolve(arg_ty)
                ),
                args[0].span(),
            );
            return Some(MirExpr::Unit);
        }
        let arg_mir = self.convert_expr(
            &args[0],
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let bigint_ty = types.intern(&Type::BigInt);
        let dest = self.fresh_local();
        self.push_temp_local(dest, bigint_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::BigIntNew,
            args: vec![arg_mir],
            dest: Some(dest),
            ty: bigint_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(super) fn try_bigint_to_string_dispatch(
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
            owner, field_name, ..
        } = inner.as_ref()
        else {
            return None;
        };
        if field_name.as_str() != "toString" {
            return None;
        }
        let owner_ty = owner.ty();
        if !matches!(types.resolve(owner_ty), Some(Type::BigInt)) {
            return None;
        }
        if !args.is_empty() {
            ctx.error(
                "P0005",
                format!(
                    "BigInt.prototype.toString() takes no arguments, got {}",
                    args.len()
                ),
                inner.span(),
            );
            return Some(MirExpr::Unit);
        }
        let owner_mir = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let string_ty = types.intern(&Type::String);
        let dest = self.fresh_local();
        self.push_temp_local(dest, string_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::BigIntToString,
            args: vec![owner_mir],
            dest: Some(dest),
            ty: string_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(super) fn try_bigint_to_number_dispatch(
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
        let HirExpr::Global { name, .. } = inner.as_ref() else {
            return None;
        };
        if name.as_str() != "Number" {
            return None;
        }
        if args.len() != 1 {
            ctx.error(
                "P0005",
                format!(
                    "Number(x) constructor takes exactly one argument, got {}",
                    args.len()
                ),
                inner.span(),
            );
            return Some(MirExpr::Unit);
        }
        let arg_ty = args[0].ty();
        if !matches!(types.resolve(arg_ty), Some(Type::BigInt)) {
            return None;
        }
        let arg_mir = self.convert_expr(
            &args[0],
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let number_ty = types.intern(&Type::F64);
        let dest = self.fresh_local();
        self.push_temp_local(dest, number_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::BigIntToNumber,
            args: vec![arg_mir],
            dest: Some(dest),
            ty: number_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
