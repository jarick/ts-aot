use std::collections::HashMap;

use ts_aot_core::{FunctionId, LocalId, Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::HirExpr;
use ts_aot_ir_mir::{MirExpr, MirLocalDecl, MirParam, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::globals::is_string_typed_source;
use crate::hir_to_mir::convert_expr::util::{
    has_potential_side_effects, is_numeric_type_for_array_len, mir_expr_ty,
};
use crate::hir_to_mir::converter::ExprConverter;

impl ExprConverter {
    /// Wraps a constructor call in `?` when the constructor may throw and the
    /// enclosing function propagates the same error type.
    fn propagate_constructor_throws(
        &mut self,
        ctor: FunctionId,
        call: MirExpr,
        ty: TypeId,
        span: Span,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let Some((name, callee_throws)) = self.function_id_throws(ctor) else {
            return call;
        };
        // `function_id_throws` names the constructor's module function
        // (`A::constructor`): the message must name the class, not the mangled
        // function.
        let ctor_name = name
            .strip_suffix("::constructor")
            .unwrap_or(name.as_str())
            .to_owned();
        let message = match self.current_throws {
            None => format!(
                "the constructor of `{ctor_name}` may throw, but the enclosing context cannot \
                 propagate it (a generator, a closure body or top-level code does not return a \
                 `Result`)"
            ),
            Some(current) if current != callee_throws => format!(
                "the constructor of `{ctor_name}` throws an error type the enclosing function does \
                 not propagate; make both throw the same type"
            ),
            Some(_) if self.try_depth > 0 => format!(
                "the constructor of `{ctor_name}` may throw, and this AOT target cannot catch an \
                 error raised inside a constructor: call `new` outside the `try`"
            ),
            Some(_) => {
                return MirExpr::Propagate {
                    expr: Box::new(call),
                    ty,
                };
            }
        };
        ctx.error(
            crate::hir_to_mir::convert_expr::call::THROW_PROPAGATION_DIAG,
            message,
            span,
        );
        call
    }

    /// Build a fresh temp local holding `ty` from the class field initializers, the way
    /// JavaScript initializes an instance before any constructor body runs, and return it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn alloc_class_instance(
        &mut self,
        ty: TypeId,
        struct_id: StructId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> ts_aot_core::LocalId {
        let mut lit_fields: Vec<(ts_aot_core::FieldId, MirExpr)> = Vec::new();
        let init_names: Vec<(ts_aot_core::Atom, ts_aot_ir_hir::HirExpr)> = self
            .program
            .class_field_inits
            .get(&ty)
            .cloned()
            .unwrap_or_default();
        for (fname, init) in init_names {
            if let Some(&fid) = self.field_id_lookup.get(&(struct_id, fname)) {
                let value = self.convert_expr(
                    &init,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                lit_fields.push((fid, value));
            }
        }
        let alloc_id = self.fresh_local();
        self.push_temp_local(alloc_id, ty);
        out.push(MirStmt::Let {
            local: alloc_id,
            ty,
            init: Some(MirExpr::StructLiteral {
                struct_id,
                fields: lit_fields,
                ty,
            }),
            mutable: true,
        });
        alloc_id
    }
    /// A class expression evaluates to a class value. This target has no separate class-object
    /// representation, so the value is a fresh instance whose fields keep their *defaults*:
    /// evaluating the class must not run the field initializers (JavaScript runs those when an
    /// instance is constructed with `new`, not when the class expression is evaluated) and it
    /// must not run the constructor. A generic class stays unemittable here, exactly as in
    /// `new`: its fields would name a type the crate does not declare, so it is reported
    /// instead.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn convert_default_construct(
        &mut self,
        ty: TypeId,
        span: ts_aot_core::Span,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if let Some(class) = self
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                ts_aot_ir_hir::HirDecl::Class(c) if c.ty == ty => Some(c),
                _ => None,
            })
            && !class.type_params.is_empty()
        {
            ctx.error(
                "E0300",
                format!(
                    "`{}` is a generic class expression; generic classes are not specialized \
                     yet, so its fields would name a type the generated crate does not declare",
                    class.name.as_str()
                ),
                span,
            );
            return MirExpr::Unit;
        }
        let struct_id = self.lookup_or_alloc_struct_id(ty, shared_struct_ids, shared_next_struct);
        let alloc_id = self.fresh_local();
        self.push_temp_local(alloc_id, ty);
        out.push(MirStmt::Let {
            local: alloc_id,
            ty,
            init: Some(MirExpr::StructLiteral {
                struct_id,
                fields: Vec::new(),
                ty,
            }),
            mutable: true,
        });
        MirExpr::Local(alloc_id)
    }
    pub(super) fn convert_new(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "Array") {
            return self.convert_new_array(
                callee,
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "Date") {
            return self.convert_new_date(
                callee,
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
        if matches!(callee, HirExpr::Global { name, .. } if name.as_str() == "ArrayBuffer") {
            return self.convert_new_array_buffer(
                callee,
                args,
                ty,
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
        }
        if let Some(mir) = self.try_typed_array_new_dispatch(
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
        if let Some(mir) = self.try_weakmap_new_dispatch(
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
        let callee_mir = self.convert_expr(
            callee,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        if has_potential_side_effects(&callee_mir) {
            out.push(MirStmt::Expr(callee_mir));
        }
        let struct_id = self.lookup_or_alloc_struct_id(ty, shared_struct_ids, shared_next_struct);
        let class_name = match callee {
            HirExpr::Global { name, .. } => Some(name.as_str().to_owned()),
            _ => None,
        };
        let is_user_class = match callee {
            HirExpr::Global { name, .. } => self.lookup_class_struct_by_name(name).is_some(),
            _ => false,
        };
        // A generic class is never specialized: its fields stay `Type::GenericParam`, which
        // has no Rust type, so the backend drops the struct. A `new` here would name a struct
        // the crate does not declare, so report it instead of emitting a dangling symbol.
        if let Some(name) = class_name.as_deref()
            && let Some(class) = self
                .program
                .declarations
                .iter()
                .find_map(|decl| match decl {
                    ts_aot_ir_hir::HirDecl::Class(c) if c.name.as_str() == name => Some(c),
                    _ => None,
                })
            && !class.type_params.is_empty()
        {
            ctx.error(
                "E0300",
                format!(
                    "`new {name}()` instantiates a generic class; generic classes are not \
                     specialized yet, so its fields would name a type the generated crate does \
                     not declare"
                ),
                callee.span(),
            );
            return MirExpr::Unit;
        }
        let ctor_key = (
            struct_id,
            ts_aot_core::Atom::new_inline("constructor"),
            false,
        );
        let ctor = self.method_map.get(&ctor_key).copied();
        // JavaScript evaluates the callee and then the arguments *before* the
        // instance exists: an argument whose evaluation has side effects must
        // therefore run before the field initializers, not just before the
        // constructor body. Converting the arguments here - ahead of the
        // initializer loop below - puts every statement an argument emits at
        // that earlier position, and pins a non-trivial argument value in a
        // temporary so it is not re-evaluated at the constructor call.
        let ctor_args: Option<Vec<MirExpr>> = if ctor.is_some() {
            let param_tys = self.method_param_tys.get(&ctor_key).cloned();
            let param_optional = self.method_param_optional.get(&ctor_key).cloned();
            let offset = 1; // the receiver
            if let Some(optional) = &param_optional {
                let declared = optional.len().saturating_sub(offset);
                let required = optional
                    .iter()
                    .skip(offset)
                    .rposition(|optional| !*optional)
                    .map_or(0, |last| last.saturating_add(1));
                if args.len() < required || args.len() > declared {
                    ctx.error(
                        "P0004",
                        format!(
                            "`new` passes {passed} argument(s) to the constructor of `{}`, \
                             which requires {required} and declares {declared} parameter(s)",
                            class_name.as_deref().unwrap_or("<anonymous class>"),
                            passed = args.len(),
                        ),
                        callee.span(),
                    );
                }
            }
            let mut materialized: Vec<MirExpr> = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                let expected = param_tys
                    .as_ref()
                    .and_then(|tys| tys.get(i + offset).copied());
                if let Some(param_ty) = expected {
                    // The ABI type of a defaulted parameter is `Option<T>`, whose
                    // outer `Option` only marks "an argument was supplied": the
                    // argument itself is a `T`, which may be nullable. The check
                    // therefore uses the same type every call site does, or a
                    // valid `new C(null)` would be reported as a kind mismatch.
                    crate::hir_to_mir::nullish::check_nullish_write(
                        a,
                        crate::hir_to_mir::nullish::argument_check_ty(param_ty, types),
                        types,
                        a.span(),
                        ctx,
                        "this constructor parameter",
                        false,
                    );
                }
                let value = self.convert_call_arg(
                    a,
                    expected,
                    out,
                    shared_struct_ids,
                    shared_next_struct,
                    types,
                    ctx,
                );
                if has_potential_side_effects(&value) {
                    // The value is pinned in a temporary so that its statement
                    // precedes the field initializers below and the value is not
                    // recomputed at the constructor call. A direct call carries a
                    // dynamic type in MIR (the emitter resolves the real type
                    // from the callee), so such an argument is declared with the
                    // type of the parameter it has to satisfy.
                    let mut value_ty = mir_expr_ty(&value);
                    if matches!(types.resolve(value_ty), None | Some(Type::Error))
                        && let Some(param_ty) = expected
                    {
                        value_ty = param_ty;
                    }
                    let temp = self.fresh_local();
                    self.push_temp_local(temp, value_ty);
                    out.push(MirStmt::Let {
                        local: temp,
                        ty: value_ty,
                        init: Some(value),
                        mutable: false,
                    });
                    materialized.push(MirExpr::Local(temp));
                } else {
                    materialized.push(value);
                }
            }
            if let Some(optional) = &param_optional {
                let declared = optional.len().saturating_sub(offset);
                while materialized.len() < declared {
                    materialized.push(MirExpr::Unit);
                }
            }
            Some(materialized)
        } else {
            // A class with no constructor of its own never converts its
            // arguments *here*: the fallback arm below decides what happens to
            // them (an arity error for a user class, side effects for a callee
            // that is lowered elsewhere), so nothing is converted twice.
            None
        };
        let alloc_id = self.alloc_class_instance(
            ty,
            struct_id,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        // JavaScript runs the field initializers and then the constructor body on
        // the freshly allocated instance; the constructor takes it by mutable
        // reference (`&mut self`) and returns nothing. The arguments were already
        // evaluated and materialized above, so the receiver is simply prepended
        // to them here.
        match (ctor, ctor_args) {
            (Some(ctor), Some(ctor_args)) => {
                let mut mir_args: Vec<MirExpr> = Vec::with_capacity(ctor_args.len() + 1);
                mir_args.push(MirExpr::Local(alloc_id));
                mir_args.extend(ctor_args);
                let void_ty = types.intern(&ts_aot_core::Type::Void);
                let call = MirExpr::Call {
                    callee: ctor,
                    args: mir_args,
                    ty: void_ty,
                };
                // A constructor that may throw returns a `Result`: without this
                // the value is dropped and the error silently disappears.
                out.push(MirStmt::Expr(self.propagate_constructor_throws(
                    ctor,
                    call,
                    void_ty,
                    callee.span(),
                    ctx,
                )));
            }
            // Only a *user* class declares whether it takes arguments: built-ins
            // (`Error`, `Map`, ...) accept their own constructor signatures, which
            // this AOT target lowers elsewhere.
            _ if !args.is_empty() && is_user_class => {
                ctx.error(
                    "P0004",
                    format!(
                        "`new` passes {} argument(s) but `{}` declares no constructor; a class \
                         without a constructor takes no arguments",
                        args.len(),
                        class_name.as_deref().unwrap_or("<anonymous class>"),
                    ),
                    callee.span(),
                );
            }
            // The callee is not lowered here, but its arguments are still
            // JavaScript expressions: evaluate each one for its side effects,
            // exactly as the callee itself is evaluated above. `convert_call_arg`
            // with no expectation is the established argument path - the `new`
            // result's expected type must not leak into an argument.
            _ if !args.is_empty() => {
                ctx.warning(
                    "E0406",
                    "constructor arguments are not supported for this `new`; the arguments are \
                     evaluated for their side effects only and the constructor is not invoked",
                    callee.span(),
                );
                for a in args {
                    let arg = self.convert_call_arg(
                        a,
                        None,
                        out,
                        shared_struct_ids,
                        shared_next_struct,
                        types,
                        ctx,
                    );
                    if has_potential_side_effects(&arg) {
                        out.push(MirStmt::Expr(arg));
                    }
                }
            }
            _ => {}
        }
        MirExpr::Local(alloc_id)
    }

    fn convert_new_array(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let _ = callee;
        let alloc_id = self.fresh_local();
        self.push_temp_local(alloc_id, ty);
        if args.is_empty() {
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreate,
                args: Vec::new(),
                dest: Some(alloc_id),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(alloc_id);
        }
        if args.len() == 1 && is_numeric_type_for_array_len(&args[0], types) {
            let len_mir = self.convert_expr(
                &args[0],
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayCreateWithLen,
                args: vec![len_mir],
                dest: Some(alloc_id),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(alloc_id);
        }
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayCreate,
            args: Vec::new(),
            dest: Some(alloc_id),
            ty,
            target_ty: None,
        });
        for a in args {
            let item_mir =
                self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::ArrayPush,
                args: vec![MirExpr::Local(alloc_id), item_mir],
                dest: None,
                ty: TypeId::from_raw(0),
                target_ty: None,
            });
        }
        MirExpr::Local(alloc_id)
    }

    pub(super) fn convert_optional_chain(
        &mut self,
        base: &HirExpr,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner = self.convert_expr(base, out, shared_struct_ids, shared_next_struct, types, ctx);
        let base_ty = crate::monomorphize::hir_expr_ty(base, types)
            .or_else(|| self.mir_expr_static_ty(&inner, types))
            .unwrap_or_else(|| types.intern(&Type::Error));
        // `k?.m()` with a `k` that cannot be nullish is exactly `k.m()`: typing the chain as an
        // `Option` makes the backend emit `.as_ref()` on a plain struct, which does not
        // compile, so only a receiver that can really be nullish keeps the chain.
        if let Some(resolved) = types.resolve(base_ty)
            && !matches!(resolved, Type::Null | Type::Void | Type::Optional { .. })
        {
            return inner;
        }
        let inner_ty = match types.resolve(base_ty) {
            Some(Type::Optional { inner, .. }) => *inner,
            _ => base_ty,
        };
        let opt_ty = types.intern(&Type::Optional {
            inner: inner_ty,
            nullish: ts_aot_core::NullishKinds::UNDEFINED_ONLY,
        });
        MirExpr::OptionalChain {
            base: Box::new(inner),
            ty: opt_ty,
        }
    }

    fn convert_new_date(
        &mut self,
        _callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        if args.is_empty() {
            let dest = self.fresh_local();
            self.push_temp_local(dest, ty);
            out.push(MirStmt::Runtime {
                op: RuntimeOp::DateNow,
                args: Vec::new(),
                dest: Some(dest),
                ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        if args.len() == 1 {
            let op = if is_string_typed_source(&args[0], types) {
                RuntimeOp::DateParse
            } else {
                RuntimeOp::DateNewFromMs
            };
            let arg = self.convert_expr(
                &args[0],
                out,
                shared_struct_ids,
                shared_next_struct,
                types,
                ctx,
            );
            let result_ty = types.intern(&Type::Date);
            let dest = self.fresh_local();
            self.push_temp_local(dest, result_ty);
            out.push(MirStmt::Runtime {
                op,
                args: vec![arg],
                dest: Some(dest),
                ty: result_ty,
                target_ty: None,
            });
            return MirExpr::Local(dest);
        }
        ctx.warning(
            "E0406",
            format!(
                "new Date(year, month, ...rest) constructor with {} positional args is not yet \
                 supported in this AOT target; only `new Date()` and `new Date(value: number | string)` \
                 are currently lowered. Pass a single epoch-millisecond value (number) or an ISO 8601 \
                 string to construct a Date from components or a timestamp.",
                args.len()
            ),
            Span::new(0, 0),
        );
        MirExpr::Unit
    }

    fn convert_new_array_buffer(
        &mut self,
        callee: &HirExpr,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let _ = callee;
        let byte_length_mir = self.convert_length_argument(
            args,
            out,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
            "new ArrayBuffer(byteLength)",
            " (per ECMAScript spec the byte length must be a non-negative integer)",
        );
        let array_buffer_ty = types.intern(&Type::ArrayBuffer);
        let dest = self.fresh_local();
        self.push_temp_local(dest, array_buffer_ty);
        out.push(MirStmt::Runtime {
            op: RuntimeOp::ArrayBufferNew,
            args: vec![byte_length_mir],
            dest: Some(dest),
            ty: array_buffer_ty,
            target_ty: None,
        });
        let _ = ty;
        MirExpr::Local(dest)
    }

    pub(super) fn convert_type_assertion(
        &mut self,
        expr: &HirExpr,
        target: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let value = self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        let value_ty = crate::hir_to_mir::convert_expr::util::hir_expr_type_id(expr)
            .or_else(|| self.mir_expr_static_ty(&value, types));
        // `x!` (and `x as T`) strip the nullish part: the AOT representation is
        // `Option<T>`, so the value must be unwrapped or the emitted code would
        // use `Option<T>` where `T` is required.
        let value_is_optional = matches!(
            value_ty.and_then(|t| types.resolve(t)),
            Some(Type::Optional { .. })
        );
        let target_is_optional = matches!(
            types.resolve(target),
            Some(Type::Optional { .. } | Type::Error) | None
        );
        if value_is_optional && !target_is_optional {
            return MirExpr::UnwrapOptional {
                expr: Box::new(value),
                ty: target,
            };
        }
        value
    }

    pub(super) fn convert_closure(
        &mut self,
        hir: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let HirExpr::Closure {
            id: closure_id,
            params,
            captures,
            body,
            ty: closure_ty,
            ..
        } = hir
        else {
            ctx.error(
                "P0005",
                "convert_closure called with non-closure",
                Span::new(0, 0),
            );
            return MirExpr::Unit;
        };
        let capture_ids: Vec<LocalId> = captures
            .iter()
            .filter_map(|c| match c {
                HirExpr::Local { id, .. } => Some(self.map_local_id(*id)),
                HirExpr::Global { name, .. } => self
                    .local_names
                    .iter()
                    .filter(|(_, n)| n.as_str() == name.as_str())
                    .map(|(id, _)| *id)
                    .max_by_key(|id| id.raw()),
                _ => None,
            })
            .collect();
        let closure_ret_ty = [Some(*closure_ty), self.expected_ty]
            .into_iter()
            .flatten()
            .find_map(|ty| match types.resolve(ty) {
                Some(Type::Fn { ret, .. })
                    if !matches!(types.resolve(*ret), None | Some(Type::Error)) =>
                {
                    Some(*ret)
                }
                _ => None,
            });
        let ret_ty = closure_ret_ty.unwrap_or(ty);
        let mut mir_params: Vec<MirParam> = Vec::with_capacity(params.len());
        let param_ids: Vec<LocalId> = (0..params.len())
            .map(|i| LocalId::from_raw(u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        let snapshot: std::collections::HashMap<LocalId, LocalId> = self.local_map.clone();
        let temp_locals_snapshot: Vec<MirLocalDecl> = std::mem::take(&mut self.temp_locals);
        for (hir_param_id, p) in param_ids.iter().zip(params.iter()) {
            let new_id = self.fresh_local();
            self.map_local_id_inplace(*hir_param_id, new_id);
            let name = self.unique_synth_local_name(new_id, "__closure_param");
            mir_params.push(MirParam {
                id: new_id,
                name,
                ty: p.ty,
            });
        }
        let saved_return_ty = self.current_return_ty;
        let saved_expected_ty = self.expected_ty;
        self.current_return_ty = closure_ret_ty
            .filter(|ret| !matches!(types.resolve(*ret), None | Some(ts_aot_core::Type::Error)))
            .or_else(|| {
                self.expected_ty
                    .and_then(|expected| match types.resolve(expected) {
                        Some(ts_aot_core::Type::Fn { ret, .. }) => Some(*ret),
                        _ => None,
                    })
                    .filter(|ret| {
                        !matches!(types.resolve(*ret), None | Some(ts_aot_core::Type::Error))
                    })
            });
        self.expected_ty = None;
        // A closure body is not a `Result`-returning function of its own, so a
        // throwing call inside it cannot be propagated (and is reported there).
        let saved_throws = self.current_throws;
        self.current_throws = None;
        let (mir_body, body_locals) = self.convert_block_with_shared_struct_ids(
            body,
            shared_struct_ids,
            shared_next_struct,
            types,
            ctx,
        );
        self.current_return_ty = saved_return_ty;
        self.expected_ty = saved_expected_ty;
        self.current_throws = saved_throws;
        self.local_map = snapshot;
        self.temp_locals = temp_locals_snapshot;
        let closure_expr = MirExpr::Closure {
            id: closure_id.raw(),
            params: mir_params,
            captures: capture_ids,
            locals: body_locals,
            body: mir_body,
            ret_ty,
            fn_ty: *closure_ty,
        };
        let dest = self.fresh_local();
        self.push_temp_local_with_mut(dest, *closure_ty, false);
        out.push(MirStmt::Let {
            local: dest,
            ty: *closure_ty,
            init: Some(closure_expr),
            mutable: false,
        });
        MirExpr::Local(dest)
    }

    pub(super) fn convert_await(
        &mut self,
        expr: &HirExpr,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        let inner = self.convert_expr(expr, out, shared_struct_ids, shared_next_struct, types, ctx);
        MirExpr::Await {
            expr: Box::new(inner),
            ty,
        }
    }

    pub(super) fn convert_yield(
        &mut self,
        expr: Option<&HirExpr>,
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> MirExpr {
        // `yield 1` in a generator declared to yield `number` has to produce an
        // `f64`, exactly as `return 1` does in an `f64` function: the declared yield
        // type is the operand's expected type. A generator lowers with
        // `Generator<inner>` as its return type, so that inner type is the yield
        // type. The `ty` the caller passes is not it - the frontend leaves the yield
        // *expression*'s own type as an error placeholder, since `yield` evaluates
        // to whatever `next()` is given.
        let yield_ty = self.current_return_ty.map(|ty| match types.resolve(ty) {
            Some(Type::Generator { inner, .. }) => *inner,
            _ => ty,
        });
        let inner = expr
            .map(|e| {
                let saved = self.expected_ty;
                self.expected_ty = yield_ty;
                let value =
                    self.convert_expr(e, out, shared_struct_ids, shared_next_struct, types, ctx);
                self.expected_ty = saved;
                match yield_ty {
                    Some(target) => {
                        let value_ty = self.mir_expr_static_ty(&value, types);
                        super::control::cast_numeric_branch_to_dest(value, value_ty, target, types)
                    }
                    None => value,
                }
            })
            .map(Box::new);
        MirExpr::Yield { expr: inner, ty }
    }
}
