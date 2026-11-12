use oxc_ast::ast::{
    Argument, AssignmentExpression, BinaryExpression, CallExpression, ChainElement,
    ChainExpression, Expression, LogicalExpression, SequenceExpression, SimpleAssignmentTarget,
    TaggedTemplateExpression, TemplateLiteral, UnaryExpression, UpdateExpression, match_expression,
    match_member_expression,
};
use oxc_ecmascript::{ToBigInt, WithoutGlobalReferenceInformation};
use oxc_span::GetSpan;
use oxc_syntax::operator::UpdateOperator;
use ts_aot_core::{
    Atom, Diagnostic, FieldId, LocalId, NullishKinds, Span, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirDecl, HirExpr, HirStmt, Visitor, walk_expr};

use crate::body::FunctionBodyInput;
use crate::ops::{
    CompoundOp, compound_op, map_binary_op, map_logical_op, map_unary_op, number_to_hir,
};
use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::util::core_span_from_oxc;

/// Return type of a closure that has no return type annotation.
///
/// A closure is typed as `Fn(..) -> T`, so an undetermined return type cannot
/// stay `Error`: the generated Rust would declare `-> ()` and then `return` a
/// value. Report it with the closure's span when the body returns something the
/// frontend cannot type.
fn infer_closure_return_ty(
    body: &[HirStmt],
    span: Span,
    fallback: TypeId,
    senv: &mut SkeletonEnv,
    what: &str,
) -> TypeId {
    match crate::util::infer_function_return_ty(body, senv.types, senv.program) {
        crate::util::ReturnInference::Inferred(ty) => ty,
        crate::util::ReturnInference::Void => senv.types.intern(&Type::Void),
        crate::util::ReturnInference::Unknown => {
            // A closure can still be typed from its context (an annotated local,
            // an argument position with an expected `Fn` type), so this is a
            // warning rather than an error.
            senv.diagnostics.push(Diagnostic::warning(
                crate::decl::RETURN_TYPE_INFERENCE_CODE,
                format!(
                    "this {what} has no return type annotation and its return type cannot be \
                     inferred from its body; give it a return type annotation (or a contextual \
                     function type) if the generated closure needs one"
                ),
                span,
            ));
            fallback
        }
    }
}
pub(crate) fn literal_hir_ty(types: &mut TypeTable, e: &HirExpr) -> Option<TypeId> {
    Some(match e {
        HirExpr::Int(..) => types.intern(&Type::I64),
        HirExpr::Float(..) => types.intern(&Type::F64),
        HirExpr::String(..) => types.intern(&Type::String),
        HirExpr::Bool(..) => types.intern(&Type::Bool),
        _ => return None,
    })
}

/// `a && b` / `a || b` yield one of their operands, so both having the same
/// statically known type makes the result that type (a literal takes the other
/// operand's type when it can represent it).
fn logical_operand_ty(senv: &mut SkeletonEnv<'_>, ops: [&HirExpr; 2]) -> TypeId {
    fn literal_fits(expr: &HirExpr, target: &Type) -> bool {
        match expr {
            // A float literal cannot be written into an integer target without
            // truncating it, and an integer literal only fits an integer type
            // whose range holds it: `300` in a `u8` is an out-of-range Rust
            // literal, and truncating it would answer differently from
            // JavaScript.
            HirExpr::Int(v, _) => match target {
                Type::F32 | Type::F64 => true,
                other => int_fits(*v, other),
            },
            HirExpr::Float(..) => matches!(target, Type::F32 | Type::F64),
            HirExpr::String(..) => matches!(target, Type::String),
            HirExpr::Bool(..) => matches!(target, Type::Bool),
            _ => false,
        }
    }
    /// Whether `value` is representable in the integer type `target` without
    /// wrapping, which is what makes it a valid Rust literal of that type.
    fn int_fits(value: i64, target: &Type) -> bool {
        match target {
            Type::I8 => i8::try_from(value).is_ok(),
            Type::I16 => i16::try_from(value).is_ok(),
            Type::I32 => i32::try_from(value).is_ok(),
            Type::I64 => true,
            Type::U8 => u8::try_from(value).is_ok(),
            Type::U16 => u16::try_from(value).is_ok(),
            Type::U32 => u32::try_from(value).is_ok(),
            Type::U64 => u64::try_from(value).is_ok(),
            _ => false,
        }
    }
    // `HirExpr::ty()` answers `TypeId::from_raw(0)` for the nodes that carry no
    // type of their own (literals), and id 0 is a *real* interned type in some
    // modules - so a literal must never be resolved through `ty()`. Ask
    // `has_static_type()` first and give a literal its type explicitly.
    let lit_f64 = senv.types.intern(&Type::F64);
    let lit_string = senv.types.intern(&Type::String);
    let lit_bool = senv.types.intern(&Type::Bool);
    let known = |e: &HirExpr| {
        if !e.has_static_type() {
            return match e {
                HirExpr::Int(..) | HirExpr::Float(..) => Some(lit_f64),
                HirExpr::String(..) => Some(lit_string),
                HirExpr::Bool(..) => Some(lit_bool),
                _ => None,
            };
        }
        let ty = e.ty();
        match senv.types.resolve(ty) {
            Some(Type::Error) | None => None,
            Some(_) => Some(ty),
        }
    };
    let resolved = |ty: TypeId| senv.types.resolve(ty).cloned();
    // A literal is a literal first: it has no type of its own, so it takes the
    // other operand's type when that type can represent it (`1` is written as
    // `1.0` or `1i64` where the other operand needs it). This has to be checked
    // before the resolved-type comparison, otherwise the literal's own default
    // type is compared against the other operand's and never matches.
    let fits_operand = |literal: &HirExpr, target: TypeId| {
        resolved(target).is_some_and(|t| literal_fits(literal, &t))
    };
    match (known(ops[0]), known(ops[1])) {
        (Some(_), Some(b)) if fits_operand(ops[0], b) => b,
        (Some(a), Some(_)) if fits_operand(ops[1], a) => a,
        (Some(a), Some(b)) if resolved(a) == resolved(b) => a,
        _ => senv.error_ty(),
    }
}

fn index_element_ty(senv: &mut SkeletonEnv, owner_ty: TypeId) -> TypeId {
    let resolved = senv.types.resolve(owner_ty).cloned();
    match resolved {
        Some(Type::Array { element }) => element,
        // `a?.[i]` is `T | undefined`: the element access is mapped over the
        // optional owner, and JavaScript yields `undefined` when the chain
        // short-circuits (never the base's own nullish value).
        Some(Type::Optional { inner, .. }) => {
            let element = match senv.types.resolve(inner).cloned() {
                Some(Type::Array { element }) => element,
                Some(Type::String) => senv.types.intern(&Type::String),
                _ => return senv.error_ty(),
            };
            // A nullable element flattens into the optional this access already
            // produces, exactly like `walk_member`'s `a?.b`: the element keeps its
            // own nullish kinds and the short-circuit adds `undefined`. Wrapping it
            // again would nest `Option<Option<_>>`, which no other layer expects.
            let (inner, nullish) = match senv.types.resolve(element).cloned() {
                Some(Type::Optional { inner, nullish }) => (
                    inner,
                    NullishKinds {
                        null: nullish.null,
                        undefined: true,
                    },
                ),
                _ => (element, NullishKinds::UNDEFINED_ONLY),
            };
            senv.types.intern(&Type::Optional { inner, nullish })
        }
        Some(Type::String) => senv.types.intern(&Type::String),
        _ => senv.error_ty(),
    }
}

fn declaration_defines(d: &HirDecl, name: &str) -> bool {
    let declared = match d {
        HirDecl::Function(f) => &f.name,
        HirDecl::Class(c) => &c.name,
        HirDecl::TypeAlias { name, .. }
        | HirDecl::Enum { name, .. }
        | HirDecl::Global { name, .. }
        | HirDecl::Interface { name, .. }
        | HirDecl::Namespace { name, .. } => name,
    };
    declared.as_str() == name
}

