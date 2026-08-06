use std::collections::HashMap;

use ts_aot_core::{FieldId, Span, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(super) fn convert_struct_literal(
        &mut self,
        ty: TypeId,
        fields: &[(FieldId, HirExpr)],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let struct_id = self.lookup_or_alloc_struct_id(ty, shared_struct_ids, shared_next_struct);
        MirExpr::StructLiteral {
            struct_id,
            fields: fields
                .iter()
                .map(|(fid, e)| {
                    (
                        *fid,
                        self.convert_expr(
                            e,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        ),
                    )
                })
                .collect(),
            ty,
        }
    }

    pub(super) fn convert_object_literal(&mut self, ctx: &mut PassContext) -> MirExpr {
        ctx.error(
            "E0402",
            "object literals (`{}`) are not supported in strict AOT mode. \
             Use an explicit struct constructor or factory function instead.",
            Span::new(0, 0),
        );
        MirExpr::Unit
    }

    pub(super) fn convert_regexp(&self, pattern: &str, flags: &str, ty: TypeId) -> MirExpr {
        MirExpr::RegExp {
            pattern: pattern.to_owned(),
            flags: flags.to_owned(),
            ty,
        }
    }

    pub(super) fn convert_bigint(&self, value: &str, ty: TypeId) -> MirExpr {
        MirExpr::BigInt {
            value: value.to_owned(),
            ty,
        }
    }

    pub(super) fn convert_import(
        &mut self,
        source: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let mut sub_out = Vec::new();
        let source_mir = self.convert_expr(
            source,
            &mut sub_out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        out.extend(sub_out);
        MirExpr::Import {
            source: Box::new(source_mir),
            ty,
        }
    }

    pub(super) fn convert_array_literal(
        &mut self,
        elements: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let args: Vec<MirExpr> = elements
            .iter()
            .map(|e| self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayCreate,
            args,
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }
}
