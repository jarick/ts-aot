use std::collections::HashMap;

use ts_aot_core::{Atom, FunctionId, MAX_DENSE_ARRAY_LEN, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirFunction};
use ts_aot_ir_mir::{MirBlock, MirExpr, MirPlace, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::mir_expr_ty;

pub(super) const THROW_PROPAGATION_DIAG: &str = "E0416";

use crate::hir_to_mir::converter::{
    ExprConverter, build_class_index, class_at_path, compute_chain_owner_ty,
};
use crate::hir_to_mir::program::throw_type_name;
use crate::hir_to_mir::{PLACEHOLDER_FUNCTION, qualified_name};

/// The source span of a call: the callee's own span when the frontend gives it
/// one (`boom`, `a.m`, `this.m`), widened by the argument list. Using the first
/// *argument* alone made every argument-less call (`boom()`) report `0..0`.
fn call_span(callee: &HirCallee, args: &[HirExpr]) -> Span {
    let mut span = match callee {
        HirCallee::Indirect(inner) => inner.span(),
        HirCallee::Runtime { .. } => Span::new(0, 0),
        HirCallee::Function(_) | HirCallee::Closure(_) => Span::new(0, 0),
    };
    if let Some(first) = args.first() {
        span = Span::new(span.start.min(first.span().start), span.end);
    }
    if let Some(last) = args.last() {
        span = Span::new(span.start, span.end.max(last.span().end));
    }
    span
}

use super::globals::{array_from_object_literal, array_push_receiver};
use super::util::is_pure_place;

const PROMISE_HANDLER_UNKNOWN_DIAG: &str = "E0508";
const PROMISE_HANDLER_MISMATCH_DIAG: &str = "E0509";

/// One parameter of a resolvable callee.
pub(super) struct CalleeParam {
    pub ty: TypeId,
    /// Whether the caller may omit this argument. For functions and methods it
    /// is `HirParam.optional`; for a function *value* it is the matching flag of
    /// its `Type::Fn` signature - never a property of the parameter's ABI type.
    pub optional: bool,
}

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

fn combinator_target_ty(args: &[HirExpr], types: &TypeTable) -> Option<TypeId> {
    args.first().and_then(|a| {
        let t = a.ty();
        types.resolve(t).and_then(|t| match t {
            Type::Array { element } => types.resolve(*element).and_then(|p| match p {
                Type::Promise { ok, .. } => Some(*ok),
                _ => None,
            }),
            _ => None,
        })
    })
}

fn promise_ok_type(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty)? {
        Type::Promise { ok, .. } => Some(*ok),
        Type::Result { ok, .. } => promise_ok_type(*ok, types),
        _ => None,
    }
}

fn known_static_ty(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty) {
        None | Some(Type::Error) => None,
        Some(_) => Some(ty),
    }
}

pub(super) fn is_numeric_type(ty: TypeId, types: &TypeTable) -> bool {
    matches!(
        types.resolve(ty),
        Some(
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::F32
                | Type::F64
        )
    )
}

/// Whether `callee` is a method invoked through an optional chain (`a?.m(..)`),
/// which is the call shape the backend emits as a `map` over the receiver
/// (`recv.as_ref().map(|r| A::m(r))`) and therefore the only shape whose
/// throwing callee yields an `Option<Result<T, E>>`.
fn optional_chain_receiver(callee: &HirCallee, types: &TypeTable) -> bool {
    let HirCallee::Indirect(inner) = callee else {
        return false;
    };
    let HirExpr::Field { owner, .. } = inner.as_ref() else {
        return false;
    };
    matches!(owner.as_ref(), HirExpr::OptionalChain { .. })
        && matches!(types.resolve(owner.ty()), Some(Type::Optional { .. }))
}

/// Whether `result` is a `MirExpr::Call` whose receiver (its first argument) is
/// the `MirExpr::OptionalChain` the backend maps over, and whose MIR type is the
/// `Optional<T>` that mapping produces. Together with
/// [`optional_chain_receiver`] this pins down the call shape that needs
/// transposing before its error can be propagated.
fn optional_receiver_call_ok(result: &MirExpr, types: &TypeTable) -> bool {
    let MirExpr::Call { args, ty, .. } = result else {
        return false;
    };
    matches!(args.first(), Some(MirExpr::OptionalChain { .. }))
        && matches!(types.resolve(*ty), Some(Type::Optional { .. }))
}