impl SkeletonEnv<'_> {
    pub(crate) fn ident_to_expr(&mut self, name: &str, scope: &BodyScope, span: Span) -> HirExpr {
        if name == "undefined" {
            return HirExpr::Undefined(span);
        }
        if let Some((id, ty)) = scope.lookup(name) {
            HirExpr::Local { id, ty, span }
        } else {
            // A module-level binding is typed by its own declaration: without it
            // every global reference reached the passes as `Error`, so the
            // nullish checks (`return g` from a non-null function, `??`, ...)
            // silently accepted anything.
            let ty = self
                .capture_type(name)
                .or_else(|| {
                    self.program
                        .declarations
                        .iter()
                        .find_map(|decl| match decl {
                            HirDecl::Global {
                                name: declared, ty, ..
                            } if declared.as_str() == name => Some(*ty),
                            _ => None,
                        })
                })
                .unwrap_or_else(|| self.error_ty());
            HirExpr::Global {
                name: Atom::from(name),
                ty,
                span,
            }
        }
    }
}

fn strip_nullish(ty: TypeId, types: &mut TypeTable) -> TypeId {
    match types.resolve(ty).cloned() {
        Some(Type::Optional { inner, .. }) => inner,
        Some(Type::Union { variants }) => {
            let mut kept: Vec<TypeId> = Vec::with_capacity(variants.len());
            for v in variants {
                if matches!(types.resolve(v), Some(Type::Null | Type::Void)) {
                    continue;
                }
                if !kept.contains(&v) {
                    kept.push(v);
                }
            }
            match kept.len() {
                0 => ty,
                1 => kept[0],
                _ => types.intern(&Type::Union { variants: kept }),
            }
        }
        _ => ty,
    }
}

fn hir_is_string_valued(e: &HirExpr, types: &TypeTable) -> bool {
    match e {
        HirExpr::String(..) | HirExpr::Template { .. } => true,
        HirExpr::Binary {
            op: HirBinaryOp::Add,
            lhs,
            rhs,
            ..
        } => hir_is_string_valued(lhs, types) || hir_is_string_valued(rhs, types),
        HirExpr::Ternary {
            then_branch,
            else_branch,
            ..
        } => hir_is_string_valued(then_branch, types) && hir_is_string_valued(else_branch, types),
        _ => e.has_static_type() && matches!(types.resolve(e.ty()), Some(Type::String)),
    }
}

const ASSERT_SHIM_METHODS: &[(&str, &str)] = &[
    ("sameValue", "assertSameValue"),
    ("notSameValue", "assertNotSameValue"),
];

impl SkeletonBuilder {
    pub(crate) fn walk_expr(
        &mut self,
        senv: &mut SkeletonEnv,
        e: &Expression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        match e {
            Expression::BooleanLiteral(b) => HirExpr::Bool(b.value, core_span_from_oxc(b.span)),
            Expression::NumericLiteral(n) => number_to_hir(n.value, core_span_from_oxc(n.span)),
            Expression::StringLiteral(s) => {
                HirExpr::String(Atom::from(s.value.as_str()), core_span_from_oxc(s.span))
            }
            Expression::NullLiteral(n) => HirExpr::Null(core_span_from_oxc(n.span)),
            Expression::Identifier(id) => {
                senv.ident_to_expr(id.name.as_str(), scope, core_span_from_oxc(id.span))
            }
            Expression::ThisExpression(this_expr) => {
                if let Some((id, ty)) = scope.lookup("this") {
                    HirExpr::Local {
                        id,
                        ty,
                        span: core_span_from_oxc(this_expr.span),
                    }
                } else {
                    let message = if self.in_static_method {
                        "`this` in a static method is not supported in this AOT target (a \
                         static method has no class receiver) and is lowered to `undefined`."
                    } else {
                        "`this` outside an instance method is not supported in this AOT target \
                         and is lowered to `undefined`; arrow/function-scope `this` and module \
                         top-level `this` are not modelled."
                    };
                    senv.diagnostics.push(Diagnostic::warning(
                        "P0014",
                        message,
                        core_span_from_oxc(this_expr.span),
                    ));
                    HirExpr::Undefined(core_span_from_oxc(this_expr.span))
                }
            }
            Expression::ParenthesizedExpression(p) => self.walk_expr(senv, &p.expression, scope),
            Expression::BinaryExpression(b) => self.walk_binary(senv, b, scope),
            Expression::LogicalExpression(l) => self.walk_logical(senv, l, scope),
            Expression::UnaryExpression(unary) => self.walk_unary(senv, unary, scope),
            Expression::UpdateExpression(update) => self.walk_update(senv, update, scope),
            Expression::CallExpression(call) => {
                // The call may run a closure that writes a variable captured by
                // it, so a narrowing of such a variable is no longer valid once
                // the call returns.
                let called = self.walk_call(senv, call, scope);
                self.invalidate_captured_narrowings(scope);
                called
            }
            Expression::NewExpression(new_expr) => {
                let created = self.walk_new_expression(senv, new_expr, scope);
                // A constructor may invoke callbacks (a stored function, a class
                // field) that write a captured variable, so a narrowing of it is
                // no longer valid once `new` returns - the later reads must see
                // the storage the callback wrote. A class whose constructor and
                // field initializers cannot call anything is proved harmless and
                // keeps the narrowing.
                if self.new_may_call_a_callback(senv, new_expr, scope) {
                    self.invalidate_captured_narrowings(scope);
                }
                created
            }
            other @ match_member_expression!(Expression) => {
                self.walk_member(senv, other.to_member_expression(), scope)
            }
            Expression::ChainExpression(chain) => {
                let chained = self.walk_chain(senv, chain, scope);
                // `o?.m()` / `o.m?.()` are calls: like a plain call they may run
                // a closure that writes a captured variable.
                if matches!(&chain.expression, ChainElement::CallExpression(_)) {
                    self.invalidate_captured_narrowings(scope);
                }
                chained
            }
            Expression::AssignmentExpression(a) => self.walk_assignment(senv, a, scope),
            Expression::AwaitExpression(a) => {
                let inner = self.walk_expr(senv, &a.argument, scope);
                // `await` hands control back to the event loop: the continuation
                // runs arbitrary user code, so a narrowing of a captured variable
                // may be stale once the awaited value arrives (like a call).
                self.invalidate_captured_narrowings(scope);
                let ty = senv.error_ty();
                HirExpr::Await {
                    expr: Box::new(inner),
                    ty,
                    span: core_span_from_oxc(a.span),
                }
            }
            Expression::YieldExpression(y) => self.walk_yield_expression(senv, y, scope),
            Expression::TemplateLiteral(t) => self.walk_template_literal(senv, t, scope),
            Expression::TaggedTemplateExpression(t) => {
                let tagged = self.walk_tagged_template_expression(senv, t, scope);
                // A tagged template calls its tag function.
                self.invalidate_captured_narrowings(scope);
                tagged
            }
            Expression::ArrayExpression(arr) => self.walk_array_expression(senv, arr, scope),
            Expression::ObjectExpression(obj) => self.walk_object_expression(senv, obj, scope),
            Expression::ConditionalExpression(cond) => {
                self.walk_conditional_expression(senv, cond, scope)
            }
            Expression::SequenceExpression(seq) => self.walk_sequence_expression(senv, seq, scope),
            Expression::ClassExpression(class_expr) => {
                self.walk_class_expression(senv, class_expr, scope)
            }
            Expression::RegExpLiteral(re) => {
                let pattern = Atom::from(re.regex.pattern.text.as_str());
                let flags = Atom::from(re.regex.flags.to_string().as_str());
                let ty = senv.error_ty();
                HirExpr::RegExp {
                    pattern,
                    flags,
                    ty,
                    span: core_span_from_oxc(re.span),
                }
            }
            Expression::BigIntLiteral(big_int) => {
                let value = big_int
                    .to_big_int(&WithoutGlobalReferenceInformation)
                    .map_or_else(
                        || Atom::from(big_int.value.as_str()),
                        |bi| Atom::from(bi.to_string()),
                    );
                let ty = senv.bigint_ty();
                HirExpr::BigInt {
                    value,
                    ty,
                    span: core_span_from_oxc(big_int.span),
                }
            }
            Expression::ImportExpression(imp) => {
                if imp.options.is_some() {
                    senv.report_unwalked(
                        "dynamic import() with options (e.g. { with: { ... } }) is not supported by the body walker",
                        imp.span,
                    );
                }
                if imp.phase.is_some() {
                    senv.report_unwalked(
                        "dynamic import() with explicit phase (e.g. import.source) is not supported by the body walker",
                        imp.span,
                    );
                }
                let source = self.walk_expr(senv, &imp.source, scope);
                let ty = senv.error_ty();
                HirExpr::Import {
                    source: Box::new(source),
                    ty,
                    span: core_span_from_oxc(imp.span),
                }
            }
            Expression::ArrowFunctionExpression(arrow) => self.walk_arrow(senv, arrow, scope),
            Expression::FunctionExpression(func) => {
                self.walk_function_expression(senv, func, scope)
            }
            Expression::TSAsExpression(as_expr) => {
                let expr = self.walk_expr(senv, &as_expr.expression, scope);
                let merged = self.merged_type_params();
                let target = self.resolve_ts_type_with_params(
                    senv,
                    Some(&as_expr.type_annotation),
                    merged.as_ref(),
                );
                HirExpr::TypeAssertion {
                    expr: Box::new(expr),
                    target,
                    span: core_span_from_oxc(as_expr.span),
                }
            }
            Expression::TSTypeAssertion(type_assertion) => {
                let expr = self.walk_expr(senv, &type_assertion.expression, scope);
                let merged = self.merged_type_params();
                let target = self.resolve_ts_type_with_params(
                    senv,
                    Some(&type_assertion.type_annotation),
                    merged.as_ref(),
                );
                HirExpr::TypeAssertion {
                    expr: Box::new(expr),
                    target,
                    span: core_span_from_oxc(type_assertion.span),
                }
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                senv.report_unwalked(
                    "`satisfies` expressions are not supported by the body walker; the \
                     assignability constraint is not checked and is dropped (the expression's \
                     runtime value is unchanged)",
                    satisfies.span,
                );
                self.walk_expr(senv, &satisfies.expression, scope)
            }
            Expression::TSNonNullExpression(non_null) => {
                let expr = self.walk_expr(senv, &non_null.expression, scope);
                let target = strip_nullish(expr.ty(), senv.types);
                HirExpr::TypeAssertion {
                    expr: Box::new(expr),
                    target,
                    span: core_span_from_oxc(non_null.span),
                }
            }
            other => {
                senv.report_unwalked(
                    "expression form is not supported by the body walker",
                    other.span(),
                );
                HirExpr::Unit(core_span_from_oxc(other.span()))
            }
        }
    }

