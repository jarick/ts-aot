use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::{has_potential_side_effects, hir_expr_type_id};
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::{
    is_array_static_type, is_global_array_reference, is_global_math_reference,
    is_global_object_reference, is_global_string_reference, is_string_typed_source,
};

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_known_indirect_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        mir_args: Vec<MirExpr>,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        _shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
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
            return Some(MirExpr::Unit);
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
                    return Some(MirExpr::Unit);
                }
                let mut mir_args_iter = mir_args.into_iter();
                let target_arg = mir_args_iter.next().expect("checked non-empty above");
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
                    value: target_arg,
                });
                for arg in mir_args_iter {
                    if has_potential_side_effects(&arg) {
                        out.push(MirStmt::Expr(arg));
                    }
                }
                return Some(MirExpr::Local(dest));
            }
            if field_name.as_str() == "getPrototypeOf" {
                ctx.error(
                    "E0406",
                    "Object.getPrototypeOf is not supported in this AOT target; \
                     prototype chain resolution requires type-tracked receivers and is \
                     tracked as a separate architectural change. Use Object.keys() for \
                     enumerable-property iteration, or drop the call.",
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
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
                return Some(MirExpr::Unit);
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ObjectKeys,
                args: mir_args,
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return Some(MirExpr::Local(dest));
        }
        if let HirExpr::Field {
            owner: array_owner,
            field_name: array_field,
            ..
        } = inner.as_ref()
            && is_global_array_reference(array_owner)
            && array_field.as_str() == "isArray"
        {
            if mir_args.len() != 1 {
                ctx.error(
                    "E0406",
                    format!(
                        "Array.isArray requires exactly 1 argument; got {}",
                        mir_args.len()
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            let op = if is_array_static_type(&args[0], types) {
                RuntimeOp::ArrayIsArray
            } else {
                RuntimeOp::ArrayIsArrayFalse
            };
            out.push(MirStmt::Runtime {
                op,
                args: mir_args,
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return Some(MirExpr::Local(dest));
        }
        if let HirExpr::Field {
            owner: array_from_owner,
            field_name: array_from_field,
            ..
        } = inner.as_ref()
            && is_global_array_reference(array_from_owner)
            && array_from_field.as_str() == "from"
        {
            if mir_args.is_empty() || mir_args.len() > 3 {
                ctx.error(
                    "E0406",
                    format!(
                        "Array.from in AOT supports 1 (source), 2 (source, mapFn) or \
                         3 (source, mapFn, thisArg) arguments; got {}",
                        mir_args.len()
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            if mir_args.len() == 1 && is_string_typed_source(&args[0], types) {
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayFromString,
                    args: mir_args,
                    dest: Some(dest),
                    ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(dest));
            }
            if mir_args.len() >= 2 && !matches!(&args[1], HirExpr::Global { .. }) {
                ctx.error(
                    "E0406",
                    "Array.from(arr, mapFn[, thisArg]) in AOT requires mapFn to be a known global function reference \
                     (a top-level fn or a non-capturing closure lifted by `lower_closures`); \
                     capturing closures and arbitrary local expressions are not supported",
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
            if mir_args.len() == 3 {
                ctx.error(
                    "E0406",
                    "Array.from(arr, mapFn, thisArg) does not support thisArg in AOT \
                     (AOT closures have no `this` binding); use the 2-arg form Array.from(arr, mapFn)",
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
            let op = if mir_args.len() == 2 {
                RuntimeOp::ArrayFromMapped
            } else {
                RuntimeOp::ArrayFrom
            };
            out.push(MirStmt::Runtime {
                op,
                args: mir_args,
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return Some(MirExpr::Local(dest));
        }
        if let HirExpr::Field {
            owner: array_of_owner,
            field_name: array_of_field,
            ..
        } = inner.as_ref()
            && is_global_array_reference(array_of_owner)
            && array_of_field.as_str() == "of"
        {
            let alloc_id = self.fresh_local();
            self.push_temp_local(alloc_id, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                args: Vec::new(),
                dest: Some(alloc_id),
                ty,
                target_ty: None,
            });
            for item_mir in mir_args {
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    args: vec![MirExpr::Local(alloc_id), item_mir],
                    dest: None,
                    ty: TypeId::from_raw(0),
                    target_ty: None,
                });
            }
            return Some(MirExpr::Local(alloc_id));
        }
        if let HirExpr::Field {
            owner: math_owner,
            field_name: math_field,
            ..
        } = inner.as_ref()
            && is_global_math_reference(math_owner)
        {
            let op = match math_field.as_str() {
                "abs" => Some(RuntimeOp::MathAbs),
                "floor" => Some(RuntimeOp::MathFloor),
                "ceil" => Some(RuntimeOp::MathCeil),
                "round" => Some(RuntimeOp::MathRound),
                "trunc" => Some(RuntimeOp::MathTrunc),
                "sign" => Some(RuntimeOp::MathSign),
                "sqrt" => Some(RuntimeOp::MathSqrt),
                "pow" => Some(RuntimeOp::MathPow),
                "log" => Some(RuntimeOp::MathLog),
                "exp" => Some(RuntimeOp::MathExp),
                "sin" => Some(RuntimeOp::MathSin),
                "cos" => Some(RuntimeOp::MathCos),
                "tan" => Some(RuntimeOp::MathTan),
                "asin" => Some(RuntimeOp::MathAsin),
                "acos" => Some(RuntimeOp::MathAcos),
                "atan" => Some(RuntimeOp::MathAtan),
                "atan2" => Some(RuntimeOp::MathAtan2),
                "max" => Some(RuntimeOp::MathMax),
                "min" => Some(RuntimeOp::MathMin),
                "random" => Some(RuntimeOp::MathRandom),
                _ => None,
            };
            if let Some(op) = op {
                let expected_arity = match op {
                    RuntimeOp::MathRandom => Some(0),
                    RuntimeOp::MathPow | RuntimeOp::MathAtan2 => Some(2),
                    RuntimeOp::MathMax | RuntimeOp::MathMin => None,
                    _ => Some(1),
                };
                let arity_ok = match expected_arity {
                    Some(n) => mir_args.len() == n,
                    None => true,
                };
                if !arity_ok {
                    ctx.error(
                        "E0406",
                        format!(
                            "Math.{} requires exactly {} argument(s); got {}",
                            math_field.as_str(),
                            expected_arity.expect("checked above"),
                            mir_args.len()
                        ),
                        Span::new(0, 0),
                    );
                    return Some(MirExpr::Unit);
                }
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op,
                    args: mir_args,
                    dest: Some(dest),
                    ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(dest));
            }
        }
        if let HirExpr::Field {
            owner: string_owner,
            field_name: string_field,
            ..
        } = inner.as_ref()
            && is_global_string_reference(string_owner)
        {
            let op = match string_field.as_str() {
                "fromCharCode" => Some(RuntimeOp::StringFromCharCode),
                "fromCodePoint" => Some(RuntimeOp::StringFromCodePoint),
                _ => None,
            };
            if let Some(op) = op {
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                out.push(MirStmt::Runtime {
                    op,
                    args: mir_args,
                    dest: Some(dest),
                    ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(dest));
            }
        }
        if let HirExpr::Field {
            owner: has_own_owner,
            field_name: has_own_field,
            ..
        } = inner.as_ref()
            && has_own_field.as_str() == "hasOwnProperty"
            && mir_args.len() == 1
            && let Some(ty) = hir_expr_type_id(has_own_owner.as_ref())
            && let Some(&sid) = self
                .struct_ids
                .get(&ty)
                .or_else(|| shared_struct_ids.get(&ty))
        {
            let arg0 = &mir_args[0];
            if let MirExpr::String { id: key_atom, .. } = arg0 {
                let key = key_atom.clone();
                return Some(MirExpr::Bool(
                    self.field_id_lookup.contains_key(&(sid, key)),
                ));
            }
            ctx.error(
                "E0406",
                "obj.hasOwnProperty() in AOT requires a literal string key when receiver is a struct; \
                 dynamic keys on struct receivers are not supported",
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        None
    }
}
