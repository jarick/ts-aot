use std::collections::HashMap;

use ts_aot_core::{MAX_DENSE_ARRAY_LEN, Span, StructId, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::array_from_object_literal;

impl ExprConverter {
    pub(super) fn convert_call(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        type_args: &[TypeId],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let prev_type_args = std::mem::take(&mut self.current_call_type_args);
        self.current_call_type_args = type_args.to_vec();
        let result = self.convert_call_inner(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        self.current_call_type_args = prev_type_args;
        result
    }

    fn convert_call_inner(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let Some(info) = array_from_object_literal(callee, args) {
            let length = info.length;
            let indexed = info.indexed;
            if length < 0 || length > i128::from(MAX_DENSE_ARRAY_LEN) {
                ctx.error(
                    "E0406",
                    format!(
                        "Array.from({{length: N}}) requires 0 <= N < {} (AOT dense-Vec cap); got {}",
                        MAX_DENSE_ARRAY_LEN, length
                    ),
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if args.len() > 1 && !indexed.is_empty() {
                ctx.error(
                    "E0406",
                    "Array.from({length: N, 0: x, ...}, mapFn) (mixing indexed values with mapFn) \
                     is not supported in this PR; either drop the indexed values or drop the mapFn",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if args.len() > 1 && !matches!(&args[1], HirExpr::Global { .. }) {
                ctx.error(
                    "E0406",
                    "Array.from({length: N}, mapFn[, thisArg]) in AOT requires mapFn to be a known global function reference \
                     (a top-level fn or a non-capturing closure lifted by `lower_closures`); \
                     capturing closures and arbitrary local expressions are not supported",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if args.len() == 3 {
                ctx.error(
                    "E0406",
                    "Array.from({length: N}, mapFn, thisArg) does not support thisArg in AOT \
                     (AOT closures have no `this` binding); use the 2-arg form Array.from({length: N}, mapFn)",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                args: vec![MirExpr::Int {
                    value: length,
                    ty: TypeId::from_raw(0),
                }],
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            for (idx, value_hir) in &indexed {
                if i128::from(*idx) >= length {
                    continue;
                }
                let value_mir = self.convert_expr(
                    value_hir,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArraySet,
                    args: vec![
                        MirExpr::Local(dest),
                        MirExpr::Int {
                            value: i128::from(*idx),
                            ty: TypeId::from_raw(0),
                        },
                        value_mir,
                    ],
                    dest: None,
                    ty: TypeId::from_raw(0),
                    target_ty: None,
                });
            }
            if args.len() == 1 {
                return MirExpr::Local(dest);
            }
            let HirExpr::Global {
                name: mapfn_name, ..
            } = &args[1]
            else {
                unreachable!("mapFn shape was pre-validated above; this branch is unreachable")
            };
            let mapfn_mir = MirExpr::Global(mapfn_name.clone());
            let final_dest = self.fresh_local();
            self.push_temp_local(final_dest, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayFromLengthMapped,
                args: vec![
                    MirExpr::Int {
                        value: length,
                        ty: TypeId::from_raw(0),
                    },
                    mapfn_mir,
                ],
                dest: Some(final_dest),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(final_dest);
        }
        if let Some(mir) = self.try_string_instance_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_function_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_date_static_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_date_instance_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_array_buffer_instance_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_json_static_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_symbol_global_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        if let Some(mir) = self.try_symbol_static_method_dispatch(
            callee,
            args,
            ty,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return mir;
        }
        let callee_id = self.resolve_callee(callee, ctx);
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        if callee_id == PLACEHOLDER_FUNCTION
            && let HirCallee::Indirect(inner) = callee
        {
            if let Some(mir) = self.try_known_indirect_dispatch(
                callee,
                args,
                mir_args.clone(),
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ) {
                return mir;
            }
            let callee_value = self.convert_expr(
                inner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            return MirExpr::IndirectCall {
                callee: Box::new(callee_value),
                args: mir_args,
                ty,
            };
        }
        MirExpr::Call {
            callee: callee_id,
            args: mir_args,
            ty,
        }
    }
}