    fn walk_template_parts(
        &mut self,
        senv: &mut SkeletonEnv,
        quasis: &[oxc_ast::ast::TemplateElement<'_>],
        expressions: &[Expression<'_>],
        scope: &mut BodyScope,
    ) -> (Vec<HirExpr>, Vec<Option<Atom>>, Vec<Option<Atom>>) {
        let mut exprs = Vec::with_capacity(expressions.len());
        let mut cooked_parts = Vec::with_capacity(quasis.len());
        let mut raw_parts = Vec::with_capacity(quasis.len());
        for (i, q) in quasis.iter().enumerate() {
            let cooked = q.value.cooked.as_ref().map(|s| Atom::from(s.as_str()));
            let raw = Some(Atom::from(q.value.raw.as_str()));
            cooked_parts.push(cooked);
            raw_parts.push(raw);
            if i < expressions.len() {
                exprs.push(self.walk_expr(senv, &expressions[i], scope));
            }
        }
        (exprs, cooked_parts, raw_parts)
    }

    fn walk_template_literal(
        &mut self,
        senv: &mut SkeletonEnv,
        t: &TemplateLiteral<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let (expressions, cooked_parts, raw_parts) =
            self.walk_template_parts(senv, &t.quasis, &t.expressions, scope);
        let ty = senv.error_ty();
        HirExpr::Template {
            tag: None,
            expressions,
            cooked_parts,
            raw_parts,
            ty,
            span: core_span_from_oxc(t.span),
        }
    }

    fn walk_arrow(
        &mut self,
        senv: &mut SkeletonEnv,
        arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        if arrow.r#async {
            senv.diagnostics.push(Diagnostic::warning(
                "E0502",
                "async arrow functions are lowered as synchronous closures in this AOT \
                 target; `await` results are not resolved before yielding",
                core_span_from_oxc(arrow.span),
            ));
        }
        if arrow.type_parameters.is_some() {
            senv.diagnostics.push(Diagnostic::error(
                "E0502",
                "generic arrow functions (<T>(x: T) => ...) are not supported in this PR",
                core_span_from_oxc(arrow.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(arrow.span));
        }
        if arrow.params.rest.is_some() {
            senv.report_unwalked(
                "rest parameters (...args) are not supported in this AOT target; the rest \
                 binding is ignored",
                arrow.span,
            );
        }
        crate::util::validate_unique_param_bindings(senv, &arrow.params, true);
        let prev_strict = self.strict;
        self.strict = prev_strict || crate::util::body_has_use_strict(Some(&arrow.body));
        let merged = self.merged_type_params();
        let param_base = scope.peek_next();
        let (params, extra_locals) =
            self.build_params_and_locals(senv, &arrow.params.items, param_base, merged.as_ref());
        let ret_ty = match arrow.return_type.as_deref() {
            Some(ann) => match merged.as_ref() {
                Some(map) => {
                    self.resolve_ts_type_from_annotation_with_params(senv, Some(ann), Some(map))
                }
                None => self.resolve_ts_type_from_annotation(senv, Some(ann)),
            },
            None => senv.error_ty(),
        };
        let narrowed = self.narrowed_stack.last().cloned().unwrap_or_default();
        // A closure body runs only when the closure is called, so it is never the
        // expression of the statement that *creates* the closure: a narrowed
        // update inside it must not be recorded as that statement's copy back.
        let outer_statement_expr = self.own_statement_expr;
        self.own_statement_expr = false;
        senv.capture_types.push(capture_type_map(scope, &narrowed));
        let mut body: Vec<HirStmt> = if arrow.expression {
            let inner_base = params
                .iter()
                .map(|p| p.id.raw())
                .max()
                .map_or(param_base, |m| m.saturating_add(1));
            let mut inner_scope = BodyScope::new(inner_base);
            let mut prelude = self.declare_params_with_prelude(
                senv,
                &mut inner_scope,
                &params,
                &extra_locals,
                &arrow.params.items,
                0,
            );
            let mut stmts = self.walk_stmts(senv, &arrow.body.statements, &mut inner_scope);
            if let Some(HirStmt::Expr { expr }) = stmts.first().cloned() {
                stmts[0] = HirStmt::Return { value: Some(expr) };
            }
            scope.set_next(inner_scope.peek_next());
            prelude.extend(stmts);
            prelude
        } else {
            let (b, used) = self.walk_function_body(
                senv,
                &FunctionBodyInput {
                    body: Some(&arrow.body),
                    params: &params,
                    extra_locals: &extra_locals,
                    items: &arrow.params.items,
                    item_offset: 0,
                    param_base,
                },
                false,
                true,
            );
            scope.set_next(used);
            b
        };
        senv.capture_types.pop();
        self.own_statement_expr = outer_statement_expr;
        self.strict = prev_strict;
        let captures = collect_capture_refs(&mut body, scope, &narrowed);
        let written = written_targets(&body);
        self.withdraw_captured_narrowings(scope, &captures, &written);
        let ret_ty = if arrow.return_type.is_none() && !arrow.r#async {
            infer_closure_return_ty(
                &body,
                core_span_from_oxc(arrow.span),
                ret_ty,
                senv,
                "arrow function",
            )
        } else {
            ret_ty
        };
        let param_types: Vec<TypeId> = params.iter().map(|p| p.ty).collect();
        // The closure's own value type carries which parameters a caller may
        // omit: `?`-parameters and defaulted ones (`HirParam.optional`, set by
        // `build_params`), so an indirect call checks the same flags a direct one
        // does instead of guessing from an `undefined` in the parameter type.
        let param_optional: Vec<bool> = params.iter().map(|p| p.optional).collect();
        let fn_ty = senv.types.intern(&Type::Fn {
            params: param_types,
            optional: param_optional,
            ret: ret_ty,
            err: None,
        });
        let id = LocalId::from_raw(self.next_closure_id);
        self.next_closure_id += 1;
        HirExpr::Closure {
            id,
            params,
            captures,
            body,
            ty: fn_ty,
            span: core_span_from_oxc(arrow.span),
        }
    }

    fn walk_function_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &oxc_ast::ast::Function<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let span = core_span_from_oxc(func.span);
        let unsupported = func.r#async || func.generator || func.type_parameters.is_some();
        if unsupported {
            senv.report_unwalked(
                "function expression form is not supported by the body walker",
                func.span,
            );
            return HirExpr::Unit(span);
        }
        if func.params.rest.is_some() {
            senv.report_unwalked(
                "rest parameters (...args) are not supported in this AOT target; the rest \
                 binding is ignored",
                func.span,
            );
        }
        let strict = self.strict || crate::util::body_has_use_strict(func.body.as_deref());
        crate::util::validate_unique_param_bindings(senv, &func.params, strict);
        let prev_strict = self.strict;
        self.strict = strict;
        let merged = self.merged_type_params();
        let param_base = scope.peek_next();
        let (params, extra_locals) =
            self.build_params_and_locals(senv, &func.params.items, param_base, merged.as_ref());
        let ret_ty = match func.return_type.as_deref() {
            Some(ann) => match merged.as_ref() {
                Some(map) => {
                    self.resolve_ts_type_from_annotation_with_params(senv, Some(ann), Some(map))
                }
                None => self.resolve_ts_type_from_annotation(senv, Some(ann)),
            },
            None => senv.error_ty(),
        };
        let narrowed = self.narrowed_stack.last().cloned().unwrap_or_default();
        // See the arrow case: a function body is not the creating statement's
        // own expression, so its narrowed updates get their own copy back.
        let outer_statement_expr = self.own_statement_expr;
        self.own_statement_expr = false;
        senv.capture_types.push(capture_type_map(scope, &narrowed));
        let (mut body, used) = self.walk_function_body(
            senv,
            &FunctionBodyInput {
                body: func.body.as_deref(),
                params: &params,
                extra_locals: &extra_locals,
                items: &func.params.items,
                item_offset: 0,
                param_base,
            },
            false,
            false,
        );
        scope.set_next(used);
        senv.capture_types.pop();
        self.own_statement_expr = outer_statement_expr;
        self.strict = prev_strict;
        let captures = collect_capture_refs(&mut body, scope, &narrowed);
        let written = written_targets(&body);
        self.withdraw_captured_narrowings(scope, &captures, &written);
        let ret_ty = if func.return_type.is_none() && !func.r#async && !func.generator {
            infer_closure_return_ty(
                &body,
                core_span_from_oxc(func.span),
                ret_ty,
                senv,
                "function expression",
            )
        } else {
            ret_ty
        };
        let param_types: Vec<TypeId> = params.iter().map(|p| p.ty).collect();
        let param_optional: Vec<bool> = params.iter().map(|p| p.optional).collect();
        let fn_ty = senv.types.intern(&Type::Fn {
            params: param_types,
            optional: param_optional,
            ret: ret_ty,
            err: None,
        });
        let id = LocalId::from_raw(self.next_closure_id);
        self.next_closure_id += 1;
        HirExpr::Closure {
            id,
            params,
            captures,
            body,
            ty: fn_ty,
            span,
        }
    }

