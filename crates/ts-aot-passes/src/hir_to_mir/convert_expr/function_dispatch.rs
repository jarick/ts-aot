use std::collections::HashMap;

use ts_aot_core::{StructId, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt};

use crate::PassContext;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_function_method_dispatch(
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
        let inner = inner.as_ref();
        let HirExpr::Field {
            owner,
            field_name,
            span,
            ..
        } = inner
        else {
            return None;
        };
        let call_span = *span;
        let HirExpr::Global { name: fn_name, .. } = owner.as_ref() else {
            return None;
        };
        let field = field_name.as_str();
        if !matches!(field, "call" | "apply" | "bind") {
            return None;
        }
        let fid = self.name_to_function.get(fn_name).copied()?;
        if field == "bind" {
            ctx.error(
                "E0406",
                format!(
                    "Function.prototype.bind on `{}` is not yet supported in this AOT target; \
                     partial application requires synthesized closures with captured bound args. \
                     Use an explicit closure `(...rest) => {}(thisArg, ...bound, ...rest)` \
                     instead.",
                    fn_name.as_str(),
                    fn_name.as_str(),
                ),
                call_span,
            );
            return Some(MirExpr::Unit);
        }
        if args.is_empty() {
            ctx.error(
                "E0406",
                format!(
                    "Function.prototype.{} requires at least the thisArg; got empty arg list",
                    field
                ),
                call_span,
            );
            return Some(MirExpr::Unit);
        }
        if !matches!(
            &args[0],
            HirExpr::Unit(_) | HirExpr::Null(_) | HirExpr::Undefined(_)
        ) {
            ctx.error(
                "E0406",
                format!(
                    "Function.prototype.{} does not support non-nullish thisArg in AOT \
                     (AOT closures have no `this` binding); pass undefined, null, or the \
                     void expression (the three HIR nullish forms Unit/Null/Undefined) as the thisArg",
                    field
                ),
                call_span,
            );
            return Some(MirExpr::Unit);
        }
        match field {
            "call" => {
                let mir_args: Vec<MirExpr> = args[1..]
                    .iter()
                    .map(|a| {
                        self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx)
                    })
                    .collect();
                Some(MirExpr::Call {
                    callee: fid,
                    args: mir_args,
                    ty,
                })
            }
            "apply" => {
                if args.len() != 2 {
                    ctx.error(
                        "E0406",
                        format!(
                            "Function.prototype.apply in AOT requires exactly 2 args \
                             (thisArg, array); got {}",
                            args.len()
                        ),
                        call_span,
                    );
                    return Some(MirExpr::Unit);
                }
                let HirExpr::ArrayLiteral { elements, .. } = &args[1] else {
                    ctx.error(
                        "E0406",
                        "Function.prototype.apply in AOT requires a literal array argument; \
                         dynamic-array spread is not yet supported (synthesized closure would \
                         be needed to forward runtime-computed args).",
                        call_span,
                    );
                    return Some(MirExpr::Unit);
                };
                let mir_args: Vec<MirExpr> = elements
                    .iter()
                    .map(|a| {
                        self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx)
                    })
                    .collect();
                Some(MirExpr::Call {
                    callee: fid,
                    args: mir_args,
                    ty,
                })
            }
            _ => unreachable!("field was filtered to `call`|`apply` above; `bind` handled earlier"),
        }
    }
}
