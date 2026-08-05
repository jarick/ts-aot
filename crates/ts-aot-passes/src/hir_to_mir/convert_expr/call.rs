use std::collections::HashMap;

use ts_aot_core::{
    MAX_DENSE_ARRAY_LEN, Span, StructId, Type, TypeId, TypeTable, canonical_integer_index,
};
use ts_aot_ir_hir::{HirCallee, HirExpr, ObjectLiteralField};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

pub(super) fn is_global_object_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Object")
}

pub(super) fn is_global_array_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Array")
}

pub(super) fn is_global_math_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Math")
}

pub(super) fn is_global_string_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "String")
}

fn is_string_typed_source(arg: &HirExpr, types: &TypeTable) -> bool {
    if matches!(arg, HirExpr::String(_, _)) {
        return true;
    }
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(types.resolve(ty), Some(Type::String))
}

fn is_array_static_type(arg: &HirExpr, types: &TypeTable) -> bool {
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(types.resolve(ty), Some(Type::Array { .. }))
}

struct ArrayLikeObjectLiteral {
    length: i128,
    indexed: Vec<(u64, HirExpr)>,
}

fn array_from_object_literal(
    callee: &HirCallee,
    args: &[HirExpr],
) -> Option<ArrayLikeObjectLiteral> {
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    let HirExpr::Field {
        owner, field_name, ..
    } = inner.as_ref()
    else {
        return None;
    };
    if !is_global_array_reference(owner) || field_name.as_str() != "from" {
        return None;
    }
    if args.is_empty() || args.len() > 3 {
        return None;
    }
    let HirExpr::ObjectLiteral { fields, .. } = &args[0] else {
        return None;
    };
    let mut length: Option<i128> = None;
    let mut indexed: Vec<(u64, HirExpr)> = Vec::new();
    for field in fields {
        let ObjectLiteralField::Property { name, value } = field else {
            return None;
        };
        if name.as_str() == "length" {
            let HirExpr::Int(value, _) = value else {
                return None;
            };
            length = Some(i128::from(*value));
        } else {
            let idx = canonical_integer_index(name.as_str())?;
            indexed.push((idx, value.clone()));
        }
    }
    let length = length?;
    indexed.sort_by_key(|(idx, _)| *idx);
    Some(ArrayLikeObjectLiteral { length, indexed })
}

impl ExprConverter {
    fn try_string_instance_method_dispatch(
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
            owner: method_owner,
            field_name: method_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if !is_string_typed_source(method_owner, types) {
            return None;
        }
        let op = match method_field.as_str() {
            "indexOf" => Some(RuntimeOp::StringIndexOf),
            "charAt" => Some(RuntimeOp::StringCharAt),
            _ => None,
        }?;
        let (min_arity, max_arity) = match op {
            RuntimeOp::StringIndexOf => (1, 2),
            RuntimeOp::StringCharAt => (1, 1),
            _ => unreachable!("string method arity"),
        };
        if args.len() < min_arity || args.len() > max_arity {
            ctx.error(
                "E0406",
                format!(
                    "String.prototype.{} requires {}..={} argument(s); got {}",
                    method_field.as_str(),
                    min_arity,
                    max_arity,
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let receiver_mir = self.convert_expr(
            method_owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let converted_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let mut full_args = Vec::with_capacity(1 + max_arity);
        full_args.push(receiver_mir);
        full_args.extend(converted_args);
        if op == RuntimeOp::StringIndexOf && full_args.len() == 2 {
            full_args.push(MirExpr::Int {
                value: 0,
                ty: TypeId::from_raw(0),
            });
        }
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: full_args,
            dest: Some(dest),
            ty,
        });
        Some(MirExpr::Local(dest))
    }

    fn try_function_method_dispatch(
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
                if field_name.as_str() == "getPrototypeOf" {
                    ctx.error(
                        "E0406",
                        "Object.getPrototypeOf is not supported in this AOT target; \
                         prototype chain resolution requires type-tracked receivers and is \
                         tracked as a separate architectural change. Use Object.keys() for \
                         enumerable-property iteration, or drop the call.",
                        Span::new(0, 0),
                    );
                    return MirExpr::Unit;
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
                    op: RuntimeOp::ObjectKeys,
                    args: mir_args,
                    dest: Some(dest),
                    ty,
                });
                return MirExpr::Local(dest);
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
                    return MirExpr::Unit;
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
                });
                return MirExpr::Local(dest);
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
                    return MirExpr::Unit;
                }
                let dest = self.fresh_local();
                self.push_temp_local(dest, ty);
                if mir_args.len() == 1 && is_string_typed_source(&args[0], types) {
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::ArrayFromString,
                        args: mir_args,
                        dest: Some(dest),
                        ty,
                    });
                    return MirExpr::Local(dest);
                }
                if mir_args.len() >= 2 && !matches!(&args[1], HirExpr::Global { .. }) {
                    ctx.error(
                        "E0406",
                        "Array.from(arr, mapFn[, thisArg]) in AOT requires mapFn to be a known global function reference \
                         (a top-level fn or a non-capturing closure lifted by `lower_closures`); \
                         capturing closures and arbitrary local expressions are not supported",
                        Span::new(0, 0),
                    );
                    return MirExpr::Unit;
                }
                if mir_args.len() == 3 {
                    ctx.error(
                        "E0406",
                        "Array.from(arr, mapFn, thisArg) does not support thisArg in AOT \
                         (AOT closures have no `this` binding); use the 2-arg form Array.from(arr, mapFn)",
                        Span::new(0, 0),
                    );
                    return MirExpr::Unit;
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
                });
                return MirExpr::Local(dest);
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
                });
                for item_mir in mir_args {
                    out.push(MirStmt::Runtime {
                        op: RuntimeOp::ArrayPush,
                        args: vec![MirExpr::Local(alloc_id), item_mir],
                        dest: None,
                        ty: TypeId::from_raw(0),
                    });
                }
                return MirExpr::Local(alloc_id);
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
                        return MirExpr::Unit;
                    }
                    let dest = self.fresh_local();
                    self.push_temp_local(dest, ty);
                    out.push(MirStmt::Runtime {
                        op,
                        args: mir_args,
                        dest: Some(dest),
                        ty,
                    });
                    return MirExpr::Local(dest);
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
                    });
                    return MirExpr::Local(dest);
                }
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
