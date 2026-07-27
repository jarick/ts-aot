#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

pub(super) fn is_global_object_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Object")
}

impl ExprConverter {
    pub(super) fn convert_call(
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
        let callee_id = self.resolve_callee(callee, ctx);
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        if callee_id == PLACEHOLDER_FUNCTION
            && let HirCallee::Indirect(inner) = callee
        {
            if let Some(callee_ty) = hir_expr_type_id(inner.as_ref())
                && let Some(Type::Fn { .. }) = types.resolve(callee_ty)
            {
                ctx.error(
                    "E0405",
                    "function-typed value cannot be called in Phase 4 — \
                     Type::Fn lowers to `()` and `()` is not callable. \
                     Use a named function declaration or call through a known callee instead.",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if let HirExpr::Field {
                owner, field_name, ..
            } = inner.as_ref()
                && is_global_object_reference(owner)
                && matches!(
                    field_name.as_str(),
                    "getPrototypeOf" | "keys" | "setPrototypeOf"
                )
            {
                if field_name.as_str() == "setPrototypeOf" {
                    if mir_args.is_empty() {
                        ctx.error(
                            "E0406",
                            "Object.setPrototypeOf requires at least the receiver arg; got empty arg list",
                            Span::new(0, 0),
                        );
                        return MirExpr::Unit;
                    }
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, ty);
                    out.push(MirStmt::Let {
                        local: dest,
                        ty,
                        init: None,
                        mutable: false,
                    });
                    out.push(MirStmt::Assign {
                        target: MirPlace::Local { id: dest },
                        value: mir_args
                            .into_iter()
                            .next()
                            .expect("checked non-empty above"),
                    });
                    return MirExpr::Local(dest);
                }
                if mir_args.len() != 1 {
                    ctx.error(
                        "E0406",
                        format!(
                            "Object.{} requires exactly 1 argument; got {}",
                            field_name.as_str(),
                            mir_args.len()
                        ),
                        Span::new(0, 0),
                    );
                    return MirExpr::Unit;
                }
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op: match field_name.as_str() {
                        "keys" => RuntimeOp::ObjectKeys,
                        "getPrototypeOf" => RuntimeOp::ObjectGetPrototypeOf,
                        _ => unreachable!("setPrototypeOf handled above"),
                    },
                    args: mir_args,
                    dest: Some(dest),
                    ty,
                });
                return MirExpr::Local(dest);
            }
            if let HirExpr::Field {
                owner: has_own_owner,
                field_name: has_own_field,
                ..
            } = inner.as_ref()
                && has_own_field.as_str() == "hasOwnProperty"
                && args.len() == 1
                && let Some(ty) = hir_expr_type_id(has_own_owner.as_ref())
                && let Some(&sid) = self
                    .struct_ids
                    .get(&ty)
                    .or_else(|| shared_struct_ids.get(&ty))
            {
                if let HirExpr::String(key_atom, _) = &args[0] {
                    let key = key_atom.clone();
                    return MirExpr::Bool(self.field_id_lookup.contains_key(&(sid, key)));
                }
                ctx.error(
                    "E0406",
                    "obj.hasOwnProperty() in AOT requires a literal string key when receiver is a struct; \
                     dynamic keys on struct receivers are not supported",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
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
