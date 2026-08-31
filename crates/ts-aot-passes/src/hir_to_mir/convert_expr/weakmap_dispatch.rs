use std::collections::HashMap;

use ts_aot_core::{LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_weakmap_new_dispatch(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        _shared_struct_ids: &mut HashMap<TypeId, StructId>,
        _shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if !is_global_weakmap_reference(callee) {
            return None;
        }
        if !args.is_empty() {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap constructor takes 0 arguments in the current backend (iterable initializer not yet lowered to MIR); got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let (key_ty, value_ty) = weakmap_key_value_types(ty, types, ctx);
        let resolved_key = if key_ty.raw() == 0 {
            types.intern(&Type::Struct {
                id: StructId::from_raw(0),
            })
        } else {
            key_ty
        };
        let resolved_value = if value_ty.raw() == 0 {
            types.intern(&Type::I64)
        } else {
            value_ty
        };
        if !is_supported_weakmap_key(resolved_key, types) {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap constructor key must be an object; runtime tracks key liveness via Weak<()> bindings. Got key type {:?}",
                    types.resolve(resolved_key)
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if !is_supported_weakmap_value(resolved_value, types) {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap constructor value must be a resolved Type::I64; runtime helpers take i64 by value. Got value type {:?}",
                    types.resolve(resolved_value)
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let weak_map_ty = types.intern(&Type::WeakMap {
            key: resolved_key,
            value: resolved_value,
        });
        let dest = self.fresh_local();
        self.push_temp_local(dest, weak_map_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::WeakMapNew,
            args: Vec::new(),
            dest: Some(dest),
            ty: weak_map_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_weakmap_instance_method_dispatch(
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
            owner: wm_method_owner,
            field_name: wm_method_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        let owner_ty = hir_expr_type_id(wm_method_owner)?;
        let Type::WeakMap {
            key: owner_key,
            value: owner_value,
        } = types.resolve(owner_ty)?
        else {
            return None;
        };
        let owner_key = *owner_key;
        let owner_value = *owner_value;
        if !is_supported_weakmap_key(owner_key, types) {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap keys must be objects; runtime tracks key liveness via Weak<()> bindings. Got key type {:?}",
                    types.resolve(owner_key)
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if !is_supported_weakmap_value(owner_value, types) {
            let detail = if owner_value.raw() == 0 {
                "unresolved (placeholder TypeId 0)".to_string()
            } else {
                format!("{:?}", types.resolve(owner_value))
            };
            ctx.error(
                "E0406",
                format!(
                    "WeakMap value must be a resolved Type::I64; runtime helpers take i64 by value, so __ts_aot_weak_map_set rejects any value not proven to be i64. Got value {detail}."
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let (op, expected_arity) = match wm_method_field.as_str() {
            "set" => (RuntimeOp::WeakMapSet, 2usize),
            "get" => (RuntimeOp::WeakMapGet, 1usize),
            "has" => (RuntimeOp::WeakMapHas, 1usize),
            "delete" => (RuntimeOp::WeakMapDelete, 1usize),
            _ => return None,
        };
        if args.len() != expected_arity {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap.prototype.{} requires exactly {} argument(s); got {}",
                    wm_method_field.as_str(),
                    expected_arity,
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if hir_root_local_id_of(&args[0]).is_none() {
            ctx.error(
                "E0406",
                "WeakMap keys must be local variables; field access (a.b) and index access (a[i]) expressions are not supported as WeakMap keys because the runtime tracks key identity via a per-key Weak<()> binding allocated at the call site, which requires a local root. Struct literals, object literals, calls, and other non-local expressions are also not supported as WeakMap keys.".to_string(),
                args[0].span(),
            );
            return Some(MirExpr::Unit);
        }
        if let Some(arg_ty) = hir_expr_type_id(&args[0])
            && arg_ty != owner_key
        {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap key type mismatch; WeakMap key type is {:?}, got {:?}",
                    types.resolve(owner_key),
                    types.resolve(arg_ty)
                ),
                args[0].span(),
            );
            return Some(MirExpr::Unit);
        }
        if op == RuntimeOp::WeakMapSet
            && let Some(val_ty) = hir_expr_type_id(&args[1])
            && val_ty != owner_value
        {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap.set value type mismatch; WeakMap value type is {:?}, got {:?}",
                    types.resolve(owner_value),
                    types.resolve(val_ty)
                ),
                args[1].span(),
            );
            return Some(MirExpr::Unit);
        }
        let receiver_mir = self.convert_expr(
            wm_method_owner,
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
        let result_ty = match op {
            RuntimeOp::WeakMapSet => owner_ty,
            RuntimeOp::WeakMapHas | RuntimeOp::WeakMapDelete => types.intern(&Type::Bool),
            RuntimeOp::WeakMapGet => owner_value,
            _ => ty,
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty: result_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }
}

fn is_global_weakmap_reference(expr: &HirExpr) -> bool {
    matches!(expr, HirExpr::Global { name, .. } if name.as_str() == "WeakMap")
}

fn hir_root_local_id_of(expr: &HirExpr) -> Option<LocalId> {
    match expr {
        HirExpr::Local { id, .. } => Some(*id),
        _ => None,
    }
}

fn weakmap_key_value_types(
    ty: TypeId,
    types: &TypeTable,
    ctx: &mut PassContext,
) -> (TypeId, TypeId) {
    if let Some(Type::WeakMap { key, value }) = types.resolve(ty) {
        if !is_supported_weakmap_key(*key, types) {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap key must be an object (struct); runtime tracks key liveness via Weak<()> bindings. Got key type {:?}.",
                    types.resolve(*key)
                ),
                Span::new(0, 0),
            );
            return (TypeId::from_raw(0), TypeId::from_raw(0));
        }
        if !is_supported_weakmap_value_for_defaulting(*value, types) {
            ctx.error(
                "E0406",
                format!(
                    "WeakMap<K, V> value must be i64; runtime helpers in ts_aot_runtime::weakmap (e.g. __ts_aot_weak_map_set) take `value: i64` directly, so non-i64 types like string or bool cannot be lowered. Got value type {:?}.",
                    types.resolve(*value)
                ),
                Span::new(0, 0),
            );
            return (TypeId::from_raw(0), TypeId::from_raw(0));
        }
        return (*key, *value);
    }
    (TypeId::from_raw(0), TypeId::from_raw(0))
}

pub(crate) fn is_supported_weakmap_key(key: TypeId, types: &TypeTable) -> bool {
    if key.raw() == 0 {
        return true;
    }
    matches!(types.resolve(key), Some(Type::Struct { .. }))
}

fn is_supported_weakmap_value(value: TypeId, types: &TypeTable) -> bool {
    if value.raw() == 0 {
        return false;
    }
    matches!(types.resolve(value), Some(Type::I64))
}

fn is_supported_weakmap_value_for_defaulting(value: TypeId, types: &TypeTable) -> bool {
    if value.raw() == 0 {
        return true;
    }
    matches!(types.resolve(value), Some(Type::I64))
}