    fn walk_yield_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        y: &oxc_ast::ast::YieldExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        if !self.current_function_is_generator() {
            senv.diagnostics.push(Diagnostic::error(
                "E0500",
                "`yield` is only valid inside a generator function (`function*`); \
                 `yield` in non-generator context is rejected by the body walker.",
                core_span_from_oxc(y.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(y.span));
        }
        if y.delegate {
            senv.diagnostics.push(Diagnostic::warning(
                "E0501",
                "`yield*` (delegating yield) is not supported in this AOT target; \
                 the delegated iterable is evaluated but its values are not forwarded",
                core_span_from_oxc(y.span),
            ));
            let delegated = match y.argument.as_ref() {
                Some(argument) => self.walk_expr(senv, argument, scope),
                None => HirExpr::Unit(core_span_from_oxc(y.span)),
            };
            // A `yield` suspends the generator and the caller resumes it, so the
            // same captured-narrowing invalidation as a call applies.
            self.invalidate_captured_narrowings(scope);
            return delegated;
        }
        let inner = y.argument.as_ref().map(|a| self.walk_expr(senv, a, scope));
        // A `yield` suspends the generator and the caller resumes it, so the same
        // captured-narrowing invalidation as a call applies.
        self.invalidate_captured_narrowings(scope);
        let ty = senv.error_ty();
        HirExpr::Yield {
            expr: inner.map(Box::new),
            ty,
            span: core_span_from_oxc(y.span),
        }
    }

    fn walk_tagged_template_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        t: &TaggedTemplateExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let tag = self.walk_expr(senv, &t.tag, scope);
        let (expressions, cooked_parts, raw_parts) =
            self.walk_template_parts(senv, &t.quasi.quasis, &t.quasi.expressions, scope);
        let ty = senv.error_ty();
        HirExpr::Template {
            tag: Some(Box::new(tag)),
            expressions,
            cooked_parts,
            raw_parts,
            ty,
            span: core_span_from_oxc(t.span),
        }
    }

