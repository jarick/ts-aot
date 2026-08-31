use std::collections::HashMap;

use ts_aot_core::{MAX_DENSE_ARRAY_LEN, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::PLACEHOLDER_FUNCTION;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::array_from_object_literal;

fn types_compatible(declared: &TypeId, expected: &TypeId, types: &TypeTable) -> bool {
    if declared == expected {
        return true;
    }
    let lhs = types.resolve(*declared);
    let rhs = types.resolve(*expected);
    match (lhs, rhs) {
        (Some(Type::Error), _) | (_, Some(Type::Error)) => true,
        (Some(lhs), Some(rhs)) => *lhs == *rhs,
        _ => false,
    }
}

fn compute_promise_target_ty(
    op: RuntimeOp,
    args: &[HirExpr],
    ty: TypeId,
    types: &TypeTable,
) -> Option<TypeId> {
    match op {
        RuntimeOp::PromiseAll
        | RuntimeOp::PromiseRace
        | RuntimeOp::PromiseAllSettled
        | RuntimeOp::PromiseAny => args.first().and_then(|a| {
            let t = a.ty();
            types.resolve(t).and_then(|t| match t {
                Type::Array { element } => types.resolve(*element).and_then(|p| match p {
                    Type::Promise { ok, .. } => Some(*ok),
                    _ => None,
                }),
                _ => None,
            })
        }),
        RuntimeOp::PromiseResolveStatic => types
            .resolve(ty)
            .and_then(|t| match t {
                Type::Promise { ok, .. } => Some(*ok),
                _ => None,
            })
            .or_else(|| args.first().map(|a| a.ty())),
        _ => None,
    }
}

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
        if let Some(mir) = self.try_promise_instance_method_dispatch(
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
        if let HirCallee::Runtime { name, .. } = callee {
            return self.convert_runtime_call(
                name.as_str(),
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
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
        if let Some(mir) = self.try_generator_instance_method_dispatch(
            callee,
            args,
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
        if let Some(mir) = self.try_promise_static_method_dispatch(
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
        if let Some(mir) = self.try_weakmap_instance_method_dispatch(
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

    fn try_promise_instance_method_dispatch(
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
        let HirCallee::Indirect(indirect) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner, field_name, ..
        } = indirect.as_ref()
        else {
            return None;
        };
        let owner_ty = owner.ty();
        let op = match field_name.as_str() {
            "then" => RuntimeOp::PromiseThenInstance,
            "catch" => RuntimeOp::PromiseCatchInstance,
            "finally" => RuntimeOp::PromiseFinallyInstance,
            _ => return None,
        };
        let method = field_name.as_str();
        if !matches!(types.resolve(owner_ty), Some(Type::Promise { .. })) {
            return None;
        }
        let target_ty = types.resolve(owner_ty).and_then(|t| match t {
            Type::Promise { ok, .. } => Some(*ok),
            _ => None,
        });
        if args.len() != 1 {
            ctx.error(
                "E0406",
                format!(
                    "Promise.prototype.{method} requires exactly 1 argument (the handler); got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let first_arg = args.first()?;
        let resolved_handler_global: Option<ts_aot_core::Atom> = if let HirExpr::Global {
            name,
            ..
        } = first_arg
        {
            let mut qualified = None;
            for depth in (0..=self.namespace_path.len()).rev() {
                let probe =
                    crate::hir_to_mir::qualified_name(&self.namespace_path[..depth], name.as_str());
                if self.name_to_function.contains_key(&probe) {
                    qualified = Some(probe);
                    break;
                }
            }
            let Some(qualified) = qualified else {
                ctx.error(
                    "E0503",
                    format!(
                        "Promise.prototype.{method} handler `{name}` is not a known function; \
                             the handler must be a top-level function whose signature is \
                             compatible with the runtime operation"
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            };
            if let Some(handler_fn) = self.program.find_function_by_qualified_name(&qualified) {
                let expected_arity = match method {
                    "catch" | "then" => 1,
                    "finally" => 0,
                    _ => unreachable!("op mapping restricts method to then/catch/finally"),
                };
                if handler_fn.params.len() != expected_arity {
                    let arg_label = match method {
                        "catch" => "String",
                        "then" => "T",
                        _ => "",
                    };
                    ctx.error(
                            "E0504",
                            format!(
                                "Promise.prototype.{method} handler `{name}` must take exactly \
                                 {expected_arity} argument(s) (got {}); runtime expects FnOnce({arg_label})",
                                handler_fn.params.len()
                            ),
                            Span::new(0, 0),
                        );
                    return Some(MirExpr::Unit);
                }
                if let Some(target) = target_ty
                    && self
                        .validate_promise_handler_param_and_return(
                            op, &qualified, handler_fn, target, types, ctx,
                        )
                        .is_none()
                {
                    return Some(MirExpr::Unit);
                }
            }
            Some(qualified)
        } else {
            None
        };
        let promise_mir = self.convert_expr(
            owner,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        let handler_mir = match first_arg {
            HirExpr::Global { .. } => match resolved_handler_global.clone() {
                Some(qualified) => MirExpr::Global(qualified),
                None => return Some(MirExpr::Unit),
            },
            _ => self.convert_expr(
                first_arg,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ),
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: vec![promise_mir, handler_mir],
            dest: Some(dest),
            ty,
            target_ty,
        });
        Some(MirExpr::Local(dest))
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_promise_handler_param_and_return(
        &self,
        op: RuntimeOp,
        name: &ts_aot_core::Atom,
        handler_fn: &ts_aot_ir_hir::HirFunction,
        target: TypeId,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<()> {
        let method = match op {
            RuntimeOp::PromiseThenInstance => "then",
            RuntimeOp::PromiseCatchInstance => "catch",
            RuntimeOp::PromiseFinallyInstance => "finally",
            _ => return Some(()),
        };
        if matches!(op, RuntimeOp::PromiseCatchInstance) {
            let string_ty = types.intern(&Type::String);
            if let Some(param) = handler_fn.params.first()
                && !types_compatible(&param.ty, &string_ty, types)
                && !matches!(types.resolve(param.ty), Some(Type::Error))
            {
                ctx.error(
                    "E0504",
                    format!(
                        "Promise.prototype.catch handler `{name}` parameter type is not \
                         compatible with String (the rejection reason); \
                         got TypeId({}) but expected TypeId({})",
                        param.ty.raw(),
                        string_ty.raw()
                    ),
                    Span::new(0, 0),
                );
                return None;
            }
        } else if matches!(op, RuntimeOp::PromiseThenInstance)
            && let Some(param) = handler_fn.params.first()
            && !types_compatible(&param.ty, &target, types)
            && !matches!(types.resolve(param.ty), Some(Type::Error))
        {
            ctx.error(
                "E0504",
                format!(
                    "Promise.prototype.then handler `{name}` parameter type is not \
                     compatible with the Promise ok type T; \
                     got TypeId({}) but expected TypeId({})",
                    param.ty.raw(),
                    target.raw()
                ),
                Span::new(0, 0),
            );
            return None;
        }
        if method != "finally"
            && !types_compatible(&handler_fn.ret, &target, types)
            && !matches!(types.resolve(handler_fn.ret), Some(Type::Error))
        {
            ctx.error(
                "E0504",
                format!(
                    "Promise.prototype.{method} handler `{name}` return type is not \
                     compatible with the Promise ok type expected by the runtime; \
                     got TypeId({}) but the Promise expects TypeId({})",
                    handler_fn.ret.raw(),
                    target.raw()
                ),
                Span::new(0, 0),
            );
            return None;
        }
        Some(())
    }

    fn try_promise_static_method_dispatch(
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
        let HirCallee::Indirect(indirect) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner, field_name, ..
        } = indirect.as_ref()
        else {
            return None;
        };
        let HirExpr::Global {
            name: owner_name, ..
        } = owner.as_ref()
        else {
            return None;
        };
        if owner_name.as_str() != "Promise" {
            return None;
        }
        let method = field_name.as_str();
        let op = match method {
            "all" => RuntimeOp::PromiseAll,
            "race" => RuntimeOp::PromiseRace,
            "allSettled" => RuntimeOp::PromiseAllSettled,
            "any" => RuntimeOp::PromiseAny,
            "resolve" => RuntimeOp::PromiseResolveStatic,
            "reject" => RuntimeOp::PromiseRejectStatic,
            _ => return None,
        };
        let expected_arity = 1usize;
        if args.len() != expected_arity {
            ctx.error(
                "E0406",
                format!(
                    "Promise.{method} requires exactly {expected_arity} argument(s); got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let target_ty = if matches!(op, RuntimeOp::PromiseRejectStatic) {
            Some(
                types
                    .resolve(ty)
                    .and_then(|t| match t {
                        Type::Promise { ok, .. } => Some(*ok),
                        _ => None,
                    })
                    .unwrap_or_else(|| types.intern(&Type::Error)),
            )
        } else {
            compute_promise_target_ty(op, args, ty, types)
        };
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty,
            target_ty,
        });
        Some(MirExpr::Local(dest))
    }

    fn convert_runtime_call(
        &mut self,
        name: &str,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let Some(builtin) = super::runtime_dispatch::lookup_builtin(name) else {
            ctx.error(
                "P0005",
                format!("runtime helper `{name}` is not yet supported in HIR→MIR"),
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        };
        let op = builtin.op();
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty,
            target_ty: None,
        });
        MirExpr::Local(dest)
    }
}
