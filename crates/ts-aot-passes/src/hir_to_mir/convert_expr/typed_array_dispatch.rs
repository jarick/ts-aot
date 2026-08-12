use std::collections::HashMap;

use ts_aot_core::{Span, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::globals::is_global_typed_array_reference;
use crate::hir_to_mir::convert_expr::util::is_numeric_type_for_array_len;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_typed_array_new_dispatch(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if !is_global_typed_array_reference(callee) {
            return None;
        }
        let HirExpr::Global { name, .. } = callee else {
            return None;
        };
        let kind_id = typed_array_kind_id_from_name(name.as_str())?;
        Some(self.convert_new_typed_array(
            args,
            kind_id,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ))
    }

    fn convert_new_typed_array(
        &mut self,
        args: &[HirExpr],
        kind_id: i64,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if args.len() != 1 {
            ctx.error(
                "E0406",
                format!(
                    "new TypedArray requires exactly 1 argument (length) in MVP; \
                     got {} (typedArray/array/buffer constructor overloads are not yet supported)",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        }
        if !is_numeric_type_for_array_len(&args[0], types) {
            ctx.error(
                "E0406",
                "new TypedArray length argument must be a number (per ECMAScript spec)",
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        }
        let length_mir = self.convert_expr(
            &args[0],
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::TypedArrayNew,
            args: vec![
                length_mir,
                MirExpr::Int {
                    value: i128::from(kind_id),
                    ty: TypeId::from_raw(0),
                },
            ],
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }
}

fn typed_array_kind_id_from_name(name: &str) -> Option<i64> {
    match name {
        "Int8Array" => Some(0),
        "Uint8Array" => Some(1),
        "Uint8ClampedArray" => Some(2),
        "Int16Array" => Some(3),
        "Uint16Array" => Some(4),
        "Int32Array" => Some(5),
        "Uint32Array" => Some(6),
        "Float32Array" => Some(7),
        "Float64Array" => Some(8),
        _ => None,
    }
}
