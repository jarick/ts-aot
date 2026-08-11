use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::{hir_expr_type_id, is_numeric_type_for_array_len};
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_array_buffer_instance_method_dispatch(
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
            owner: buf_method_owner,
            field_name: buf_method_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        let owner_ty = hir_expr_type_id(buf_method_owner)?;
        if !matches!(types.resolve(owner_ty), Some(Type::ArrayBuffer)) {
            return None;
        }
        let op = match buf_method_field.as_str() {
            "slice" => RuntimeOp::ArrayBufferSlice,
            _ => return None,
        };
        if args.len() > 2 {
            ctx.error(
                "E0406",
                format!(
                    "ArrayBuffer.prototype.{} requires at most 2 arguments (begin, end); got {}",
                    buf_method_field.as_str(),
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        for (idx, arg) in args.iter().enumerate() {
            if !is_numeric_type_for_array_len(arg, types) {
                ctx.error(
                    "E0406",
                    format!(
                        "ArrayBuffer.prototype.{} argument #{} ({} or end) must be a number (per \
                         ECMAScript spec); got non-numeric expression. Coerce to a number before \
                         calling buf.slice(...).",
                        buf_method_field.as_str(),
                        idx,
                        if idx == 0 { "begin" } else { "end" }
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
        }
        let receiver_mir = self.convert_expr(
            buf_method_owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let mut mir_args: Vec<MirExpr> = vec![receiver_mir];
        for arg in args {
            mir_args.push(self.convert_expr(
                arg,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        if mir_args.len() == 1 {
            mir_args.push(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            });
        }
        if mir_args.len() == 2 {
            mir_args.push(MirExpr::Int {
                value: i64::MAX as i128,
                ty: TypeId::from_raw(0),
            });
        }
        let array_buffer_ty = types.intern(&Type::ArrayBuffer);
        let dest = self.fresh_local();
        self.push_temp_local(dest, array_buffer_ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty: array_buffer_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}