enum StaticOkRecovery {
    Ok(TypeId),
    VoidMismatch(TypeId),
    NullMismatch(TypeId),
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
        self.propagate_callee_throws(callee, args, result, out, types, ctx)
    }

    /// A call to a function that may throw returns a `Result`, so the caller must
    /// propagate it (`expr?`) - which is only possible when the enclosing
    /// function returns a `Result` of the same error type, i.e. when the call
    /// graph fixpoint (`module_throws`) gave it that error type. Everything else
    /// is reported instead of emitting code that cannot compile.
    /// Error type of the module function a converted `MirExpr::Call` refers to.
    pub(super) fn function_id_throws(&self, id: FunctionId) -> Option<(String, TypeId)> {
        let name = self
            .name_to_function
            .iter()
            .find_map(|(name, fid)| (*fid == id).then(|| name.as_str().to_owned()))?;
        self.function_throws(&Atom::from(name.clone()))
            .map(|throws| (name, throws))
    }

    fn propagate_callee_throws(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        result: MirExpr,
        out: &mut Vec<MirStmt>,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        // A free function is resolved by name; any other call target that the
        // module declares itself (a method, say) is resolved through the
        // function id of the converted call.
        let named = match callee {
            HirCallee::Indirect(inner) => match inner.as_ref() {
                HirExpr::Global { name, .. } => self
                    .function_throws(name)
                    .map(|throws| (name.as_str().to_owned(), throws)),
                _ => None,
            },
            _ => None,
        };
        let resolved = named.or_else(|| match &result {
            MirExpr::Call { callee, .. } => self.function_id_throws(*callee),
            _ => None,
        });
        let Some((callee_label, callee_throws)) = resolved else {
            return result;
        };
        let name = Atom::from(callee_label);
        let span = call_span(callee, args);
        let callee_name = throw_type_name(types, callee_throws);
        // `a?.m(..)` with a throwing `m`: the backend emits the call as a `map`
        // over the optional receiver, so the call yields `Option<Result<T, E>>`
        // while its MIR type says `Option<T>`. The plain `expr?` that
        // `MirExpr::Propagate` emits does not type-check on that value, so the
        // propagation needs the transposing shape (see
        // `transpose_optional_receiver_call`). Only the receiver mapping is
        // affected: an optional value that happens to be a *regular* argument
        // (`f(a?.x)`) is passed through the ABI as usual.
        let transposable =
            optional_chain_receiver(callee, types) && optional_receiver_call_ok(&result, types);
        if self.try_depth > 0 {
            ctx.error(
                THROW_PROPAGATION_DIAG,
                format!(
                    "the call to `{}` may throw `{}`, and this AOT target cannot catch an error \
                     raised inside a callee: a `try`/`catch` only handles the enclosing \
                     function's own `throw`s. Call it outside the `try`, or handle the error in \
                     the callee",
                    name.as_str(),
                    callee_name
                ),
                span,
            );
            return result;
        }
        match self.current_throws {
            Some(current) if types.resolve(current) == types.resolve(callee_throws) => {
                if transposable {
                    return self.transpose_optional_receiver_call(
                        result,
                        callee_throws,
                        out,
                        types,
                    );
                }
                let ty = mir_expr_ty(&result);
                MirExpr::Propagate {
                    expr: Box::new(result),
                    ty,
                }
            }
            Some(current) => {
                let current_name = throw_type_name(types, current);
                ctx.error(
                    THROW_PROPAGATION_DIAG,
                    format!(
                        "the call to `{}` throws `{}`, but the enclosing function propagates \
                         `{}`: this AOT target keeps one error type per function. Make both \
                         throw the same type, or handle the error in `{}`",
                        name.as_str(),
                        callee_name,
                        current_name,
                        name.as_str()
                    ),
                    span,
                );
                result
            }
            None => {
                ctx.error(
                    THROW_PROPAGATION_DIAG,
                    format!(
                        "the call to `{}` may throw `{}`, but the enclosing context cannot \
                         propagate it (a generator, a closure body or top-level code does not \
                         return a `Result`). Call it from a plain function that declares a \
                         `throws` type, or handle the error in `{}`",
                        name.as_str(),
                        callee_name,
                        name.as_str()
                    ),
                    span,
                );
                result
            }
        }
    }

    /// `a?.m(..)` where `m` throws: the receiver is mapped over, so the call
    /// produces an `Option<Result<T, E>>`. MIR has no transposing node and the
    /// backend emits `MirExpr::Propagate` as a plain `expr?` (which cannot be
    /// applied to an `Option` inside a `Result`-returning function), so the
    /// transposed value is built from ordinary statements:
    ///
    /// ```text
    /// let __mapped: Option<Result<T, E>> = a.as_ref().map(|r| A::m(r));
    /// let mut __value: Option<T>;
    /// if __mapped.is_some() {
    ///     __value = Some(__mapped.unwrap()?);
    /// } else {
    ///     __value = None;
    /// }
    /// ```
    ///
    /// The expression is then `__value`, i.e. `a?.m()` propagates the callee's
    /// error and keeps the receiver's absence, and the enclosing function must
    /// return `Result<T, E>` - which the caller has already checked.
    fn transpose_optional_receiver_call(
        &mut self,
        mapped: MirExpr,
        err_ty: TypeId,
        out: &mut Vec<MirStmt>,
        types: &mut TypeTable,
    ) -> MirExpr {
        let value_ty = mir_expr_ty(&mapped);
        let Some(Type::Optional {
            inner: ok_ty,
            nullish,
        }) = types.resolve(value_ty)
        else {
            // Not the mapped-over-receiver shape after all: `?` is still wrong,
            // but there is nothing to transpose, so leave the value as the call
            // produced it instead of inventing a `Result` out of it.
            return mapped;
        };
        let (ok_ty, nullish) = (*ok_ty, *nullish);
        let result_ty = types.intern(&Type::Result {
            ok: ok_ty,
            err: err_ty,
        });
        let mapped_ty = types.intern(&Type::Optional {
            inner: result_ty,
            nullish,
        });
        let mapped_local = self.fresh_local();
        self.push_temp_local_with_mut(mapped_local, mapped_ty, false);
        out.push(MirStmt::Let {
            local: mapped_local,
            ty: mapped_ty,
            init: Some(mapped),
            mutable: false,
        });
        let value_local = self.fresh_local();
        self.push_temp_local(value_local, value_ty);
        out.push(MirStmt::Let {
            local: value_local,
            ty: value_ty,
            init: None,
            mutable: true,
        });
        let bool_ty = types.intern(&Type::Bool);
        out.push(MirStmt::If {
            cond: MirExpr::IsSome {
                expr: Box::new(MirExpr::Local(mapped_local)),
                ty: bool_ty,
            },
            then_block: MirBlock::with(MirStmt::Assign {
                target: MirPlace::Local { id: value_local },
                value: MirExpr::Propagate {
                    expr: Box::new(MirExpr::UnwrapOptional {
                        expr: Box::new(MirExpr::Local(mapped_local)),
                        ty: result_ty,
                    }),
                    ty: ok_ty,
                },
            }),
            else_block: Some(MirBlock::with(MirStmt::Assign {
                target: MirPlace::Local { id: value_local },
                value: MirExpr::Unit,
            })),
        });
        MirExpr::Local(value_local)
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
        if let Some(mir) = self.try_promise_instance_method_dispatch(callee, args, types, ctx) {
            return mir;
        }
        if let Some(mir) = self.try_bigint_new_dispatch(
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
        if let Some(mir) = self.try_bigint_to_string_dispatch(
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
        if let Some(mir) = self.try_bigint_to_number_dispatch(
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
                ctx.warning(
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
                ctx.warning(
                    "E0406",
                    "Array.from({length: N, 0: x, ...}, mapFn) (mixing indexed values with \
                     mapFn) is not supported in this AOT target; the call is lowered as \
                     undefined",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if args.len() > 1 && !matches!(&args[1], HirExpr::Global { .. }) {
                ctx.warning(
                    "E0406",
                    "Array.from({length: N}, mapFn[, thisArg]) in AOT requires mapFn to be a known global function reference \
                     (a top-level fn or a non-capturing closure lifted by `lower_closures`); \
                     capturing closures and arbitrary local expressions are not supported",
                    Span::new(0, 0),
                );
                return MirExpr::Unit;
            }
            if args.len() == 3 {
                ctx.warning(
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
        if let Some(mir) = self.try_array_instance_method_dispatch(
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
        if let Some(mir) = self.try_user_method_dispatch(
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
        // Parameters of the callee when this AOT can resolve them: used for arity
        // diagnostics, missing-argument padding and nullish kind checks.
        let callee_params = self.callee_params(callee, types);
        if let Some(params) = &callee_params {
            let required = params
                .iter()
                .rposition(|param| !param.optional)
                .map_or(0, |last| last.saturating_add(1));
            if args.len() < required || args.len() > params.len() {
                let name = Self::callee_display_name(callee);
                let span = match callee {
                    HirCallee::Indirect(inner) => inner.span(),
                    _ => args.first().map_or(Span::new(0, 0), |arg| arg.span()),
                };
                ctx.error(
                    "P0004",
                    format!(
                        "call to `{name}` passes {} argument(s) but `{name}` requires {} and \
                         declares {} parameter(s); passing more than declared or omitting a \
                         required argument is not supported (a parameter with a default value is \
                         optional, so it may be omitted)",
                        args.len(),
                        required,
                        params.len()
                    ),
                    span,
                );
            }
            for (i, arg) in args.iter().enumerate() {
                if let Some(param) = params.get(i) {
                    crate::hir_to_mir::nullish::check_nullish_write(
                        arg,
                        crate::hir_to_mir::nullish::argument_check_ty(param.ty, types),
                        types,
                        arg.span(),
                        ctx,
                        &format!("parameter {} of this call", i + 1),
                        false,
                    );
                }
            }
        }
        let recv = if callee_id == PLACEHOLDER_FUNCTION
            && let HirCallee::Indirect(inner) = callee
            && let Some(owner) = array_push_receiver(inner.as_ref(), types)
        {
            let recv_ty = owner.ty();
            let recv_mir = self.convert_expr(
                owner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            if is_pure_place(&recv_mir) {
                Some(recv_mir)
            } else {
                let tmp = self.fresh_local();
                self.push_temp_local(tmp, recv_ty);
                out.push(MirStmt::Let {
                    local: tmp,
                    ty: recv_ty,
                    init: Some(recv_mir),
                    mutable: true,
                });
                Some(MirExpr::Local(tmp))
            }
        } else {
            None
        };
        let arg_tys = self.callee_param_tys(callee);
        let mut mir_args: Vec<MirExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let expected = arg_tys.as_ref().and_then(|tys| tys.get(i).copied());
                self.convert_call_arg(
                    a,
                    expected,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )
            })
            .collect();
        if let Some(params) = &callee_params
            && mir_args.len() < params.len()
        {
            // Missing trailing arguments can only be `?`-optional parameters:
            // omitting a required one is already reported as P0004.
            while mir_args.len() < params.len() {
                mir_args.push(MirExpr::Unit);
            }
        }
        if callee_id == PLACEHOLDER_FUNCTION
            && let HirCallee::Indirect(inner) = callee
        {
            if let Some(mir) = self.try_known_indirect_dispatch(
                callee,
                args,
                recv,
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
        let owner_ty = self.expr_type(owner, types)?;
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
            ctx.warning(
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
        if let HirExpr::Global { name, .. } = first_arg {
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
                    PROMISE_HANDLER_UNKNOWN_DIAG,
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
                        PROMISE_HANDLER_MISMATCH_DIAG,
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
        }
        ctx.error(
            "E0507",
            format!(
                "Promise.prototype.{method} cannot be lowered yet: the generated Rust cannot \
                     satisfy the runtime handler bound, so this call is rejected instead of \
                     emitting code that does not compile (planned for P3)"
            ),
            Span::new(0, 0),
        );
        Some(MirExpr::Unit)
    }

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
                    PROMISE_HANDLER_MISMATCH_DIAG,
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
                PROMISE_HANDLER_MISMATCH_DIAG,
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
                PROMISE_HANDLER_MISMATCH_DIAG,
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
        let is_void_resolve = matches!(op, RuntimeOp::PromiseResolveStatic)
            && (args.is_empty()
                || matches!(args.first(), Some(HirExpr::Undefined(_) | HirExpr::Unit(_))));
        let skip_arity = matches!(op, RuntimeOp::PromiseResolveStatic) && args.is_empty();
        if !skip_arity {
            let expected_arity = 1usize;
            if args.len() != expected_arity {
                ctx.warning(
                    "E0406",
                    format!(
                        "Promise.{method} requires exactly {expected_arity} argument(s); got {}",
                        args.len()
                    ),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
        }
        let mir_args: Vec<MirExpr> = if args.is_empty() {
            vec![MirExpr::Unit]
        } else {
            args.iter()
                .map(|a| {
                    self.convert_call_arg(
                        a,
                        None,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    )
                })
                .collect()
        };
        if matches!(op, RuntimeOp::PromiseResolveStatic)
            && !is_void_resolve
            && args
                .first()
                .is_some_and(|arg| self.arg_adopts_promise(arg, types))
        {
            return mir_args.into_iter().next();
        }
        let ok_ty = if matches!(op, RuntimeOp::PromiseRejectStatic) {
            self.expected_ty
                .and_then(|ret| promise_ok_type(ret, types))
                .or_else(|| promise_ok_type(ty, types))
                .unwrap_or_else(|| types.intern(&Type::Error))
        } else if matches!(op, RuntimeOp::PromiseResolveStatic) {
            match self.resolve_static_ok_ty(args, ty, types, ctx) {
                Some(ok) => ok,
                None => return Some(MirExpr::Unit),
            }
        } else {
            combinator_target_ty(args, types).unwrap_or_else(|| types.intern(&Type::Error))
        };
        let result_ty = types.intern(&Type::Promise {
            ok: ok_ty,
            err: None,
        });
        let dest = self.fresh_local();
        self.push_temp_local(dest, result_ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty: result_ty,
            target_ty: Some(ok_ty),
        });
        Some(MirExpr::Local(dest))
    }

    fn callee_param_tys(&self, callee: &HirCallee) -> Option<Vec<TypeId>> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Global { name, .. } = inner.as_ref() else {
            return None;
        };
        self.named_function_param_tys(name)
    }

    pub(super) fn named_function_param_tys(&self, name: &Atom) -> Option<Vec<TypeId>> {
        self.find_named_function(name)
            .map(|f| f.params.iter().map(|p| p.ty).collect())
    }

    fn find_named_function(&self, name: &Atom) -> Option<&HirFunction> {
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(f) = self.program.find_function_by_qualified_name(&probe) {
                return Some(f);
            }
        }
        self.program.find_function_by_name(name)
    }

    /// Parameters of a callee this AOT can resolve: free functions (by name),
    /// class methods (by owner type + field name) and function values (by their
    /// `Fn` type). Returns `None` for anything else (built-in methods, unknown
    /// globals, ...), so callers never invent an arity for a call they do not
    /// understand.
    pub(super) fn callee_params(
        &self,
        callee: &HirCallee,
        types: &mut TypeTable,
    ) -> Option<Vec<CalleeParam>> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        match inner.as_ref() {
            HirExpr::Global { name, .. } => {
                let function = self.find_named_function(name)?;
                Some(
                    function
                        .params
                        .iter()
                        .map(|p| CalleeParam {
                            ty: p.ty,
                            optional: p.optional,
                        })
                        .collect(),
                )
            }
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let declarations = &self.program.declarations;
                let class_index = self
                    .class_index
                    .get_or_init(|| build_class_index(declarations));
                let resolve_global = |name: &Atom| self.global_decl_type(name);
                let owner_ty = compute_chain_owner_ty(
                    owner,
                    class_index,
                    declarations,
                    types,
                    &resolve_global,
                )?;
                let owner_ty = match types.resolve(owner_ty) {
                    Some(Type::Optional { inner, .. }) => *inner,
                    _ => owner_ty,
                };
                let path = class_index.get(&owner_ty)?;
                let class = class_at_path(declarations, path.clone())?;
                let method = class.methods.iter().find(|m| m.name == *field_name)?;
                // Methods carry a synthetic `this`/`self` parameter first.
                Some(
                    method
                        .params
                        .iter()
                        .skip(1)
                        .map(|p| CalleeParam {
                            ty: p.ty,
                            optional: p.optional,
                        })
                        .collect(),
                )
            }
            other => {
                let ty = self.expr_type(other, types)?;
                let ty = match types.resolve(ty) {
                    Some(Type::Optional { inner, .. }) => *inner,
                    _ => ty,
                };
                match types.resolve(ty) {
                    Some(fn_ty @ Type::Fn { params, .. }) => Some(
                        params
                            .iter()
                            .enumerate()
                            .map(|(i, ty)| CalleeParam {
                                ty: *ty,
                                // The right to omit an argument is declared by
                                // the signature's own flag (`?`-parameter or a
                                // default), never inferred from the parameter
                                // type: `x: T | undefined` without `?` is a
                                // required parameter whose value can be
                                // `undefined`. A short/missing flag vector means
                                // "required".
                                optional: fn_ty.fn_param_optional(i),
                            })
                            .collect(),
                    ),
                    _ => None,
                }
            }
        }
    }

    /// A human-readable name for diagnostics about a call.
    pub(super) fn callee_display_name(callee: &HirCallee) -> String {
        match callee {
            HirCallee::Indirect(inner) => match inner.as_ref() {
                HirExpr::Global { name, .. } => name.as_str().to_string(),
                HirExpr::Field { field_name, .. } => field_name.as_str().to_string(),
                _ => "the callee".to_string(),
            },
            _ => "the callee".to_string(),
        }
    }

    pub(super) fn convert_call_arg(
        &mut self,
        arg: &HirExpr,
        expected: Option<TypeId>,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let saved = self.expected_ty;
        self.expected_ty = expected;
        let mir = self.convert_expr(arg, out, shared_struct_ids, shared_next_struct, types, ctx);
        self.expected_ty = saved;
        mir
    }

    fn arg_adopts_promise(&self, arg: &HirExpr, types: &mut TypeTable) -> bool {
        if promise_ok_type(arg.ty(), types).is_some() {
            return true;
        }
        let value = self
            .value_static_ty(arg, types)
            .or_else(|| self.expr_type(arg, types));
        let Some(value) = value else {
            return false;
        };
        if matches!(types.resolve(value), Some(Type::Error)) {
            return false;
        }
        promise_ok_type(value, types).is_some()
    }

    fn resolve_static_ok_ty(
        &self,
        args: &[HirExpr],
        ty: TypeId,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<TypeId> {
        match self.static_ok_ty_quiet(args, ty, types) {
            StaticOkRecovery::Ok(ok) => Some(ok),
            StaticOkRecovery::VoidMismatch(ok) => {
                ctx.error(
                    "E0506",
                    format!(
                        "Promise.resolve() resolves to Promise<void>, but the expected type is \
                         Promise<{}>; a void promise cannot satisfy it",
                        ok.raw()
                    ),
                    Span::new(0, 0),
                );
                None
            }
            StaticOkRecovery::NullMismatch(ok) => {
                ctx.error(
                    "E0506",
                    format!(
                        "Promise.resolve(null) resolves to Promise<null>, but the expected type is \
                         Promise<{}>; a null promise cannot satisfy it",
                        ok.raw()
                    ),
                    Span::new(0, 0),
                );
                None
            }
        }
    }

    fn static_ok_ty_quiet(
        &self,
        args: &[HirExpr],
        ty: TypeId,
        types: &mut TypeTable,
    ) -> StaticOkRecovery {
        if let Some(ok) = promise_ok_type(ty, types) {
            return StaticOkRecovery::Ok(ok);
        }
        if args.is_empty() || matches!(args.first(), Some(HirExpr::Unit(_) | HirExpr::Undefined(_)))
        {
            let context_ok = self.expected_ty.and_then(|ret| promise_ok_type(ret, types));
            if let Some(ok) = context_ok
                && !matches!(
                    types.resolve(ok),
                    Some(Type::Void) | Some(Type::Error) | None
                )
            {
                return StaticOkRecovery::VoidMismatch(ok);
            }
            return StaticOkRecovery::Ok(types.intern(&Type::Void));
        }
        if matches!(args.first(), Some(HirExpr::Null(_))) {
            let produced = types.intern(&Type::Null);
            let context_ok = self.expected_ty.and_then(|ret| promise_ok_type(ret, types));
            if let Some(ok) = context_ok
                && ok != produced
                && !matches!(
                    types.resolve(ok),
                    Some(Type::Void) | Some(Type::Error) | None
                )
            {
                return StaticOkRecovery::NullMismatch(ok);
            }
            return StaticOkRecovery::Ok(produced);
        }
        let value_ty = args
            .first()
            .and_then(|arg| {
                self.value_static_ty(arg, types)
                    .or_else(|| self.expr_type(arg, types))
            })
            .filter(|value| !matches!(types.resolve(*value), Some(Type::Error)));
        let context_ok = self.expected_ty.and_then(|ret| promise_ok_type(ret, types));
        StaticOkRecovery::Ok(match (value_ty, context_ok) {
            (Some(value), Some(ok))
                if is_numeric_type(value, types) && is_numeric_type(ok, types) =>
            {
                ok
            }
            (Some(value), _) => value,
            (None, Some(ok)) => ok,
            (None, None) => types.intern(&Type::Error),
        })
    }

    pub(crate) fn value_static_ty(&self, expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
        match expr {
            HirExpr::Bool(..) => Some(types.intern(&Type::Bool)),
            HirExpr::Int(..) => Some(types.intern(&Type::I64)),
            HirExpr::Float(..) => Some(types.intern(&Type::F64)),
            HirExpr::String(..) => Some(types.intern(&Type::String)),
            HirExpr::Unit(_) | HirExpr::Null(_) | HirExpr::Undefined(_) => None,
            HirExpr::Call {
                callee: HirCallee::Indirect(inner),
                ..
            } => match inner.as_ref() {
                HirExpr::Global { name, .. } => {
                    for depth in (0..=self.namespace_path.len()).rev() {
                        let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
                        if let Some(f) = self.program.find_function_by_qualified_name(&probe) {
                            return Some(f.ret);
                        }
                    }
                    self.program
                        .find_function_by_name(name)
                        .map(|f| f.ret)
                        .or_else(|| self.recovered_static_ty(expr, types))
                }
                _ => self.recovered_static_ty(expr, types),
            },
            other if other.has_static_type() => Some(other.ty()),
            _ => None,
        }
    }

    fn recovered_static_ty(&self, expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
        if !expr.has_static_type() {
            return None;
        }
        known_static_ty(expr.ty(), types)
    }

    pub(super) fn join_expr_types(
        &self,
        a: TypeId,
        b: TypeId,
        types: &TypeTable,
    ) -> Option<TypeId> {
        if a == b {
            return Some(a);
        }
        // `cond ? <optional> : null` (and `: undefined`) is the optional itself:
        // the nullish kind is already part of the optional representation.
        let nullish = |t: TypeId| matches!(types.resolve(t), Some(Type::Null | Type::Void));
        let optional = |t: TypeId| {
            matches!(
                types.resolve(t),
                Some(Type::Optional { .. } | Type::Union { .. })
            )
        };
        if nullish(b) && optional(a) {
            return Some(a);
        }
        if nullish(a) && optional(b) {
            return Some(b);
        }
        let rank = |t: TypeId| -> Option<u8> {
            match types.resolve(t) {
                Some(Type::F32 | Type::F64) => Some(4),
                Some(Type::I64 | Type::U64) => Some(3),
                Some(Type::I32 | Type::U32) => Some(2),
                Some(Type::I16 | Type::U16) => Some(1),
                Some(Type::I8 | Type::U8) => Some(0),
                _ => None,
            }
        };
        match (rank(a), rank(b)) {
            (Some(ra), Some(rb)) => Some(if ra >= rb { a } else { b }),
            _ => None,
        }
    }

    pub(crate) fn expr_type(&self, expr: &HirExpr, types: &mut TypeTable) -> Option<TypeId> {
        let this = expr;
        match this {
            HirExpr::Bool(..) => Some(types.intern(&Type::Bool)),
            HirExpr::Int(..) => Some(types.intern(&Type::I64)),
            HirExpr::Float(..) => Some(types.intern(&Type::F64)),
            HirExpr::String(..) => Some(types.intern(&Type::String)),
            HirExpr::Unit(_) | HirExpr::Null(_) | HirExpr::Undefined(_) => None,
            HirExpr::Await { expr, .. } => {
                let inner = self.expr_type(expr, types)?;
                promise_ok_type(inner, types)
            }
            HirExpr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                let a = self.expr_type(then_branch, types)?;
                let b = self.expr_type(else_branch, types)?;
                self.join_expr_types(a, b, types)
            }
            HirExpr::Call {
                callee, args, ty, ..
            } => {
                if let HirCallee::Indirect(inner) = callee {
                    match inner.as_ref() {
                        HirExpr::Field {
                            owner, field_name, ..
                        } => {
                            if let HirExpr::Global { name, .. } = owner.as_ref()
                                && name.as_str() == "Promise"
                                && matches!(field_name.as_str(), "resolve" | "reject")
                            {
                                if field_name.as_str() == "reject" {
                                    let ok = self
                                        .expected_ty
                                        .and_then(|ret| promise_ok_type(ret, types))
                                        .or_else(|| promise_ok_type(*ty, types))
                                        .unwrap_or_else(|| types.intern(&Type::Error));
                                    return Some(types.intern(&Type::Promise { ok, err: None }));
                                }
                                if let Some(arg) = args.first()
                                    && self.arg_adopts_promise(arg, types)
                                    && let Some(adopted) = self.expr_type(arg, types)
                                {
                                    return Some(adopted);
                                }
                                return match self.static_ok_ty_quiet(args, *ty, types) {
                                    StaticOkRecovery::Ok(ok)
                                        if !matches!(types.resolve(ok), Some(Type::Error)) =>
                                    {
                                        if promise_ok_type(ok, types).is_some() {
                                            Some(ok)
                                        } else {
                                            Some(types.intern(&Type::Promise { ok, err: None }))
                                        }
                                    }
                                    _ => None,
                                };
                            }
                            if matches!(field_name.as_str(), "then" | "catch" | "finally") {
                                return None;
                            }
                        }
                        HirExpr::Global { name, .. } => {
                            for depth in (0..=self.namespace_path.len()).rev() {
                                let probe =
                                    qualified_name(&self.namespace_path[..depth], name.as_str());
                                if let Some(f) =
                                    self.program.find_function_by_qualified_name(&probe)
                                {
                                    return known_static_ty(f.ret, types);
                                }
                            }
                            return self
                                .program
                                .find_function_by_name(name)
                                .and_then(|f| known_static_ty(f.ret, types));
                        }
                        _ => {}
                    }
                }
                self.recovered_static_ty(this, types)
            }
            // An arithmetic or logical *expression* branch: the HIR carries no
            // type for these (the frontend leaves `Error`), so the type comes
            // from the operands - which is what a conditional whose branch is
            // `n * fact(n - 1)` needs.
            HirExpr::Binary {
                op, lhs, rhs, ty, ..
            } => {
                if let Some(known) = known_static_ty(*ty, types) {
                    return Some(known);
                }
                let a = self.expr_type(lhs, types);
                let b = self.expr_type(rhs, types);
                match op {
                    HirBinaryOp::Lt
                    | HirBinaryOp::Le
                    | HirBinaryOp::Gt
                    | HirBinaryOp::Ge
                    | HirBinaryOp::Eq
                    | HirBinaryOp::Ne
                    | HirBinaryOp::StrictEq
                    | HirBinaryOp::StrictNe
                    | HirBinaryOp::In
                    | HirBinaryOp::InstanceOf => Some(types.intern(&Type::Bool)),
                    // `&&`, `||` and `??` yield one of their operands.
                    HirBinaryOp::And | HirBinaryOp::Or | HirBinaryOp::Coalesce => match (a, b) {
                        (Some(a), Some(b)) => self.join_expr_types(a, b, types).or(Some(a)),
                        (Some(a), None) | (None, Some(a)) => Some(a),
                        (None, None) => None,
                    },
                    _ => match (a, b) {
                        (Some(a), Some(b)) => self.join_expr_types(a, b, types),
                        _ => None,
                    },
                }
            }
            other => self.recovered_static_ty(other, types),
        }
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
            .map(|a| {
                self.convert_call_arg(
                    a,
                    None,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                )
            })
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

    fn try_user_method_dispatch(
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
            owner, field_name, ..
        } = inner.as_ref()
        else {
            return None;
        };
        let static_sid = match owner.as_ref() {
            HirExpr::Global { name, .. } => self.lookup_class_struct_by_name(name),
            _ => None,
        };
        let (sid, is_static) = match static_sid {
            Some(sid) => (sid, true),
            None => (
                self.owner_struct_id(owner, shared_struct_ids, types)?,
                false,
            ),
        };
        let key = (sid, field_name.clone(), is_static);
        let fid = *self.method_map.get(&key)?;
        // A class may declare an instance field and a method under one name. The field is the
        // member an instance actually carries, so `k.m(...)` calls the field's value: report it
        // instead of emitting a call to a method the backend drops as unspecialized.
        if !is_static
            && self
                .field_id_lookup
                .contains_key(&(sid, field_name.clone()))
        {
            ctx.error(
                "E0418",
                format!(
                    "`{}` is an instance field, not a method; a field's value cannot be called",
                    field_name.as_str()
                ),
                owner.span(),
            );
        }
        // `c?.m(...)`: the receiver is optional, so the backend maps the call
        // over it and the result is `ret | undefined`.
        let owner_optional = matches!(types.resolve(owner.ty()), Some(Type::Optional { .. }));
        let chain_receiver =
            owner_optional && matches!(owner.as_ref(), HirExpr::OptionalChain { .. });
        if owner_optional && !is_static && !chain_receiver {
            let receiver_ty = match types.resolve(owner.ty()) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => owner.ty(),
            };
            crate::hir_to_mir::nullish::check_nullish_write(
                owner,
                receiver_ty,
                types,
                owner.span(),
                ctx,
                "the receiver of this method call",
                false,
            );
        }
        let declared_ret = self.method_ret.get(&key).copied().unwrap_or(ty);
        let call_ty = if chain_receiver {
            types.intern(&Type::Optional {
                inner: declared_ret,
                nullish: ts_aot_core::NullishKinds::UNDEFINED_ONLY,
            })
        } else {
            declared_ret
        };
        let param_tys = self.method_param_tys.get(&key).cloned();
        let param_optional = self.method_param_optional.get(&key).cloned();
        let receiver_offset = usize::from(!is_static);
        if let Some(optional) = &param_optional {
            let declared = optional.len().saturating_sub(receiver_offset);
            let required = optional
                .iter()
                .skip(receiver_offset)
                .rposition(|optional| !*optional)
                .map_or(0, |last| last.saturating_add(1));
            if args.len() < required || args.len() > declared {
                ctx.error(
                    "P0004",
                    format!(
                        "call to `{name}` passes {passed} argument(s) but `{name}` requires \
                         {required} and declares {declared} parameter(s); passing more than \
                         declared or omitting a required argument is not supported (a parameter \
                         with a default value is optional, so it may be omitted)",
                        name = field_name.as_str(),
                        passed = args.len(),
                    ),
                    inner.span(),
                );
            }
            for (i, arg) in args.iter().enumerate() {
                if let Some(param_ty) = param_tys
                    .as_ref()
                    .and_then(|tys| tys.get(i + receiver_offset))
                {
                    crate::hir_to_mir::nullish::check_nullish_write(
                        arg,
                        crate::hir_to_mir::nullish::argument_check_ty(*param_ty, types),
                        types,
                        arg.span(),
                        ctx,
                        &format!("parameter {} of this call", i + 1),
                        false,
                    );
                }
            }
        }
        let mut mir_args = Vec::with_capacity(args.len() + 1);
        if !is_static {
            mir_args.push(self.convert_expr(
                owner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        for (i, a) in args.iter().enumerate() {
            let expected = param_tys
                .as_ref()
                .and_then(|tys| tys.get(i + receiver_offset).copied());
            mir_args.push(self.convert_call_arg(
                a,
                expected,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        if let Some(optional) = &param_optional {
            // Omitting trailing `?`-optional arguments is allowed; the missing
            // slots become `Option::None`.
            let expected_args = optional.len().saturating_sub(receiver_offset);
            while mir_args.len() < expected_args + receiver_offset {
                mir_args.push(MirExpr::Unit);
            }
        }
        Some(MirExpr::Call {
            callee: fid,
            args: mir_args,
            ty: call_ty,
        })
    }

    fn resolve_accessor_target(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        shared_struct_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
    ) -> Option<(StructId, bool)> {
        let static_sid = match owner {
            HirExpr::Global { name, .. } => self.lookup_class_struct_by_name(name),
            _ => None,
        };
        let (sid, is_static) = match static_sid {
            Some(sid) => (sid, true),
            None => (
                self.owner_struct_id(owner, shared_struct_ids, types)?,
                false,
            ),
        };
        if !is_static
            && self
                .field_id_lookup
                .contains_key(&(sid, field_name.clone()))
        {
            return None;
        }
        Some((sid, is_static))
    }

    pub(super) fn resolve_accessor_getter(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        shared_struct_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
        fallback_ty: TypeId,
    ) -> Option<(FunctionId, TypeId, bool)> {
        let (sid, is_static) =
            self.resolve_accessor_target(owner, field_name, shared_struct_ids, types)?;
        let key = (sid, Atom::from(format!("__get_{field_name}")), is_static);
        let fid = *self.method_map.get(&key)?;
        let ret_ty = self.method_ret.get(&key).copied().unwrap_or(fallback_ty);
        Some((fid, ret_ty, is_static))
    }

    pub(super) fn try_accessor_getter_dispatch(
        &mut self,
        owner: &HirExpr,
        field_name: &Atom,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let (fid, call_ty, is_static) =
            self.resolve_accessor_getter(owner, field_name, shared_struct_ids, types, ty)?;
        let mut mir_args = Vec::new();
        if !is_static {
            mir_args.push(self.convert_expr(
                owner,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            ));
        }
        Some(MirExpr::Call {
            callee: fid,
            args: mir_args,
            ty: call_ty,
        })
    }

    pub(super) fn resolve_accessor_setter(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        shared_struct_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
    ) -> Option<(FunctionId, TypeId, bool)> {
        let (sid, is_static) =
            self.resolve_accessor_target(owner, field_name, shared_struct_ids, types)?;
        let key = (sid, Atom::from(format!("__set_{field_name}")), is_static);
        let fid = *self.method_map.get(&key)?;
        let value_ty = self
            .method_param_tys
            .get(&key)
            .and_then(|params| params.last().copied())?;
        Some((fid, value_ty, is_static))
    }
}