    fn walk_array_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        arr: &oxc_ast::ast::ArrayExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::ArrayExpressionElement;
        enum RawPart {
            Literal(HirExpr),
            Spread(HirExpr),
            Elision,
        }
        let has_spread = arr
            .elements
            .iter()
            .any(|el| matches!(el, ArrayExpressionElement::SpreadElement(_)));
        if !has_spread {
            return self.walk_array_literal_simple(senv, arr, scope);
        }
        let span = core_span_from_oxc(arr.span);
        let mut raw: Vec<RawPart> = Vec::new();
        let mut element_ty_id: Option<TypeId> = None;
        for el in &arr.elements {
            match el {
                ArrayExpressionElement::Elision(_) => raw.push(RawPart::Elision),
                ArrayExpressionElement::SpreadElement(spread) => {
                    let arg = self.walk_expr(senv, &spread.argument, scope);
                    let spread_elem_ty = match senv.types.resolve(arg.ty()) {
                        Some(Type::Array { element }) => Some(*element),
                        _ => None,
                    };
                    if let Some(new_ty) = spread_elem_ty {
                        match element_ty_id {
                            Some(existing) if existing != new_ty => {
                                senv.report_unwalked(
                                    "array spread operands have incompatible element types",
                                    spread.span,
                                );
                                return HirExpr::Unit(span);
                            }
                            _ => element_ty_id = Some(new_ty),
                        }
                    }
                    raw.push(RawPart::Spread(arg));
                }
                other => {
                    let elem = self.walk_expr(senv, other.to_expression(), scope);
                    raw.push(RawPart::Literal(elem));
                }
            }
        }
        let Some(element_ty_id) = element_ty_id else {
            senv.report_unwalked(
                "array spread requires at least one spread of a typed array \
                 to determine the element type",
                arr.span,
            );
            return HirExpr::Unit(span);
        };
        let arr_ty = senv.types.intern(&Type::Array {
            element: element_ty_id,
        });
        let mut parts: Vec<HirExpr> = Vec::new();
        for r in raw {
            match r {
                RawPart::Literal(elem) => parts.push(HirExpr::ArrayLiteral {
                    elements: vec![elem],
                    ty: arr_ty,
                    span,
                }),
                RawPart::Spread(arg) => parts.push(arg),
                RawPart::Elision => parts.push(HirExpr::Call {
                    callee: HirCallee::Runtime {
                        name: Atom::from("__ts_aot_array_hole"),
                        ty: arr_ty,
                    },
                    args: Vec::new(),
                    type_args: Vec::new(),
                    ty: arr_ty,
                    span,
                }),
            }
        }
        let result_ty = senv.types.intern(&Type::Array {
            element: element_ty_id,
        });
        HirExpr::Call {
            callee: HirCallee::Runtime {
                name: Atom::from("__ts_aot_array_concat"),
                ty: result_ty,
            },
            args: parts,
            type_args: Vec::new(),
            ty: result_ty,
            span,
        }
    }

    fn walk_array_literal_simple(
        &mut self,
        senv: &mut SkeletonEnv,
        arr: &oxc_ast::ast::ArrayExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::ArrayExpressionElement;
        let mut elements = Vec::with_capacity(arr.elements.len());
        for el in &arr.elements {
            match el {
                ArrayExpressionElement::Elision(elision) => {
                    elements.push(HirExpr::Undefined(core_span_from_oxc(elision.span)));
                }
                ArrayExpressionElement::SpreadElement(_) => {
                    return HirExpr::Unit(core_span_from_oxc(arr.span));
                }
                el @ match_expression!(ArrayExpressionElement) => {
                    elements.push(self.walk_expr(senv, el.to_expression(), scope));
                }
            }
        }
        let mut inferred: Option<ts_aot_core::TypeId> = None;
        let mut inconclusive = false;
        for e in &elements {
            let ty = match e {
                HirExpr::Int(..) => Some(senv.types.intern(&ts_aot_core::Type::I64)),
                HirExpr::Float(..) => Some(senv.types.intern(&ts_aot_core::Type::F64)),
                HirExpr::String(..) => Some(senv.types.intern(&ts_aot_core::Type::String)),
                HirExpr::Bool(..) => Some(senv.types.intern(&ts_aot_core::Type::Bool)),
                _ => {
                    let t = e.ty();
                    (t != ts_aot_core::TypeId::from_raw(0)).then_some(t)
                }
            };
            let Some(ty) = ty else {
                inconclusive = true;
                break;
            };
            match inferred {
                None => inferred = Some(ty),
                Some(prev) if prev == ty => {}
                Some(_) => {
                    inconclusive = true;
                    break;
                }
            }
        }
        let element_ty = if inconclusive {
            senv.error_ty()
        } else {
            inferred.unwrap_or_else(|| senv.error_ty())
        };
        let ty = if matches!(
            senv.types.resolve(element_ty),
            Some(ts_aot_core::Type::Error)
        ) {
            senv.error_ty()
        } else {
            senv.types.intern(&ts_aot_core::Type::Array {
                element: element_ty,
            })
        };
        HirExpr::ArrayLiteral {
            elements,
            ty,
            span: core_span_from_oxc(arr.span),
        }
    }

    fn walk_object_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        obj: &oxc_ast::ast::ObjectExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::{ObjectPropertyKind, PropertyKey, PropertyKind};
        use ts_aot_ir_hir::ObjectLiteralField;
        let mut fields = Vec::with_capacity(obj.properties.len());
        for prop in &obj.properties {
            match prop {
                ObjectPropertyKind::SpreadProperty(spread) => {
                    senv.report_unwalked(
                        "object spread property is not supported by the body walker (planned for PR 7.7)",
                        spread.span,
                    );
                    let value = self.walk_expr(senv, &spread.argument, scope);
                    fields.push(ObjectLiteralField::Spread(value));
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    if p.kind != PropertyKind::Init {
                        senv.report_unwalked(
                            "object accessor (get/set) property is not supported by the body walker",
                            p.span,
                        );
                        continue;
                    }
                    let name = match &p.key {
                        PropertyKey::StaticIdentifier(ident) => Atom::from(ident.name.as_str()),
                        PropertyKey::StringLiteral(s) => Atom::from(s.value.as_str()),
                        PropertyKey::NumericLiteral(n) => Atom::from(n.value.to_string().as_str()),
                        key @ match_expression!(PropertyKey) => {
                            senv.report_unwalked(
                                "object computed property key is not supported by the body walker",
                                p.key.span(),
                            );
                            self.walk_expr(senv, key.to_expression(), scope);
                            self.walk_expr(senv, &p.value, scope);
                            continue;
                        }
                        _ => {
                            senv.report_unwalked(
                                "object computed property key is not supported by the body walker",
                                p.key.span(),
                            );
                            self.walk_expr(senv, &p.value, scope);
                            continue;
                        }
                    };
                    let value = self.walk_expr(senv, &p.value, scope);
                    fields.push(ObjectLiteralField::Property { name, value });
                }
            }
        }
        let ty = senv.error_ty();
        HirExpr::ObjectLiteral {
            fields,
            ty,
            span: core_span_from_oxc(obj.span),
        }
    }

    fn walk_conditional_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        c: &oxc_ast::ast::ConditionalExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let cond = self.walk_expr(senv, &c.test, scope);
        let then_branch = self.walk_expr(senv, &c.consequent, scope);
        let else_branch = self.walk_expr(senv, &c.alternate, scope);
        let ty = if hir_is_string_valued(&then_branch, senv.types)
            && hir_is_string_valued(&else_branch, senv.types)
        {
            senv.types.intern(&ts_aot_core::Type::String)
        } else {
            senv.error_ty()
        };
        HirExpr::Ternary {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
            ty,
            span: core_span_from_oxc(c.span),
        }
    }

    fn walk_sequence_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        seq: &SequenceExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let exprs: Vec<HirExpr> = seq
            .expressions
            .iter()
            .map(|e| self.walk_expr(senv, e, scope))
            .collect();
        let ty = senv.error_ty();
        HirExpr::Sequence {
            exprs,
            ty,
            span: core_span_from_oxc(seq.span),
        }
    }

    fn walk_class_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        class_expr: &oxc_ast::ast::Class<'_>,
        _scope: &mut BodyScope,
    ) -> HirExpr {
        let module_id = senv.program.module.raw();
        let seq = self.next_anon_class_id;
        self.next_anon_class_id = self.next_anon_class_id.saturating_add(1);
        let unique_name = Atom::from(format!("__class_m{module_id}_{seq}"));
        let mut hir_class = self.build_class(senv, class_expr, false);
        hir_class.name = unique_name.clone();
        senv.program
            .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
        let ty = senv.error_ty();
        HirExpr::Global {
            name: unique_name,
            ty,
            span: core_span_from_oxc(class_expr.span),
        }
    }

    fn walk_binary(
        &mut self,
        senv: &mut SkeletonEnv,
        b: &BinaryExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let lhs = self.walk_expr(senv, &b.left, scope);
        let rhs = self.walk_expr(senv, &b.right, scope);
        let ty = senv.error_ty();
        if let Some(op) = map_binary_op(b.operator) {
            HirExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty,
                span: core_span_from_oxc(b.span),
            }
        } else {
            senv.report_unwalked(
                "binary operator is not supported by the body walker",
                b.span,
            );
            HirExpr::Unit(core_span_from_oxc(b.span))
        }
    }

    fn walk_logical(
        &mut self,
        senv: &mut SkeletonEnv,
        l: &LogicalExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let lhs = self.walk_expr(senv, &l.left, scope);
        let rhs = self.walk_expr(senv, &l.right, scope);
        let op = map_logical_op(l.operator);
        // `a && b` / `a || b` yield one of their operands: see `logical_operand_ty`.
        let ty = if op == HirBinaryOp::Coalesce {
            // `a ?? b` yields the non-nullish part of `a`, or `b`; when `b` is
            // itself nullable the result keeps the shared inner type but takes
            // the *fallback's* nullish kinds (`T | null ?? T | undefined` is
            // `T | undefined`, not `T | null`).
            let lhs_ty = lhs.ty();
            let lhs_resolved = senv.types.resolve(lhs_ty).cloned();
            match lhs_resolved {
                Some(Type::Optional { inner, .. }) => {
                    let rhs_resolved = senv.types.resolve(rhs.ty()).cloned();
                    match rhs_resolved {
                        Some(Type::Optional {
                            inner: rhs_inner,
                            nullish: rhs_kinds,
                        }) if rhs_inner == inner => senv.types.intern(&Type::Optional {
                            inner: rhs_inner,
                            nullish: rhs_kinds,
                        }),
                        Some(Type::Null | Type::Void) => lhs_ty,
                        _ => inner,
                    }
                }
                _ => lhs_ty,
            }
        } else if matches!(op, HirBinaryOp::And | HirBinaryOp::Or) {
            logical_operand_ty(senv, [&lhs, &rhs])
        } else {
            senv.error_ty()
        };
        HirExpr::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
            span: core_span_from_oxc(l.span),
        }
    }

    fn walk_unary(
        &mut self,
        senv: &mut SkeletonEnv,
        unary: &UnaryExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let inner = self.walk_expr(senv, &unary.argument, scope);
        match map_unary_op(unary.operator) {
            Some(op) => {
                let ty = senv.error_ty();
                HirExpr::Unary {
                    op,
                    expr: Box::new(inner),
                    ty,
                    span: core_span_from_oxc(unary.span),
                }
            }
            None => inner,
        }
    }

    fn wrap_optional_chain(inner: HirExpr, span: Span) -> HirExpr {
        let ty = inner.ty();
        HirExpr::OptionalChain {
            base: Box::new(inner),
            ty,
            span,
        }
    }

    fn walk_update(
        &mut self,
        senv: &mut SkeletonEnv,
        update: &UpdateExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let target = self.walk_simple_target(senv, &update.argument, scope);
        let op = match update.operator {
            UpdateOperator::Increment => HirBinaryOp::Add,
            UpdateOperator::Decrement => HirBinaryOp::Sub,
        };
        let ty = senv.error_ty();
        self.record_narrowed_update(&target, update.span, senv);
        HirExpr::CompoundUpdate {
            target: Box::new(target),
            op,
            rhs: Box::new(HirExpr::Int(1, Span::default())),
            post: !update.prefix,
            ty,
            span: core_span_from_oxc(update.span),
        }
    }

    fn walk_call(
        &mut self,
        senv: &mut SkeletonEnv,
        call: &CallExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let callee_expr = self.walk_expr(senv, &call.callee, scope);
        let callee_expr = if call.optional {
            Self::wrap_optional_chain(callee_expr, core_span_from_oxc(call.callee.span()))
        } else {
            callee_expr
        };
        let mut args = Vec::with_capacity(call.arguments.len());
        for arg in &call.arguments {
            match arg {
                arg @ match_expression!(Argument) => {
                    args.push(self.walk_expr(senv, arg.to_expression(), scope));
                }
                _ => {
                    senv.report_unwalked("spread argument is not supported", arg.span());
                }
            }
        }
        let type_args = if let Some(type_params) = &call.type_arguments {
            let merged = self.merged_type_params();
            type_params
                .params
                .iter()
                .map(|tp| self.resolve_ts_type_with_params(senv, Some(tp), merged.as_ref()))
                .collect()
        } else {
            vec![]
        };
        let ty = match &callee_expr {
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let object_static = matches!(
                    owner.as_ref(),
                    HirExpr::Global { name, .. } if name.as_str() == "Object"
                );
                if object_static && matches!(field_name.as_str(), "keys" | "getOwnPropertyNames") {
                    let string_ty = senv.types.intern(&ts_aot_core::Type::String);
                    senv.types
                        .intern(&ts_aot_core::Type::Array { element: string_ty })
                } else {
                    senv.error_ty()
                }
            }
            _ => senv.error_ty(),
        };
        HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(callee_expr)),
            args,
            type_args,
            ty,
            span: core_span_from_oxc(call.span),
        }
    }

    fn walk_new_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        new_expr: &oxc_ast::ast::NewExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let callee_expr = self.walk_expr(senv, &new_expr.callee, scope);
        let mut args = Vec::with_capacity(new_expr.arguments.len());
        for arg in &new_expr.arguments {
            match arg {
                arg @ match_expression!(Argument) => {
                    args.push(self.walk_expr(senv, arg.to_expression(), scope));
                }
                _ => {
                    senv.report_unwalked("spread argument is not supported in `new`", arg.span());
                }
            }
        }
        let date_shadowed = senv
            .program
            .declarations
            .iter()
            .any(|d| declaration_defines(d, "Date"));
        let ty = match &new_expr.callee {
            Expression::Identifier(id)
                if id.name.as_str() == "Date"
                    && scope.lookup("Date").is_none()
                    && !date_shadowed =>
            {
                senv.types.intern(&ts_aot_core::Type::Date)
            }
            Expression::Identifier(id) if scope.lookup(id.name.as_str()).is_none() => {
                match self.class_types.get(id.name.as_str()) {
                    Some(&class_ty) => class_ty,
                    None => senv.error_ty(),
                }
            }
            _ => senv.error_ty(),
        };
        HirExpr::New {
            callee: Box::new(callee_expr),
            args,
            ty,
            span: core_span_from_oxc(new_expr.span),
        }
    }

    fn walk_member(
        &mut self,
        senv: &mut SkeletonEnv,
        m: &oxc_ast::ast::MemberExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::MemberExpression as ME;
        match m {
            ME::StaticMemberExpression(s) => {
                if let Expression::Identifier(obj) = &s.object
                    && obj.name.as_str() == "assert"
                    && scope.lookup("assert").is_none()
                {
                    let target = ASSERT_SHIM_METHODS
                        .iter()
                        .find(|(method, _)| *method == s.property.name.as_str())
                        .map(|(_, target)| *target);
                    if let Some(target) = target {
                        return HirExpr::Global {
                            name: Atom::new_inline(target),
                            ty: senv.error_ty(),
                            span: core_span_from_oxc(s.span),
                        };
                    }
                }
                let owner = self.walk_expr(senv, &s.object, scope);
                let owner = if s.optional {
                    Self::wrap_optional_chain(owner, core_span_from_oxc(s.object.span()))
                } else {
                    owner
                };
                let prop = s.property.name.as_str();
                // `c?.x` looks the member up on the *unwrapped* owner type and
                // yields a nullable result, so the field type must be recovered
                // from `C` even though the owner is `C | null`.
                let owner_ty = owner.ty();
                let (lookup_ty, nullish) = match senv.types.resolve(owner_ty) {
                    Some(ts_aot_core::Type::Optional { inner, nullish }) => {
                        (*inner, Some(*nullish))
                    }
                    _ => (owner_ty, None),
                };
                let has_length = matches!(
                    senv.types.resolve(lookup_ty),
                    Some(ts_aot_core::Type::String | ts_aot_core::Type::Array { .. })
                );
                let field_ty = if matches!(
                    senv.types.resolve(lookup_ty),
                    Some(ts_aot_core::Type::Struct { .. })
                ) {
                    senv.program
                        .declarations
                        .iter()
                        .find_map(|d| match d {
                            HirDecl::Class(c) if c.ty == lookup_ty => c
                                .fields
                                .iter()
                                .find(|f| f.name.as_str() == prop)
                                .map(|f| f.ty),
                            _ => None,
                        })
                        .or_else(|| {
                            senv.program
                                .class_fields
                                .get(&lookup_ty)
                                .and_then(|fields| {
                                    fields
                                        .iter()
                                        .find(|f| f.name.as_str() == prop)
                                        .map(|f| f.ty)
                                })
                        })
                } else {
                    None
                };
                let bare_ty = if prop == "length" && has_length {
                    senv.types.intern(&ts_aot_core::Type::F64)
                } else {
                    field_ty.unwrap_or_else(|| senv.error_ty())
                };
                let ty = match nullish {
                    Some(_) => {
                        // `a?.b` yields `undefined` when the chain is short and the
                        // field's own value otherwise, so a nullable field must be
                        // flattened into one optional whose kinds are the field's
                        // plus `undefined` - never a nested `Option<Option<_>>`.
                        let bare = senv.types.resolve(bare_ty).cloned();
                        let (inner, kinds) = match bare {
                            Some(ts_aot_core::Type::Optional { inner, nullish }) => (
                                inner,
                                NullishKinds {
                                    null: nullish.null,
                                    undefined: true,
                                },
                            ),
                            _ => (bare_ty, NullishKinds::UNDEFINED_ONLY),
                        };
                        senv.types.intern(&ts_aot_core::Type::Optional {
                            inner,
                            nullish: kinds,
                        })
                    }
                    None => bare_ty,
                };
                HirExpr::Field {
                    owner: Box::new(owner),
                    field: FieldId::from_raw(0),
                    field_name: Atom::from(s.property.name.as_str()),
                    ty,
                    span: core_span_from_oxc(s.span),
                }
            }
            ME::ComputedMemberExpression(computed) => {
                let owner = self.walk_expr(senv, &computed.object, scope);
                let owner = if computed.optional {
                    Self::wrap_optional_chain(owner, core_span_from_oxc(computed.object.span()))
                } else {
                    owner
                };
                let index = self.walk_expr(senv, &computed.expression, scope);
                let ty = index_element_ty(senv, owner.ty());
                HirExpr::Index {
                    owner: Box::new(owner),
                    index: Box::new(index),
                    ty,
                    span: core_span_from_oxc(computed.span),
                }
            }
            ME::PrivateFieldExpression(p) => {
                senv.report_unwalked("private field access is not supported", p.span);
                HirExpr::Unit(core_span_from_oxc(p.span))
            }
        }
    }

    fn walk_chain(
        &mut self,
        senv: &mut SkeletonEnv,
        chain: &ChainExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        match &chain.expression {
            ChainElement::CallExpression(call) => self.walk_call(senv, call, scope),
            ChainElement::TSNonNullExpression(non_null) => {
                self.walk_expr(senv, &non_null.expression, scope)
            }
            other @ match_member_expression!(ChainElement) => {
                self.walk_member(senv, other.to_member_expression(), scope)
            }
        }
    }

    fn walk_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        a: &AssignmentExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let Some(simple_target) = a.left.as_simple_assignment_target() else {
            senv.report_unwalked(
                "destructuring assignment target is not supported; the assignment is skipped \
                 and the right-hand side is evaluated for its effects",
                a.span,
            );
            return self.walk_expr(senv, &a.right, scope);
        };
        let target = self.walk_simple_target(senv, simple_target, scope);
        let rhs = self.walk_expr(senv, &a.right, scope);
        if !matches!(
            &target,
            HirExpr::Local { .. }
                | HirExpr::Global { .. }
                | HirExpr::Field { .. }
                | HirExpr::Index { .. }
        ) {
            senv.report_unwalked(
                "assignment target is not supported; the assignment is skipped and the \
                 right-hand side is evaluated for its effects",
                a.span,
            );
            return HirExpr::Sequence {
                exprs: vec![target, rhs],
                ty: senv.error_ty(),
                span: core_span_from_oxc(a.span),
            };
        }
        let ty = literal_hir_ty(senv.types, &rhs).unwrap_or_else(|| {
            let t = rhs.ty();
            if t == ts_aot_core::TypeId::from_raw(0) {
                senv.error_ty()
            } else {
                t
            }
        });
        match compound_op(a.operator) {
            CompoundOp::Assign => HirExpr::Assignment {
                // The write must reach the variable's storage even when the
                // assignment sits inside a larger expression.
                target: Box::new(self.redirect_narrowed_target(target, scope)),
                value: Box::new(rhs),
                ty,
                span: core_span_from_oxc(a.span),
            },
            CompoundOp::Binary(op) => {
                // `&&=`/`||=`/`??=` may keep a nullish `x`, so the write must
                // reach the variable's storage rather than the narrowed copy -
                // in every expression context, exactly like a plain assignment.
                // Only an arithmetic update needs the narrowed *value* and is
                // copied back by `finish_statement_expression`.
                let target = if matches!(
                    op,
                    HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
                ) {
                    self.redirect_narrowed_target(target, scope)
                } else {
                    self.record_narrowed_update(&target, a.span, senv);
                    target
                };
                HirExpr::CompoundUpdate {
                    target: Box::new(target),
                    op,
                    rhs: Box::new(rhs),
                    post: false,
                    ty,
                    span: core_span_from_oxc(a.span),
                }
            }
            CompoundOp::Unsupported => {
                senv.report_unwalked("assignment operator is not supported", a.span);
                HirExpr::Unit(core_span_from_oxc(a.span))
            }
        }
    }

    /// A compound update (`x += 1`, `x++`) of a *narrowed* variable computes on
    /// the narrowed copy, so its new value still has to be copied back into the
    /// variable's storage. The copy back is only possible when the update is the
    /// whole expression of a statement slot, where the statement walker emits it
    /// right after the statement; the update is recorded here and any other one
    /// in the same expression is reported (see
    /// [`crate::stmt::SkeletonBuilder::finish_statement_expression`]) rather than
    /// silently keeping the stale storage value.
    ///
    /// The report is *blocking*: the storage would keep the old value while the
    /// narrowed copy moves on, so the emitted program would still compile and
    /// answer differently from JavaScript.
    fn record_narrowed_update(
        &mut self,
        target: &HirExpr,
        span: oxc_span::Span,
        senv: &mut SkeletonEnv,
    ) {
        let HirExpr::Local { id, .. } = target else {
            return;
        };
        if self.narrowed_storage(*id).is_none() {
            return;
        }
        if self.own_statement_expr {
            self.pending_narrowed_updates
                .push(crate::skeleton::NarrowedUpdate {
                    narrowed_id: *id,
                    span,
                });
            return;
        }
        senv.report_narrowed_update(span);
    }

    fn walk_simple_target(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &oxc_ast::ast::SimpleAssignmentTarget<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        use oxc_ast::ast::SimpleAssignmentTarget as SAT;
        match s {
            SAT::AssignmentTargetIdentifier(id) => {
                senv.ident_to_expr(id.name.as_str(), scope, core_span_from_oxc(id.span))
            }
            m @ match_member_expression!(SimpleAssignmentTarget) => {
                self.walk_member(senv, m.to_member_expression(), scope)
            }
            _ => match s.get_expression() {
                Some(inner) => self.walk_expr(senv, inner, scope),
                None => HirExpr::Unit(Span::default()),
            },
        }
    }
}

