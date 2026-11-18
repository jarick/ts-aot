use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirUnaryOp};
use ts_aot_ir_mir::{MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::binary::{hir_expr_contains_call, hir_expr_may_reassign};
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, hir_expr_type_id, is_numeric_type_for_array_len, is_place,
    is_pure_place,
};
use crate::hir_to_mir::converter::ExprConverter;
use crate::lower_generators::GENERATOR_DIAG_DEFERRED_METHOD;

use super::globals::{
    array_push_receiver, generator_inner_type, is_array_static_type, is_global_array_reference,
    is_global_math_reference, is_global_object_reference, is_global_reflect_reference,
    is_global_string_reference, is_string_typed_source, type_label,
};

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_known_indirect_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        recv: Option<MirExpr>,
        mir_args: Vec<MirExpr>,
        arg_marks: &[usize],
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
        if let HirExpr::Field {
            owner, field_name, ..
        } = inner.as_ref()
            && is_global_object_reference(owner)
            && matches!(
                field_name.as_str(),
                "getPrototypeOf" | "keys" | "getOwnPropertyNames" | "setPrototypeOf"
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
                ctx.warning(
                    "E0406",
                    "Object.getPrototypeOf is not supported in this AOT target; \
                     prototype chain resolution requires type-tracked receivers and is \
                     tracked as a separate architectural change. The call is lowered to \
                     `undefined`.",
                    Span::new(0, 0),
                );
                for arg in mir_args {
                    if has_potential_side_effects(&arg) {
                        out.push(MirStmt::Expr(arg));
                    }
                }
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
            let string_ty = types.intern(&Type::String);
            let keys_ty = types.intern(&Type::Array { element: string_ty });
            if is_array_static_type(&args[0], types) {
                let include_length = field_name.as_str() == "getOwnPropertyNames";
                let mut own_args = mir_args;
                own_args.push(MirExpr::Bool(include_length));
                let dest = self.fresh_local();
                self.push_temp_local(dest, keys_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayOwnKeys,
                    args: own_args,
                    dest: Some(dest),
                    ty: keys_ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(dest));
            }
            let dest = self.fresh_local();
            self.push_temp_local(dest, keys_ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ObjectKeys,
                args: mir_args,
                dest: Some(dest),
                ty: keys_ty,
                target_ty: None,
            });
            return Some(MirExpr::Local(dest));
        }
        if let HirExpr::Field {
            owner: is_owner,
            field_name: is_field,
            ..
        } = inner.as_ref()
            && is_global_object_reference(is_owner)
            && is_field.as_str() == "is"
        {
            if mir_args.len() != 2 {
                ctx.error(
                    "E0406",
                    format!(
                        "Object.is requires exactly 2 arguments; got {}",
                        mir_args.len()
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
            let bool_ty = types.intern(&Type::Bool);
            let dest = self.fresh_local();
            self.push_temp_local(dest, bool_ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ObjectIs,
                args: mir_args,
                dest: Some(dest),
                ty: bool_ty,
                target_ty: None,
            });
            return Some(MirExpr::Local(dest));
        }
        if let Some(recv_owner) = array_push_receiver(inner.as_ref(), types) {
            debug_assert!(
                recv.is_some(),
                "convert_call must pre-evaluate the push receiver before converting arguments"
            );
            let recv_mir = recv.unwrap_or_else(|| {
                self.convert_expr(
                    recv_owner,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )
            });
            if mir_args.is_empty() {
                let len_ty = types.intern(&Type::F64);
                let len_dest = self.fresh_local();
                self.push_temp_local(len_dest, len_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayLen,
                    args: vec![recv_mir],
                    dest: Some(len_dest),
                    ty: len_ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(len_dest));
            }
            let len_ty = types.intern(&Type::I64);
            let len_dest = self.fresh_local();
            self.push_temp_local(len_dest, len_ty);
            let last = mir_args.len() - 1;
            for (i, item) in mir_args.into_iter().enumerate() {
                let is_last = i == last;
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayPush,
                    args: vec![recv_mir.clone(), item],
                    dest: is_last.then_some(len_dest),
                    ty: if is_last { len_ty } else { TypeId::from_raw(0) },
                    target_ty: None,
                });
            }
            return Some(MirExpr::Local(len_dest));
        }
        if let HirExpr::Field {
            owner: freeze_owner,
            field_name: freeze_field,
            ..
        } = inner.as_ref()
            && is_global_object_reference(freeze_owner)
            && matches!(
                freeze_field.as_str(),
                "freeze" | "seal" | "preventExtensions"
            )
        {
            if mir_args.is_empty() {
                return Some(MirExpr::Unit);
            }
            let mut iter = mir_args.into_iter();
            let target = iter.next().expect("checked non-empty above");
            for extra in iter {
                if has_potential_side_effects(&extra) {
                    out.push(MirStmt::Expr(extra));
                }
            }
            return Some(target);
        }
        if let HirExpr::Field {
            owner: degrade_owner,
            field_name: degrade_field,
            ..
        } = inner.as_ref()
            && (is_global_object_reference(degrade_owner)
                || is_global_reflect_reference(degrade_owner))
        {
            let owner_name = if is_global_reflect_reference(degrade_owner) {
                "Reflect"
            } else {
                "Object"
            };
            ctx.warning(
                "E0406",
                format!(
                    "static `{owner_name}.{}` is not supported in this AOT target; the call is \
                     lowered to `undefined`",
                    degrade_field.as_str()
                ),
                Span::new(0, 0),
            );
            for arg in mir_args {
                if has_potential_side_effects(&arg) {
                    out.push(MirStmt::Expr(arg));
                }
            }
            return Some(MirExpr::Unit);
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
            let bool_ty = types.intern(&Type::Bool);
            let dest = self.fresh_local();
            self.push_temp_local(dest, bool_ty);
            let op = if is_array_static_type(&args[0], types) {
                RuntimeOp::ArrayIsArray
            } else {
                RuntimeOp::ArrayIsArrayFalse
            };
            out.push(MirStmt::Runtime {
                op,
                args: mir_args,
                dest: Some(dest),
                ty: bool_ty,
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
            let element_ty = match hir_expr_type_id(&args[0]).and_then(|t| types.resolve(t)) {
                Some(Type::Array { element }) => *element,
                Some(Type::String) => types.intern(&Type::String),
                _ => ty,
            };
            let result_ty = if mir_args.len() == 2 {
                let mapped = match &args[1] {
                    HirExpr::Global { name, .. } => self
                        .program
                        .find_function_by_name(name)
                        .map_or(ty, |f| f.ret),
                    _ => ty,
                };
                types.intern(&Type::Array { element: mapped })
            } else {
                types.intern(&Type::Array {
                    element: element_ty,
                })
            };
            let dest = self.fresh_local();
            self.push_temp_local(dest, result_ty);
            if mir_args.len() == 1 && is_string_typed_source(&args[0], types) {
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayFromString,
                    args: mir_args,
                    dest: Some(dest),
                    ty: result_ty,
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
                ty: result_ty,
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
            && let MirExpr::String { id: key, .. } = &mir_args[0]
            && let Some(own) = self.has_own_property_literal(has_own_owner, key, shared_struct_ids)
        {
            if hir_expr_contains_call(has_own_owner) || hir_expr_may_reassign(has_own_owner) {
                let receiver = match recv {
                    Some(receiver) => receiver,
                    None => self.convert_expr(
                        has_own_owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    ),
                };
                out.push(MirStmt::Expr(receiver));
            }
            return Some(MirExpr::Bool(own));
        }
        if let HirExpr::Field {
            owner: has_own_owner,
            field_name: has_own_field,
            ..
        } = inner.as_ref()
            && has_own_field.as_str() == "hasOwnProperty"
            && mir_args.len() == 1
            && self.own_property_receiver_is_known(has_own_owner, shared_struct_ids)
        {
            ctx.error(
                "E0406",
                "obj.hasOwnProperty() in AOT requires a literal string key and a receiver with a \
                 concrete struct type or a known class value; dynamic keys have no statically \
                 known own-property set",
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if let Some(mir) = self.try_number_instance_method_dispatch(
            callee,
            args,
            recv.clone(),
            &mir_args,
            arg_marks,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return Some(mir);
        }
        if let Some(mir) = self.try_array_builtin_method_dispatch(
            callee,
            args,
            recv.clone(),
            &mir_args,
            arg_marks,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return Some(mir);
        }
        if let Some(mir) = self.try_prototype_method_call_dispatch(
            callee,
            args,
            &mir_args,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        ) {
            return Some(mir);
        }
        None
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_number_instance_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        recv: Option<MirExpr>,
        mir_args: &[MirExpr],
        arg_marks: &[usize],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let (owner, field_name, span) = indirect_method_field(callee)?;
        let op = number_method_op(field_name.as_str())?;
        self.lower_number_method(
            op,
            field_name.as_str(),
            owner,
            args,
            recv,
            mir_args,
            arg_marks,
            span,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        )
    }

    pub(in crate::hir_to_mir::convert_expr) fn lower_number_method(
        &mut self,
        op: RuntimeOp,
        method: &str,
        owner: &HirExpr,
        args: &[HirExpr],
        recv: Option<MirExpr>,
        mir_args: &[MirExpr],
        arg_marks: &[usize],
        span: Span,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        if !number_valued(owner, types) {
            return None;
        }
        if mir_args.len() > 1 {
            ctx.error(
                "E0406",
                format!(
                    "Number.prototype.{method}() takes at most one argument; got {}",
                    mir_args.len()
                ),
                span,
            );
            return Some(MirExpr::Unit);
        }
        if let Some(argument) = args.first()
            && !number_valued(argument, types)
            && !matches!(argument, HirExpr::Undefined(_))
        {
            ctx.error(
                "E0406",
                format!(
                    "Number.prototype.{method}() in AOT requires a number-typed argument; \
                     string and other coercions of the radix/digits argument are not supported"
                ),
                span,
            );
            return Some(MirExpr::Unit);
        }
        let receiver = match recv {
            Some(recv) => recv,
            None => {
                let snapshot_ty = self.operand_static_ty(owner, types);
                match (arg_marks.first().copied(), snapshot_ty) {
                    (Some(at), Some(recv_ty)) => {
                        let mut owner_stmts: Vec<MirStmt> = Vec::new();
                        let owner_mir = self.convert_expr(
                            owner,
                            &mut owner_stmts,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        let snapshot = self.fresh_local();
                        self.push_temp_local(snapshot, recv_ty);
                        owner_stmts.push(MirStmt::Let {
                            local: snapshot,
                            ty: recv_ty,
                            init: Some(owner_mir),
                            mutable: false,
                        });
                        let at = at.min(out.len());
                        out.splice(at..at, owner_stmts);
                        MirExpr::Local(snapshot)
                    }
                    _ => self.convert_expr(
                        owner,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    ),
                }
            }
        };
        let string_ty = types.intern(&Type::String);
        let dest = self.fresh_local();
        self.push_temp_local(dest, string_ty);
        let mut runtime_args = vec![receiver];
        if let Some(argument) = present_arg(mir_args, 0) {
            runtime_args.push(argument);
        }
        out.push(MirStmt::Runtime {
            op,
            args: runtime_args,
            dest: Some(dest),
            ty: string_ty,
            target_ty: None,
        });
        Some(MirExpr::Local(dest))
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_array_builtin_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        recv: Option<MirExpr>,
        mir_args: &[MirExpr],
        arg_marks: &[usize],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let (owner, field_name, span) = indirect_method_field(callee)?;
        let method = field_name.as_str();
        if !matches!(method, "slice" | "indexOf" | "concat") {
            return None;
        }
        let element = array_receiver_element_ty(owner, types)?;
        self.lower_array_builtin_method(
            method,
            owner,
            element,
            args,
            recv,
            mir_args,
            arg_marks,
            span,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        )
    }

    pub(in crate::hir_to_mir::convert_expr) fn lower_array_builtin_method(
        &mut self,
        method: &str,
        owner: &HirExpr,
        element: TypeId,
        args: &[HirExpr],
        recv: Option<MirExpr>,
        mir_args: &[MirExpr],
        arg_marks: &[usize],
        span: Span,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let (min_arity, max_arity) = match method {
            "slice" => (0, 2),
            "indexOf" => (1, 2),
            "concat" => (0, usize::MAX),
            _ => return None,
        };
        if mir_args.len() < min_arity || mir_args.len() > max_arity {
            ctx.error(
                "E0406",
                format!(
                    "Array.prototype.{method}() requires {min_arity}..={max_arity} argument(s); got {}",
                    mir_args.len()
                ),
                span,
            );
            return Some(MirExpr::Unit);
        }
        let array_ty = types.intern(&Type::Array { element });
        let mut receiver_staged = 0usize;
        let receiver = match recv {
            Some(recv) => recv,
            None => {
                let at = arg_marks
                    .first()
                    .copied()
                    .unwrap_or(out.len())
                    .min(out.len());
                let mut staged: Vec<MirStmt> = Vec::new();
                let recv_mir = self.convert_expr(
                    owner,
                    &mut staged,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                let recv_mir = if is_pure_place(&recv_mir) {
                    recv_mir
                } else {
                    let tmp = self.fresh_local();
                    self.push_temp_local(tmp, array_ty);
                    staged.push(MirStmt::Let {
                        local: tmp,
                        ty: array_ty,
                        init: Some(recv_mir),
                        mutable: true,
                    });
                    MirExpr::Local(tmp)
                };
                receiver_staged = staged.len();
                out.splice(at..at, staged);
                recv_mir
            }
        };
        match method {
            "slice" => {
                if matches!(types.resolve(element), Some(Type::Array { .. })) {
                    ctx.error(
                        "E0406",
                        "Array.prototype.slice() in AOT does not support arrays of arrays: the \
                         element type is itself an array, and a cloned element would have an \
                         independent identity instead of the shared identity JavaScript requires",
                        span,
                    );
                    return Some(MirExpr::Unit);
                }
                for argument in args {
                    if !number_valued(argument, types) && !matches!(argument, HirExpr::Undefined(_))
                    {
                        ctx.error(
                            "E0406",
                            "Array.prototype.slice() in AOT requires number-typed start/end \
                             arguments; nullish arguments must be written as `undefined`",
                            span,
                        );
                        return Some(MirExpr::Unit);
                    }
                }
                let start = present_arg(mir_args, 0).unwrap_or(MirExpr::Int {
                    value: 0,
                    ty: TypeId::from_raw(0),
                });
                let start = if arg_marks.len() > 1 && is_place(&start) {
                    let start_ty = self
                        .operand_static_ty(&args[0], types)
                        .unwrap_or(TypeId::from_raw(0));
                    let snapshot = self.fresh_local();
                    self.push_temp_local(snapshot, start_ty);
                    let at = (arg_marks[1] + receiver_staged).min(out.len());
                    out.insert(
                        at,
                        MirStmt::Let {
                            local: snapshot,
                            ty: start_ty,
                            init: Some(start),
                            mutable: false,
                        },
                    );
                    MirExpr::Local(snapshot)
                } else {
                    start
                };
                let end = present_arg(mir_args, 1).unwrap_or(MirExpr::Int {
                    value: i128::from(i64::MAX),
                    ty: TypeId::from_raw(0),
                });
                let dest = self.fresh_local();
                self.push_temp_local(dest, array_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArraySlice,
                    args: vec![receiver, start, end],
                    dest: Some(dest),
                    ty: array_ty,
                    target_ty: None,
                });
                Some(MirExpr::Local(dest))
            }
            "indexOf" => {
                let mut needle = match mir_args.first() {
                    Some(needle) if !matches!(needle, MirExpr::Unit) => needle.clone(),
                    _ => {
                        ctx.error(
                            "E0406",
                            "Array.prototype.indexOf() in AOT requires the searched value; \
                             `undefined` as the searched value is not supported",
                            span,
                        );
                        return Some(MirExpr::Unit);
                    }
                };
                if present_arg(mir_args, 1).is_some()
                    && !matches!(
                        needle,
                        MirExpr::Unit
                            | MirExpr::Bool(_)
                            | MirExpr::Int { .. }
                            | MirExpr::Float { .. }
                            | MirExpr::String { .. }
                            | MirExpr::Null { .. }
                    )
                    && let Some(needle_ty) = args
                        .first()
                        .and_then(|argument| self.operand_static_ty(argument, types))
                        .or_else(|| self.mir_expr_static_ty(&needle, types))
                {
                    let needle_temp = self.fresh_local();
                    self.push_temp_local(needle_temp, needle_ty);
                    let at = arg_marks
                        .get(1)
                        .copied()
                        .map_or(out.len(), |mark| mark + receiver_staged)
                        .min(out.len());
                    out.insert(
                        at,
                        MirStmt::Let {
                            local: needle_temp,
                            ty: needle_ty,
                            init: Some(needle),
                            mutable: false,
                        },
                    );
                    needle = MirExpr::Local(needle_temp);
                }
                let from = present_arg(mir_args, 1).unwrap_or(MirExpr::Int {
                    value: 0,
                    ty: TypeId::from_raw(0),
                });
                let number_ty = types.intern(&Type::F64);
                let dest = self.fresh_local();
                self.push_temp_local(dest, number_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayIndexOf,
                    args: vec![receiver, needle, from],
                    dest: Some(dest),
                    ty: number_ty,
                    target_ty: None,
                });
                Some(MirExpr::Local(dest))
            }
            "concat" => {
                let mut parts = vec![receiver];
                for (argument, mir) in args.iter().zip(mir_args.iter()) {
                    if !matches!(types.resolve(argument.ty()), Some(Type::Array { element: arg_element }) if *arg_element == element)
                    {
                        ctx.error(
                            "E0406",
                            "Array.prototype.concat() in AOT only concatenates arrays with the \
                             same element type; non-array and mismatched-element arguments are \
                             not supported",
                            span,
                        );
                        return Some(MirExpr::Unit);
                    }
                    parts.push(mir.clone());
                }
                let dest = self.fresh_local();
                self.push_temp_local(dest, array_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ArrayConcat,
                    args: parts,
                    dest: Some(dest),
                    ty: array_ty,
                    target_ty: None,
                });
                Some(MirExpr::Local(dest))
            }
            _ => None,
        }
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_prototype_method_call_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        mir_args: &[MirExpr],
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let (method_ref, field_name, span) = indirect_method_field(callee)?;
        let is_apply = match field_name.as_str() {
            "call" => false,
            "apply" => true,
            _ => return None,
        };
        let HirExpr::Field {
            owner: prototype,
            field_name: method_name,
            ..
        } = method_ref
        else {
            return None;
        };
        let HirExpr::Field {
            owner: prototype_owner,
            field_name: prototype_field,
            ..
        } = prototype.as_ref()
        else {
            return None;
        };
        if prototype_field.as_str() != "prototype" {
            return None;
        }
        let prototype_owner_name = match prototype_owner.as_ref() {
            HirExpr::Global { name, .. } => Some(name.clone()),
            _ => None,
        };
        let owner_is = |expected: &str| {
            prototype_owner_name
                .as_ref()
                .is_some_and(|name| name.as_str() == expected)
        };
        let owner_is_class = prototype_owner_name
            .as_ref()
            .is_some_and(|name| self.lookup_class_struct_by_name(name).is_some())
            || self.struct_id_for_ty(prototype_owner.ty(), types).is_some();
        if args.is_empty() || mir_args.is_empty() {
            ctx.error(
                "E0406",
                format!(
                    "`X.prototype.m.{}` requires the receiver object as its first argument",
                    field_name.as_str()
                ),
                span,
            );
            return Some(MirExpr::Unit);
        }
        let receiver = &args[0];
        let receiver_mir = mir_args[0].clone();
        let (rest_args, rest_mir): (Vec<HirExpr>, Vec<MirExpr>) = if is_apply {
            match args.get(1) {
                None => (Vec::new(), Vec::new()),
                Some(HirExpr::ArrayLiteral { elements, .. }) => {
                    let converted: Vec<MirExpr> = elements
                        .iter()
                        .map(|element| {
                            self.convert_call_arg(
                                element,
                                None,
                                out,
                                shared_struct_ids,
                                shared_next_struct,
                                types,
                                ctx,
                            )
                        })
                        .collect();
                    for extra in args.iter().skip(2) {
                        let extra_mir = self.convert_call_arg(
                            extra,
                            None,
                            out,
                            shared_struct_ids,
                            shared_next_struct,
                            types,
                            ctx,
                        );
                        if has_potential_side_effects(&extra_mir) {
                            out.push(MirStmt::Expr(extra_mir));
                        }
                    }
                    (elements.clone(), converted)
                }
                Some(_) => {
                    ctx.error(
                        "E0406",
                        "`X.prototype.m.apply(receiver, args)` in AOT requires a literal array \
                         as the arguments value; a runtime-computed array cannot be unpacked \
                         into an argument list",
                        span,
                    );
                    return Some(MirExpr::Unit);
                }
            }
        } else {
            (args[1..].to_vec(), mir_args[1..].to_vec())
        };
        let method = method_name.as_str();
        if owner_is("Number")
            && let Some(op) = number_method_op(method)
            && let Some(mir) = self.lower_number_method(
                op,
                method,
                receiver,
                &rest_args,
                Some(receiver_mir.clone()),
                &rest_mir,
                &[],
                span,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            )
        {
            return Some(mir);
        }
        if owner_is("Array")
            && let Some(element) = array_receiver_element_ty(receiver, types)
            && matches!(method, "slice" | "indexOf" | "concat")
            && let Some(mir) = self.lower_array_builtin_method(
                method,
                receiver,
                element,
                &rest_args,
                Some(receiver_mir.clone()),
                &rest_mir,
                &[],
                span,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            )
        {
            return Some(mir);
        }
        if method == "hasOwnProperty" && (owner_is("Object") || owner_is_class) {
            if rest_mir.len() != 1 {
                ctx.error(
                    "E0406",
                    format!(
                        "Object.prototype.hasOwnProperty.{} requires exactly 1 argument (the key); got {}",
                        field_name.as_str(),
                        rest_mir.len()
                    ),
                    span,
                );
                return Some(MirExpr::Unit);
            }
            let MirExpr::String { id: key, .. } = &rest_mir[0] else {
                ctx.error(
                    "E0406",
                    "hasOwnProperty() in AOT requires a literal string key; a runtime-computed key \
                     has no statically known own-property set",
                    span,
                );
                return Some(MirExpr::Unit);
            };
            if matches!(receiver, HirExpr::Null(_) | HirExpr::Undefined(_)) {
                let string_ty = types.intern(&Type::String);
                let bool_ty = types.intern(&Type::Bool);
                let result_temp = self.fresh_local();
                self.push_temp_local(result_temp, bool_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::ThrowTypeError,
                    args: vec![MirExpr::String {
                        id: ts_aot_core::Atom::from(
                            "Object.prototype.hasOwnProperty called on null or undefined",
                        ),
                        ty: string_ty,
                    }],
                    dest: Some(result_temp),
                    ty: bool_ty,
                    target_ty: None,
                });
                return Some(MirExpr::Local(result_temp));
            }
            if let Some(own) = self.has_own_property_literal(receiver, key, shared_struct_ids) {
                if hir_expr_contains_call(receiver) {
                    out.push(MirStmt::Expr(receiver_mir.clone()));
                }
                return Some(MirExpr::Bool(own));
            }
            ctx.error(
                "E0406",
                "hasOwnProperty() in AOT requires a receiver with a concrete struct type or a known \
                 class value (`C`, `C.prototype`); this receiver's own-property set is not \
                 statically known",
                span,
            );
            return Some(MirExpr::Unit);
        }
        ctx.error(
            "E0406",
            format!(
                "`X.prototype.{method}.{}` is not supported in this AOT target; supported \
                 prototype methods are Number.prototype.{{toString,toFixed,toExponential,toPrecision}}, \
                 Array.prototype.{{slice,indexOf,concat}} and Object.prototype.hasOwnProperty",
                field_name.as_str()
            ),
            span,
        );
        Some(MirExpr::Unit)
    }

    pub(in crate::hir_to_mir::convert_expr) fn has_own_property_literal(
        &self,
        receiver: &HirExpr,
        key: &ts_aot_core::Atom,
        shared_struct_ids: &HashMap<TypeId, StructId>,
    ) -> Option<bool> {
        let receiver_struct = hir_expr_type_id(receiver).and_then(|ty| {
            self.struct_ids
                .get(&ty)
                .or_else(|| shared_struct_ids.get(&ty))
                .copied()
        });
        let receiver_class = match receiver {
            HirExpr::Global { name, .. } => self.lookup_class_struct_by_name(name),
            HirExpr::Field {
                owner, field_name, ..
            } if field_name.as_str() == "prototype" => match owner.as_ref() {
                HirExpr::Global { name, .. } => self.lookup_class_struct_by_name(name),
                _ => None,
            },
            _ => None,
        };
        if receiver_struct.is_none() && receiver_class.is_none() {
            return None;
        }
        if key.as_str().starts_with('#') {
            return Some(false);
        }
        if let Some(sid) = receiver_struct {
            return Some(self.field_id_lookup.contains_key(&(sid, key.clone())));
        }
        if let HirExpr::Global { name, .. } = receiver
            && let Some(sid) = receiver_class
        {
            return Some(self.class_value_own_property(name, key, sid));
        }
        if let HirExpr::Field { field_name, .. } = receiver
            && field_name.as_str() == "prototype"
            && let Some(sid) = receiver_class
        {
            let getter = ts_aot_core::Atom::from(format!("__get_{}", key.as_str()));
            let setter = ts_aot_core::Atom::from(format!("__set_{}", key.as_str()));
            let own = key.as_str() == "constructor"
                || self.method_map.contains_key(&(sid, key.clone(), false))
                || self.method_map.contains_key(&(sid, getter, false))
                || self.method_map.contains_key(&(sid, setter, false));
            return Some(own);
        }
        None
    }

    fn own_property_receiver_is_known(
        &self,
        receiver: &HirExpr,
        shared_struct_ids: &HashMap<TypeId, StructId>,
    ) -> bool {
        if hir_expr_type_id(receiver).is_some_and(|ty| {
            self.struct_ids
                .get(&ty)
                .or_else(|| shared_struct_ids.get(&ty))
                .is_some()
        }) {
            return true;
        }
        if let HirExpr::Global { name, .. } = receiver {
            return self.lookup_class_struct_by_name(name).is_some();
        }
        if let HirExpr::Field {
            owner, field_name, ..
        } = receiver
            && field_name.as_str() == "prototype"
            && let HirExpr::Global { name, .. } = owner.as_ref()
        {
            return self.lookup_class_struct_by_name(name).is_some();
        }
        false
    }

    fn class_value_own_property(
        &self,
        class: &ts_aot_core::Atom,
        key: &ts_aot_core::Atom,
        sid: StructId,
    ) -> bool {
        if matches!(key.as_str(), "prototype" | "name" | "length") {
            return true;
        }
        let getter = ts_aot_core::Atom::from(format!("__get_{}", key.as_str()));
        let setter = ts_aot_core::Atom::from(format!("__set_{}", key.as_str()));
        if self.method_map.contains_key(&(sid, key.clone(), true))
            || self.method_map.contains_key(&(sid, getter, true))
            || self.method_map.contains_key(&(sid, setter, true))
        {
            return true;
        }
        let prefix = format!(
            "{}__{}_",
            ts_aot_core::escape_rust_ident(class.as_str()),
            ts_aot_core::escape_rust_ident(key.as_str())
        );
        self.program.declarations.iter().any(|decl| match decl {
            ts_aot_ir_hir::HirDecl::Global { name, .. } => {
                name.as_str().strip_prefix(&prefix).is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
            }
            _ => false,
        })
    }

    pub(in crate::hir_to_mir::convert_expr) fn try_generator_instance_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
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
            owner,
            field_name,
            span: field_span,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        let field_span = *field_span;
        let owner_ty = owner.ty();
        let inner_ty = generator_inner_type(owner_ty, types)?;
        match field_name.as_str() {
            "next" => {
                if !args.is_empty() {
                    ctx.error(
                        "E0406",
                        format!(
                            "Generator.prototype.next() takes no arguments; got {}",
                            args.len()
                        ),
                        field_span,
                    );
                    return Some(MirExpr::Unit);
                }
                let owner_mir = self.convert_expr(
                    owner,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                let dest = self.fresh_local();
                let result_ty = if matches!(
                    types.resolve(owner_ty),
                    Some(Type::Generator { is_async: true, .. })
                ) {
                    let yield_ty = types.intern(&Type::AsyncGenYield { inner: inner_ty });
                    types.intern(&Type::Promise {
                        ok: yield_ty,
                        err: None,
                    })
                } else {
                    types.intern(&Type::GeneratorResult { inner: inner_ty })
                };
                self.push_temp_local(dest, result_ty);
                out.push(MirStmt::Runtime {
                    op: RuntimeOp::GeneratorNext,
                    args: vec![owner_mir],
                    dest: Some(dest),
                    ty: result_ty,
                    target_ty: None,
                });
                Some(MirExpr::Local(dest))
            }
            "return" | "throw" => {
                ctx.error(
                    GENERATOR_DIAG_DEFERRED_METHOD,
                    format!(
                        "Generator.prototype.{} is not supported in PR 7.8 MVP \
                         (deferred — use .next() and explicit done handling instead)",
                        field_name
                    ),
                    field_span,
                );
                Some(MirExpr::Unit)
            }
            _ => {
                ctx.error(
                    "E0406",
                    format!(
                        "method `{}` is not recognized on `Generator<{}>`; \
                         recognized methods are: `next`, `return`, `throw`",
                        field_name,
                        type_label(types, inner_ty),
                    ),
                    field_span,
                );
                Some(MirExpr::Unit)
            }
        }
    }
}

fn indirect_method_field(callee: &HirCallee) -> Option<(&HirExpr, &ts_aot_core::Atom, Span)> {
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    match inner.as_ref() {
        HirExpr::Field {
            owner,
            field_name,
            span,
            ..
        } => Some((owner.as_ref(), field_name, *span)),
        _ => None,
    }
}

fn number_method_op(method: &str) -> Option<RuntimeOp> {
    match method {
        "toString" => Some(RuntimeOp::NumberToString),
        "toFixed" => Some(RuntimeOp::NumberToFixed),
        "toExponential" => Some(RuntimeOp::NumberToExponential),
        "toPrecision" => Some(RuntimeOp::NumberToPrecision),
        _ => None,
    }
}

fn present_arg(mir_args: &[MirExpr], index: usize) -> Option<MirExpr> {
    mir_args
        .get(index)
        .filter(|argument| !matches!(argument, MirExpr::Unit))
        .cloned()
}

fn array_receiver_element_ty(owner: &HirExpr, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(hir_expr_type_id(owner)?) {
        Some(Type::Array { element }) => Some(*element),
        _ => None,
    }
}

fn number_valued(expr: &HirExpr, types: &TypeTable) -> bool {
    if is_numeric_type_for_array_len(expr, types) {
        return true;
    }
    match expr {
        HirExpr::Unary { op, expr, .. } => {
            matches!(op, HirUnaryOp::Neg | HirUnaryOp::BitNot) && number_valued(expr, types)
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            matches!(
                op,
                HirBinaryOp::Add
                    | HirBinaryOp::Sub
                    | HirBinaryOp::Mul
                    | HirBinaryOp::Div
                    | HirBinaryOp::Mod
                    | HirBinaryOp::BitAnd
                    | HirBinaryOp::BitOr
                    | HirBinaryOp::BitXor
                    | HirBinaryOp::Shl
                    | HirBinaryOp::Shr
            ) && number_valued(lhs, types)
                && number_valued(rhs, types)
        }
        HirExpr::Ternary {
            then_branch,
            else_branch,
            ..
        } => number_valued(then_branch, types) && number_valued(else_branch, types),
        HirExpr::TypeAssertion { expr, .. } => number_valued(expr, types),
        _ => false,
    }
}