/// The local `this` is bound to in a class's bodies: the synthetic first
/// parameter every instance method gets (`decl.rs`). The scan needs it to tell a
/// read of the constructed instance's own field from a read of another object's.
fn class_this_id(class: &ts_aot_ir_hir::HirClass) -> LocalId {
    class
        .methods
        .iter()
        .find_map(|method| {
            method
                .params
                .iter()
                .find(|param| param.name.as_str() == "this")
                .map(|param| param.id)
        })
        // A class with no instance method has no constructor body to scan; the
        // fallback keeps the scan conservative for a field read it cannot place.
        .unwrap_or_else(|| LocalId::from_raw(u32::MAX))
}

impl SkeletonBuilder {
    /// Whether a `new` expression may run code that calls a closure, and so may
    /// write a variable captured by one.
    ///
    /// The constructor and the class field initializers are inspected when the
    /// callee names a class of this program: a class whose constructor *and*
    /// field initializers contain no call-like expression, which neither extends
    /// another class nor declares an accessor (the constructor could invoke a
    /// getter/setter), cannot reach a callback, so a narrowing of a captured
    /// variable survives its `new`. Everything else - a builtin (`new Date()`,
    /// `new Map()`), a dynamic callee, a subclass, or a class whose declaration
    /// is not visible at this point - keeps the conservative withdrawal.
    fn new_may_call_a_callback(
        &self,
        senv: &SkeletonEnv<'_>,
        new_expr: &oxc_ast::ast::NewExpression<'_>,
        scope: &BodyScope,
    ) -> bool {
        let Expression::Identifier(id) = &new_expr.callee else {
            return true;
        };
        let name = id.name.as_str();
        // A local binding may hold any function (or a class the frontend cannot
        // resolve here), so only a name that is not in scope is inspected.
        if scope.lookup(name).is_some() {
            return true;
        }
        let Some(&class_ty) = self.class_types.get(name) else {
            return true;
        };
        let Some(class) = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.ty == class_ty => Some(class),
                _ => None,
            })
        else {
            return true;
        };
        // The base constructor runs first, and an accessor the constructor
        // touches can run code the scan below cannot see.
        if class.extends.is_some()
            || class.methods.iter().any(|method| {
                method.name.as_str().starts_with("__get_")
                    || method.name.as_str().starts_with("__set_")
            })
        {
            return true;
        }
        let mut search = CallSearch {
            found: false,
            this_id: class_this_id(class),
        };
        for method in &class.methods {
            if method.name.as_str() == "constructor" {
                search.visit_block(&method.body);
            }
        }
        if let Some(inits) = senv.program.class_field_inits.get(&class_ty) {
            for (_, init) in inits {
                search.visit_expr(init);
            }
        }
        search.found
    }

    /// A variable captured by a closure cannot stay narrowed: the closure may
    /// outlive the narrowing and be called after the variable was set back to a
    /// nullish value. Its body is built against the variable's own storage (see
    /// [`capture_type_map`]), and every later statement in the enclosing scope
    /// sees the storage again as well.
    /// Records the variables the closure captures and withdraws the narrowings
    /// they are not part of: the closure body was built against the variable's
    /// own storage (see [`capture_type_map`]), and a later call of the closure
    /// may write that storage.
    ///
    /// Only the captures the closure **writes** are recorded ([`written_targets`]):
    /// a closure that merely reads a variable cannot make its narrowing stale,
    /// and treating such a capture as a write rejected sound programs with
    /// `P0018`.
    fn withdraw_captured_narrowings(
        &mut self,
        scope: &mut BodyScope,
        captures: &[HirExpr],
        written: &WrittenTargets,
    ) {
        // A capture is a local of the enclosing body: for a narrowed variable
        // that is the variable's *storage* local (see [`collect_capture_refs`]),
        // which is exactly what identifies the variable for the whole body.
        //
        // A write performed inside a *nested* closure is still a name reference
        // at this level, so the captured name is used as a fallback: map each
        // captured local back to the name it belongs to, resolving a narrowed
        // name through its storage.
        let mut name_of_id: std::collections::HashMap<LocalId, String> =
            std::collections::HashMap::new();
        for name in scope.names() {
            if let Some((binding, _)) = scope.lookup(&name) {
                name_of_id.entry(binding).or_insert_with(|| name.clone());
                if let Some((storage, _)) = self.narrowed_storage(binding) {
                    name_of_id.entry(storage).or_insert(name);
                }
            }
        }
        for capture in captures {
            let HirExpr::Local { id, .. } = capture else {
                continue;
            };
            let is_written = written.ids.contains(id)
                || name_of_id
                    .get(id)
                    .is_some_and(|name| written.names.contains(name));
            if !is_written {
                continue;
            }
            if let Some(frame) = self.captured_stack.last_mut() {
                frame.insert(*id);
            }
        }
    }

    /// A call may run a closure that *writes* a captured variable, so every
    /// narrowing of such a variable is withdrawn once the call returns. The
    /// storage mapping of the narrowing itself is kept: the left-hand side of an
    /// assignment whose right-hand side made this call was already walked
    /// against the narrowed local and still has to reach the storage.
    ///
    /// The narrowings are identified by their own locals, so a nested block that
    /// narrows a *shadowing* variable of the same name cannot hide the outer
    /// variable's narrowing from the withdrawal.
    pub(super) fn invalidate_captured_narrowings(&mut self, scope: &mut BodyScope) {
        let captured = self.captured_stack.last().cloned().unwrap_or_default();
        if captured.is_empty() {
            return;
        }
        let narrowed: Vec<LocalId> = match self.narrowed_stack.last() {
            Some(frame) => frame
                .iter()
                .filter(|(_, (storage, _))| captured.contains(storage))
                .map(|(narrowed, _)| *narrowed)
                .collect(),
            None => return,
        };
        for id in narrowed {
            scope.widen_id(id);
        }
    }
}

/// The type each visible name has *inside* a closure body.
///
/// A closure can outlive the statement that narrowed a variable, so a narrowed
/// name is seen there with the type of its own storage: the narrowed binding is
/// a snapshot, and the closure may be called after the variable was set back to
/// a nullish value. The narrowing is recognised through the *local the name is
/// currently bound to*, so a nested block that shadows the name with its own
/// declaration keeps its own type.
fn capture_type_map(
    scope: &BodyScope,
    narrowed: &std::collections::HashMap<LocalId, (LocalId, TypeId)>,
) -> std::collections::HashMap<String, TypeId> {
    let mut map = std::collections::HashMap::new();
    for name in scope.names() {
        if let Some((id, _)) = scope.lookup(&name)
            && let Some((_, storage_ty)) = narrowed.get(&id)
        {
            map.insert(name, *storage_ty);
            continue;
        }
        if let Some(ty) = scope.type_of_visible(&name) {
            map.insert(name, ty);
        }
    }
    map
}

/// The variables a closure body assigns to (`x = ...`, `x += ...`, `x ??= ...`),
/// including the writes of the closures nested inside it.
///
/// Only these captures can make a flow-sensitive narrowing of the enclosing
/// variable stale: a call of the closure may write the variable, while a closure
/// that only reads it can merely observe whatever the narrowing already
/// guarantees.
///
/// A captured variable is written through the variable's own local once the
/// closure body has been rewritten, but a write performed inside a *nested*
/// closure is still a name reference at this level (the rewrite does not descend
/// into it), so both forms are collected.
struct WrittenTargets {
    ids: std::collections::HashSet<LocalId>,
    names: std::collections::HashSet<String>,
}

/// Finds a call-like expression: a call, a nested `new`, `await`, `yield` or a
/// tagged template can run user code, so a constructor that contains one may
/// reach a closure. A closure *literal* is not called by being created, so its
/// body is skipped.
///
/// Reading a field is call-like too unless the owner is the *constructed
/// instance*: the scan does not know the owner's class, so `o.value` may run a
/// `get` accessor of `o`'s class, while `this.value` is a field of the class
/// being constructed (whose own accessors the caller has already rejected, and
/// whose field initializers hold literals only).
struct CallSearch {
    found: bool,
    /// The local `this` is bound to in the body being scanned. `decl.rs` binds it
    /// as the synthetic first parameter of every instance method, so the field
    /// read the scan has to let through is exactly `Local { id: this_id }`.
    this_id: LocalId,
}

/// Whether a member access reads the constructed instance itself. Only the
/// outermost owner is compared: `this.a.value` has another class in the middle
/// (`a`), so it stays conservative.
fn reads_the_constructed_instance(owner: &HirExpr, this_id: LocalId) -> bool {
    matches!(owner, HirExpr::Local { id, .. } if *id == this_id)
}

impl Visitor for CallSearch {
    fn visit_expr(&mut self, expr: &HirExpr) {
        if self.found {
            return;
        }
        match expr {
            HirExpr::Call { .. }
            | HirExpr::New { .. }
            | HirExpr::Await { .. }
            | HirExpr::Yield { .. }
            | HirExpr::Template { tag: Some(_), .. } => self.found = true,
            HirExpr::Closure { .. } => {}
            // A read of another object's field can run that class's accessor,
            // which this scan cannot see (`new_may_call_a_callback` only checks
            // the class it constructs).
            HirExpr::Field { owner, .. }
                if !reads_the_constructed_instance(owner, self.this_id) =>
            {
                self.found = true;
            }
            _ => walk_expr(self, expr),
        }
    }
}

fn written_targets(stmts: &[HirStmt]) -> WrittenTargets {
    struct WriteSearch {
        ids: std::collections::HashSet<LocalId>,
        names: std::collections::HashSet<String>,
    }
    impl Visitor for WriteSearch {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. } =
                expr
            {
                match target.as_ref() {
                    HirExpr::Local { id, .. } => {
                        self.ids.insert(*id);
                    }
                    HirExpr::Global { name, .. } => {
                        self.names.insert(name.as_str().to_owned());
                    }
                    _ => {}
                }
            }
            walk_expr(self, expr);
        }
    }
    let mut search = WriteSearch {
        ids: std::collections::HashSet::new(),
        names: std::collections::HashSet::new(),
    };
    for stmt in stmts {
        search.visit_stmt(stmt);
    }
    WrittenTargets {
        ids: search.ids,
        names: search.names,
    }
}

fn collect_capture_refs(
    stmts: &mut [HirStmt],
    scope: &BodyScope,
    narrowed: &std::collections::HashMap<LocalId, (LocalId, TypeId)>,
) -> Vec<HirExpr> {
    use ts_aot_ir_hir::{Visitor, VisitorMut, walk_expr, walk_expr_mut};
    struct CaptureSearch<'a> {
        scope: &'a BodyScope,
        seen: std::collections::HashSet<Atom>,
        ordered: Vec<(Atom, Span)>,
    }
    impl Visitor for CaptureSearch<'_> {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Global { name, span, .. } = expr
                && self.scope.type_of_visible(name.as_str()).is_some()
                && self.seen.insert(name.clone())
            {
                self.ordered.push((name.clone(), *span));
            }
            walk_expr(self, expr);
        }
    }
    struct CaptureRewrite {
        bindings: std::collections::HashMap<String, (LocalId, TypeId)>,
    }
    impl VisitorMut for CaptureRewrite {
        fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
            if let HirExpr::Global { name, span, .. } = expr
                && let Some((id, ty)) = self.bindings.get(name.as_str()).copied()
            {
                *expr = HirExpr::Local {
                    id,
                    ty,
                    span: *span,
                };
            }
            if matches!(expr, HirExpr::Closure { .. }) {
                return;
            }
            walk_expr_mut(self, expr);
        }
    }

    let mut search = CaptureSearch {
        scope,
        seen: std::collections::HashSet::new(),
        ordered: Vec::new(),
    };
    for stmt in stmts.iter() {
        search.visit_stmt(stmt);
    }

    let mut bindings: std::collections::HashMap<String, (LocalId, TypeId)> =
        std::collections::HashMap::new();
    for (name, _) in &search.ordered {
        // A narrowed variable is captured through its storage, so the closure
        // keeps observing the variable itself rather than the narrowed snapshot.
        if let Some((id, ty)) = scope.lookup(name.as_str()) {
            let binding = narrowed.get(&id).copied().unwrap_or((id, ty));
            bindings.insert(name.as_str().to_owned(), binding);
        }
    }

    let mut rewriter = CaptureRewrite { bindings };
    rewriter.visit_block_mut(stmts);

    search
        .ordered
        .iter()
        .filter_map(|(name, span)| {
            let (id, ty) = scope.lookup(name.as_str())?;
            let (id, ty) = narrowed.get(&id).copied().unwrap_or((id, ty));
            Some(HirExpr::Local {
                id,
                ty,
                span: *span,
            })
        })
        .collect()
}
