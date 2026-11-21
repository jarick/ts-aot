use std::collections::HashMap;

use oxc_ast::ast::{
    Argument, AssignmentExpression, BinaryExpression, CallExpression, ChainElement,
    ChainExpression, Expression, LogicalExpression, SequenceExpression, SimpleAssignmentTarget,
    TaggedTemplateExpression, TemplateLiteral, UnaryExpression, UpdateExpression, match_expression,
    match_member_expression,
};
use oxc_ecmascript::{ToBigInt, WithoutGlobalReferenceInformation};
use oxc_span::GetSpan;
use oxc_syntax::operator::{AssignmentOperator, UpdateOperator};
use ts_aot_core::{
    Atom, Diagnostic, FieldId, LocalId, NullishKinds, Span, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirClass, HirDecl, HirExpr, HirFunction, HirStmt, HirUnaryOp, Visitor,
    walk_expr,
};

use crate::body::FunctionBodyInput;
use crate::ops::{
    CompoundOp, compound_op, map_binary_op, map_logical_op, map_unary_op, number_to_hir,
};
use crate::scope::BodyScope;
use crate::skeleton::{
    ClassValueFieldBase, ClassValueFieldState, ClassValuePoisonConstraint, ClassValuePoisonFacts,
    ClassValuePoisonWrite, PendingGlobalRead, PendingGlobalReceiverCall, ReceiverRef,
    ReceiverWriteScan, SkeletonBuilder, SkeletonEnv, StaticMethodCapture, state_holds_class_value,
};
use crate::util::core_span_from_oxc;

const CLASS_VALUE_FIELD_LOGICAL_ASSIGN_CODE: &str = "E0419";
const CLASS_VALUE_FIELD_LOGICAL_ASSIGN: &str = "logical assignment `&&=` to a class-value field is not supported here: the field is \
     not provably a class value at this point, so its truthiness test cannot be lowered soundly; \
     assign the class value with `=`, or make the field provably a class value first";

fn same_number_expr(lhs: &HirExpr, rhs: &HirExpr) -> bool {
    match (lhs, rhs) {
        (HirExpr::Int(lhs, _), HirExpr::Int(rhs, _)) => lhs == rhs,
        (HirExpr::Float(lhs, _), HirExpr::Float(rhs, _)) => lhs == rhs,
        (HirExpr::Local { id: lhs, .. }, HirExpr::Local { id: rhs, .. }) => lhs == rhs,
        (HirExpr::Global { name: lhs, .. }, HirExpr::Global { name: rhs, .. }) => lhs == rhs,
        (
            HirExpr::Unary {
                op: HirUnaryOp::Neg,
                expr: lhs,
                ..
            },
            HirExpr::Unary {
                op: HirUnaryOp::Neg,
                expr: rhs,
                ..
            },
        ) => same_number_expr(lhs, rhs),
        _ => false,
    }
}

fn constant_numeric_value(expr: &HirExpr) -> Option<f64> {
    match expr {
        #[allow(clippy::cast_precision_loss)]
        HirExpr::Int(value, _) => Some(*value as f64),
        HirExpr::Float(bits, _) => Some(f64::from_bits(*bits)),
        HirExpr::Unary {
            op: HirUnaryOp::Neg,
            expr,
            ..
        } => Some(-constant_numeric_value(expr)?),
        HirExpr::Binary { op, lhs, rhs, .. } => {
            if *op == HirBinaryOp::Sub && same_number_expr(lhs, rhs) {
                return Some(0.0);
            }
            let lhs = constant_numeric_value(lhs)?;
            let rhs = constant_numeric_value(rhs)?;
            match op {
                HirBinaryOp::Add => Some(lhs + rhs),
                HirBinaryOp::Sub => Some(lhs - rhs),
                HirBinaryOp::Mul => Some(lhs * rhs),
                HirBinaryOp::Div => Some(lhs / rhs),
                HirBinaryOp::Mod => Some(lhs % rhs),
                _ => None,
            }
        }
        _ => None,
    }
}

fn class_value_definite_branch_writes(
    writes: &[ClassValuePoisonWrite],
    branch_depth: u32,
) -> HashMap<(TypeId, Atom, ClassValueFieldBase), (bool, ClassValuePoisonFacts)> {
    let mut last: HashMap<(TypeId, Atom, ClassValueFieldBase), &ClassValuePoisonWrite> =
        HashMap::new();
    for write in writes {
        last.insert(
            (write.owner_ty, write.field.clone(), write.base.clone()),
            write,
        );
    }
    last.into_iter()
        .filter(|(_, write)| {
            (write.plain_assign || write.holds_class_value) && write.depth == branch_depth
        })
        .map(|(key, write)| (key, (write.holds_class_value, write.facts)))
        .collect()
}

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

fn logical_operand_ty(senv: &mut SkeletonEnv<'_>, ops: [&HirExpr; 2]) -> TypeId {
    fn literal_fits(expr: &HirExpr, target: &Type) -> bool {
        match expr {
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
        Some(Type::Optional { inner, .. }) => {
            let element = match senv.types.resolve(inner).cloned() {
                Some(Type::Array { element }) => element,
                Some(Type::String) => senv.types.intern(&Type::String),
                _ => return senv.error_ty(),
            };
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
    ("throws", "assertThrows"),
];

impl SkeletonBuilder {
    fn identifier_expr(
        &mut self,
        senv: &mut SkeletonEnv,
        name: &str,
        scope: &mut BodyScope,
        span: Span,
    ) -> HirExpr {
        if scope.lookup(name).is_none()
            && !SkeletonBuilder::captured_name_is_visible(senv, name)
            && let Some(alias) = self.visible_generator_alias(name)
        {
            let ty = if self.in_callee_position {
                alias.call_ty
            } else {
                alias.fn_ty
            };
            return HirExpr::Global {
                name: alias.lifted,
                ty,
                span,
            };
        }
        let expr = senv.ident_to_expr(name, scope, span);
        if let HirExpr::Local { id, .. } = &expr {
            self.note_switch_case_local_read(*id);
        }
        expr
    }

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
                self.identifier_expr(senv, id.name.as_str(), scope, core_span_from_oxc(id.span))
            }
            Expression::ThisExpression(this_expr) => {
                if let Some((id, ty)) = scope.lookup("this") {
                    HirExpr::Local {
                        id,
                        ty,
                        span: core_span_from_oxc(this_expr.span),
                    }
                } else if let Some(ty) = senv.capture_type("this") {
                    HirExpr::Global {
                        name: Atom::new_inline("this"),
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
            Expression::PrivateInExpression(p) => self.walk_private_in(senv, p, scope),
            Expression::LogicalExpression(l) => self.walk_logical(senv, l, scope),
            Expression::UnaryExpression(unary) => self.walk_unary(senv, unary, scope),
            Expression::UpdateExpression(update) => self.walk_update(senv, update, scope),
            Expression::CallExpression(call) => {
                let called = self.walk_call(senv, call, scope);
                self.invalidate_captured_narrowings(scope);
                called
            }
            Expression::NewExpression(new_expr) => {
                let created = self.walk_new_expression(senv, new_expr, scope);
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
                if matches!(&chain.expression, ChainElement::CallExpression(_)) {
                    self.invalidate_captured_narrowings(scope);
                }
                chained
            }
            Expression::AssignmentExpression(a) => self.walk_assignment(senv, a, scope),
            Expression::AwaitExpression(a) => {
                let inner = self.walk_expr(senv, &a.argument, scope);
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
                self.invalidate_captured_narrowings(scope);
                tagged
            }
            Expression::ArrayExpression(arr) => self.walk_array_expression(senv, arr, scope),
            Expression::ObjectExpression(obj) => self.walk_object_expression(senv, obj, scope),
            Expression::ConditionalExpression(cond) => {
                let saved_unknown = self.unknown_class_value_locals.clone();
                let expr = self.walk_conditional_expression(senv, cond, scope);
                self.merge_unknown_class_value_locals(saved_unknown);
                expr
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
                self.walk_function_expression(senv, func, scope, false)
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
        let ret_ty = match arrow.return_type.as_deref() {
            Some(ann) => match merged.as_ref() {
                Some(map) => self.resolve_ts_type_from_annotation_with_params_in_scope(
                    senv,
                    Some(ann),
                    Some(map),
                    Some(scope),
                ),
                None => self.resolve_ts_type_from_annotation_in_scope(senv, Some(ann), Some(scope)),
            },
            None => senv.error_ty(),
        };
        let ret_hint = crate::param_infer::return_hint_ty(
            senv,
            arrow.return_type.is_some().then_some(ret_ty),
            arrow.r#async,
            false,
        );
        let hints = Self::infer_param_hints(
            senv,
            Some(&arrow.body),
            &arrow.params.items,
            ret_hint,
            None,
            arrow.expression,
            Some(scope),
        );
        let (params, extra_locals) = self.build_params_and_locals(
            senv,
            &arrow.params.items,
            param_base,
            merged.as_ref(),
            Some(&hints),
            Some(scope),
        );
        let narrowed = self.narrowed_stack.last().cloned().unwrap_or_default();
        let outer_statement_expr = self.own_statement_expr;
        self.own_statement_expr = false;
        senv.capture_types.push(capture_type_map(scope, &narrowed));
        let captured_class_values = self.capture_class_value_map(scope, &narrowed);
        self.capture_class_values.push(captured_class_values);
        self.push_pending_closure_captures(scope, &narrowed);
        let mut body: Vec<HirStmt> = if arrow.expression {
            let inner_base = params
                .iter()
                .map(|p| p.id.raw())
                .max()
                .map_or(param_base, |m| m.saturating_add(1));
            let mut inner_scope = BodyScope::new(inner_base);
            inner_scope.inherit_classes(scope);
            let mut prelude = self.declare_params_with_prelude(
                senv,
                &mut inner_scope,
                &params,
                &extra_locals,
                &arrow.params.items,
                0,
            );
            self.body_depth = self.body_depth.saturating_add(1);
            let mut stmts = {
                let saved_module_init = std::mem::take(&mut self.module_init_depth);
                let stmts = self.walk_stmts(senv, &arrow.body.statements, &mut inner_scope);
                self.module_init_depth = saved_module_init;
                stmts
            };
            self.body_depth = self.body_depth.saturating_sub(1);
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
                    outer_classes: Some(scope),
                },
                false,
                true,
                false,
            );
            scope.set_next(used);
            b
        };
        senv.capture_types.pop();
        self.capture_class_values.pop();
        self.own_statement_expr = outer_statement_expr;
        self.strict = prev_strict;
        let mut captures = collect_capture_refs(&mut body, scope, &narrowed);
        self.finish_pending_closure_captures(&mut captures);
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

    pub(crate) fn walk_function_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &oxc_ast::ast::Function<'_>,
        scope: &mut BodyScope,
        self_binding: bool,
    ) -> HirExpr {
        let span = core_span_from_oxc(func.span);
        if func.generator && func.type_parameters.is_none() {
            return self.walk_generator_function_expression(senv, func, scope);
        }
        let unsupported = func.generator || func.type_parameters.is_some();
        if unsupported {
            senv.report_unwalked(
                "function expression form is not supported by the body walker",
                func.span,
            );
            return HirExpr::Unit(span);
        }
        if func.r#async {
            senv.diagnostics.push(Diagnostic::warning(
                "E0502",
                "async function expressions are lowered as synchronous closures in this AOT \
                 target; `await` results are not resolved before yielding",
                core_span_from_oxc(func.span),
            ));
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
        let ret_ty = match func.return_type.as_deref() {
            Some(ann) => match merged.as_ref() {
                Some(map) => self.resolve_ts_type_from_annotation_with_params_in_scope(
                    senv,
                    Some(ann),
                    Some(map),
                    Some(scope),
                ),
                None => self.resolve_ts_type_from_annotation_in_scope(senv, Some(ann), Some(scope)),
            },
            None => senv.error_ty(),
        };
        let ret_hint = crate::param_infer::return_hint_ty(
            senv,
            func.return_type.is_some().then_some(ret_ty),
            func.r#async,
            func.generator,
        );
        let hints = Self::infer_param_hints(
            senv,
            func.body.as_deref(),
            &func.params.items,
            ret_hint,
            None,
            false,
            Some(scope),
        );
        let (params, extra_locals) = self.build_params_and_locals(
            senv,
            &func.params.items,
            param_base,
            merged.as_ref(),
            Some(&hints),
            Some(scope),
        );
        if self_binding && let Some(id) = func.id.as_ref() {
            let self_ty = senv.types.intern(&Type::Fn {
                params: params.iter().map(|p| p.ty).collect(),
                optional: params.iter().map(|p| p.optional).collect(),
                ret: ret_ty,
                err: None,
            });
            scope.predeclare_typed(id.name.as_str(), self_ty);
        }
        let narrowed = self.narrowed_stack.last().cloned().unwrap_or_default();
        let outer_statement_expr = self.own_statement_expr;
        self.own_statement_expr = false;
        senv.capture_types.push(capture_type_map(scope, &narrowed));
        let shadowed_this = Self::shadow_this_captures(senv);
        let captured_class_values = self.capture_class_value_map(scope, &narrowed);
        self.capture_class_values.push(captured_class_values);
        self.push_pending_closure_captures(scope, &narrowed);
        let (mut body, used) = self.walk_function_body(
            senv,
            &FunctionBodyInput {
                body: func.body.as_deref(),
                params: &params,
                extra_locals: &extra_locals,
                items: &func.params.items,
                item_offset: 0,
                param_base,
                outer_classes: Some(scope),
            },
            false,
            false,
            true,
        );
        scope.set_next(used);
        senv.capture_types.pop();
        Self::restore_this_captures(senv, shadowed_this);
        self.capture_class_values.pop();
        self.own_statement_expr = outer_statement_expr;
        self.strict = prev_strict;
        let mut captures = collect_capture_refs(&mut body, scope, &narrowed);
        self.finish_pending_closure_captures(&mut captures);
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

    pub(crate) fn walk_generator_function_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &oxc_ast::ast::Function<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let span = core_span_from_oxc(func.span);
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
        let element_ty = if func.return_type.is_some() {
            let annotation = func.return_type.as_deref();
            let resolved =
                self.resolve_ts_type_from_annotation_in_scope(senv, annotation, Some(scope));
            generator_element_ty(self, senv, annotation, resolved, func.r#async, scope)
        } else {
            senv.error_ty()
        };
        let ret_hint = crate::param_infer::return_hint_ty(
            senv,
            func.return_type.is_some().then_some(element_ty),
            func.r#async,
            true,
        );
        let hints = Self::infer_param_hints(
            senv,
            func.body.as_deref(),
            &func.params.items,
            ret_hint,
            None,
            false,
            None,
        );
        let param_base = scope.peek_next();
        let (params, extra_locals) = self.build_params_and_locals(
            senv,
            &func.params.items,
            param_base,
            None,
            Some(&hints),
            Some(scope),
        );
        let narrowed = self.narrowed_stack.last().cloned().unwrap_or_default();
        let outer_statement_expr = self.own_statement_expr;
        self.own_statement_expr = false;
        senv.capture_types.push(capture_type_map(scope, &narrowed));
        let shadowed_this = Self::shadow_this_captures(senv);
        let captured_class_values = self.capture_class_value_map(scope, &narrowed);
        self.capture_class_values.push(captured_class_values);
        self.push_pending_closure_captures(scope, &narrowed);
        let (mut body, used) = self.walk_function_body(
            senv,
            &FunctionBodyInput {
                body: func.body.as_deref(),
                params: &params,
                extra_locals: &extra_locals,
                items: &func.params.items,
                item_offset: 0,
                param_base,
                outer_classes: Some(scope),
            },
            true,
            false,
            true,
        );
        scope.set_next(used);
        senv.capture_types.pop();
        Self::restore_this_captures(senv, shadowed_this);
        self.capture_class_values.pop();
        self.own_statement_expr = outer_statement_expr;
        self.strict = prev_strict;
        let mut captures = collect_capture_refs(&mut body, scope, &narrowed);
        self.finish_pending_closure_captures(&mut captures);
        let written = written_targets(&body);
        self.withdraw_captured_narrowings(scope, &captures, &written);
        if !captures.is_empty() {
            senv.report_unsupported(
                "E0500",
                "a generator function expression that reads a variable of the enclosing \
                 function is not supported in this AOT target; hoist the generator to module \
                 scope and pass the value as an argument instead",
                func.span,
            );
            return HirExpr::Unit(span);
        }
        let element_ty = if func.return_type.is_some() {
            element_ty
        } else {
            infer_generator_element_ty(&body, senv.types).unwrap_or(element_ty)
        };
        let name = self.fresh_generator_expr_name(senv);
        let param_types: Vec<TypeId> = params.iter().map(|p| p.ty).collect();
        let param_optional: Vec<bool> = params.iter().map(|p| p.optional).collect();
        let wrapper_params = params.clone();
        senv.program.push_decl(HirDecl::Function(HirFunction {
            name: name.clone(),
            params,
            ret: element_ty,
            throws: None,
            body,
            is_async: func.r#async,
            is_generator: true,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }));
        let generator_ty = senv.types.intern(&Type::Generator {
            inner: element_ty,
            is_async: func.r#async,
        });
        let fn_ty = senv.types.intern(&Type::Fn {
            params: param_types,
            optional: param_optional,
            ret: generator_ty,
            err: None,
        });
        if self.in_callee_position {
            return HirExpr::Global {
                name,
                ty: generator_ty,
                span,
            };
        }
        let args: Vec<HirExpr> = wrapper_params
            .iter()
            .map(|param| HirExpr::Local {
                id: param.id,
                ty: param.ty,
                span,
            })
            .collect();
        let call = HirExpr::Call {
            callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                name,
                ty: generator_ty,
                span,
            })),
            args,
            type_args: Vec::new(),
            ty: generator_ty,
            span,
        };
        let wrapper_name = self.fresh_generator_expr_name(senv);
        senv.program.push_decl(HirDecl::Function(HirFunction {
            name: wrapper_name.clone(),
            params: wrapper_params,
            ret: generator_ty,
            throws: None,
            body: vec![HirStmt::Return { value: Some(call) }],
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }));
        HirExpr::Global {
            name: wrapper_name,
            ty: fn_ty,
            span,
        }
    }

    fn fresh_generator_expr_name(&mut self, senv: &SkeletonEnv<'_>) -> Atom {
        loop {
            let id = self.next_closure_id;
            self.next_closure_id = self.next_closure_id.saturating_add(1);
            let name = Atom::from(format!(
                "__ts_aot_genexpr_m{}_{id}",
                senv.program.module.raw()
            ));
            if !declares_name(senv.program, &name) {
                return name;
            }
        }
    }

    fn shadow_this_captures(senv: &mut SkeletonEnv) -> Vec<(usize, TypeId)> {
        let mut shadowed = Vec::new();
        for (index, frame) in senv.capture_types.iter_mut().enumerate() {
            if let Some(ty) = frame.remove("this") {
                shadowed.push((index, ty));
            }
        }
        shadowed
    }

    fn restore_this_captures(senv: &mut SkeletonEnv, shadowed: Vec<(usize, TypeId)>) {
        for (index, ty) in shadowed {
            if let Some(frame) = senv.capture_types.get_mut(index) {
                frame.insert("this".to_owned(), ty);
            }
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
            self.invalidate_captured_narrowings(scope);
            return delegated;
        }
        let inner = y.argument.as_ref().map(|a| self.walk_expr(senv, a, scope));
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
                    if e.has_static_type() {
                        Some(e.ty())
                    } else {
                        None
                    }
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
            inferred
                .or_else(|| self.array_literal_hints.get(&arr.span.start).copied())
                .unwrap_or_else(|| senv.error_ty())
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
        let saved_fields = self.snapshot_class_value_field_state();
        self.enter_class_value_branch();
        let then_start = self.class_value_poison_writes.len();
        let then_branch = self.walk_expr(senv, &c.consequent, scope);
        let else_start = self.class_value_poison_writes.len();
        self.leave_class_value_branch();
        let then_fields = self.snapshot_class_value_field_state();
        self.restore_class_value_field_state(&saved_fields);
        self.enter_class_value_branch();
        let else_branch = self.walk_expr(senv, &c.alternate, scope);
        let else_end = self.class_value_poison_writes.len();
        self.leave_class_value_branch();
        let else_fields = self.snapshot_class_value_field_state();
        self.join_class_value_field_states(&[then_fields.clone(), else_fields.clone()]);
        let branch_depth = self.conditional_depth + 1;
        self.join_class_value_field_poison(
            &[(then_start, else_start), (else_start, else_end)],
            &[then_fields, else_fields],
            &saved_fields,
            branch_depth,
        );
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

    pub(crate) fn walk_class_expression(
        &mut self,
        senv: &mut SkeletonEnv,
        class_expr: &oxc_ast::ast::Class<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let unique_name = self.fresh_anon_class_name(senv);
        let mut hir_class = self.build_class(senv, class_expr, false, false, Some(scope));
        hir_class.name = unique_name.clone();
        self.register_static_methods(senv, &hir_class, &unique_name);
        let class_ty = hir_class.ty;
        self.walk_local_class_field_inits(senv, class_expr, class_ty, scope);
        let span = core_span_from_oxc(class_expr.span);
        senv.program
            .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
        HirExpr::DefaultConstruct { ty: class_ty, span }
    }

    fn walk_binary(
        &mut self,
        senv: &mut SkeletonEnv,
        b: &BinaryExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let lhs = self.walk_expr(senv, &b.left, scope);
        let lhs_class = self.is_class_value_expr(&lhs, scope.body_id());
        let rhs = self.walk_expr(senv, &b.right, scope);
        let ty = senv.error_ty();
        let span = core_span_from_oxc(b.span);
        if let Some(op) = map_binary_op(b.operator) {
            let rhs_class = self.is_class_value_expr(&rhs, scope.body_id());
            if lhs_class || rhs_class {
                if matches!(
                    op,
                    HirBinaryOp::Eq
                        | HirBinaryOp::Ne
                        | HirBinaryOp::StrictEq
                        | HirBinaryOp::StrictNe
                ) {
                    if !(lhs_class && rhs_class) {
                        let other = if lhs_class { &rhs } else { &lhs };
                        let Some(value) = Self::class_value_equality_constant(senv, op, other)
                        else {
                            senv.report_unsupported(
                                "E0419",
                                "comparing a class value with a string or a value of unknown type \
                                 is not supported; compare against a number, `null`, \
                                 `undefined`, or another class value",
                                span,
                            );
                            return HirExpr::Bool(false, span);
                        };
                        let bool_ty = senv.types.intern(&Type::Bool);
                        return HirExpr::Sequence {
                            exprs: vec![lhs, rhs, HirExpr::Bool(value, span)],
                            ty: bool_ty,
                            span,
                        };
                    }
                } else if matches!(
                    op,
                    HirBinaryOp::Add
                        | HirBinaryOp::Sub
                        | HirBinaryOp::Mul
                        | HirBinaryOp::Div
                        | HirBinaryOp::Mod
                        | HirBinaryOp::Lt
                        | HirBinaryOp::Le
                        | HirBinaryOp::Gt
                        | HirBinaryOp::Ge
                        | HirBinaryOp::BitAnd
                        | HirBinaryOp::BitOr
                        | HirBinaryOp::BitXor
                        | HirBinaryOp::Shl
                        | HirBinaryOp::Shr
                        | HirBinaryOp::Usr
                ) {
                    senv.report_unsupported(
                        "E0419",
                        "arithmetic and ordering comparisons on a class value are not supported; \
                         a class value is an object, not a number",
                        span,
                    );
                    return HirExpr::Bool(false, span);
                }
            }
            HirExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty,
                span,
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
        let saved_unknown = self.unknown_class_value_locals.clone();
        self.enter_class_value_branch();
        let rhs = self.walk_expr(senv, &l.right, scope);
        self.leave_class_value_branch();
        self.merge_unknown_class_value_locals(saved_unknown);
        let op = map_logical_op(l.operator);
        let ty = if op == HirBinaryOp::Coalesce {
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
        let span = core_span_from_oxc(unary.span);
        match map_unary_op(unary.operator) {
            Some(HirUnaryOp::Not) if self.is_class_value_expr(&inner, scope.body_id()) => {
                let ty = senv.types.intern(&Type::Bool);
                HirExpr::Sequence {
                    exprs: vec![inner, HirExpr::Bool(false, span)],
                    ty,
                    span,
                }
            }
            Some(HirUnaryOp::Not) if self.may_be_class_value_expr(&inner) => {
                senv.report_unsupported(
                    "E0419",
                    "`!` on a binding that may hold either a class value or a plain value is not \
                     supported; a class value is always truthy, while a plain value is not",
                    span,
                );
                HirExpr::Bool(false, span)
            }
            Some(op) => {
                let ty = match op {
                    HirUnaryOp::Not => senv.types.intern(&Type::Bool),
                    _ => senv.error_ty(),
                };
                HirExpr::Unary {
                    op,
                    expr: Box::new(inner),
                    ty,
                    span,
                }
            }
            None => inner,
        }
    }

    pub(super) fn is_class_value_expr(&self, expr: &HirExpr, body_id: u32) -> bool {
        match expr {
            HirExpr::Local { id, .. } => {
                let storage = self.class_value_storage(*id);
                self.class_value_locals.contains_key(&storage)
                    && !self.unknown_class_value_locals.contains_key(&storage)
            }
            HirExpr::Global { name, .. } => {
                let name = name.as_str();
                if let Some(id) = self.captured_local_id(name) {
                    return self.captured_class_value_local(name).is_some()
                        && !self.unknown_class_value_locals.contains_key(&id);
                }
                !self.unknown_class_value_globals.contains(name)
                    && (self.class_value_globals.contains_key(name)
                        || self.class_types.contains_key(name))
            }
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let owner_ty = owner.ty();
                let base = self.class_value_field_base(owner, body_id);
                let escaping = base
                    .as_ref()
                    .is_none_or(|base| !self.class_value_field_base_is_fresh(base));
                let unpoisoned = self
                    .class_value_field_poison_bases
                    .get(&owner_ty)
                    .and_then(|fields| fields.get(field_name))
                    .is_none_or(|bases| {
                        !bases.unknown_base
                            && if escaping {
                                bases.bases.is_empty()
                            } else {
                                base.as_ref()
                                    .is_none_or(|base| !bases.bases.contains_key(base))
                            }
                    });
                let has_candidates = escaping
                    || base.as_ref().is_some_and(|base| {
                        !self
                            .fresh_field_base_classes(base, owner_ty, field_name)
                            .is_empty()
                    });
                unpoisoned
                    && has_candidates
                    && self
                        .class_value_fields
                        .get(&owner_ty)
                        .is_some_and(|fields| fields.contains_key(field_name))
                    && !self
                        .unknown_class_value_fields
                        .get(&owner_ty)
                        .is_some_and(|unknown| unknown.contains(field_name))
            }
            _ => false,
        }
    }

    fn class_value_equality_constant(
        senv: &SkeletonEnv<'_>,
        op: HirBinaryOp,
        other: &HirExpr,
    ) -> Option<bool> {
        let strict = matches!(op, HirBinaryOp::StrictEq | HirBinaryOp::StrictNe);
        let definitely_primitive = matches!(
            other,
            HirExpr::Int(..)
                | HirExpr::Float(..)
                | HirExpr::Bool(..)
                | HirExpr::Null(..)
                | HirExpr::Undefined(..)
        ) || matches!(
            senv.types.resolve(other.ty()),
            Some(
                Type::F32
                    | Type::F64
                    | Type::I8
                    | Type::I16
                    | Type::I32
                    | Type::I64
                    | Type::U8
                    | Type::U16
                    | Type::U32
                    | Type::U64
                    | Type::Bool
                    | Type::Null
                    | Type::Void
            )
        );
        if !strict && !definitely_primitive {
            return None;
        }
        Some(matches!(op, HirBinaryOp::Ne | HirBinaryOp::StrictNe))
    }

    fn may_be_class_value_expr(&self, expr: &HirExpr) -> bool {
        match expr {
            HirExpr::Local { id, .. } => self
                .unknown_class_value_locals
                .contains_key(&self.class_value_storage(*id)),
            HirExpr::Global { name, .. } => {
                let name = name.as_str();
                if let Some(id) = self.captured_local_id(name) {
                    return self.unknown_class_value_locals.contains_key(&id);
                }
                self.unknown_class_value_globals.contains(name)
                    && (self.class_value_globals.contains_key(name)
                        || self.class_types.contains_key(name))
            }
            HirExpr::Field {
                owner, field_name, ..
            } => self
                .unknown_class_value_fields
                .get(&owner.ty())
                .is_some_and(|unknown| unknown.contains(field_name)),
            _ => false,
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
        if let HirExpr::Local { id, .. } = &target {
            let storage = self.class_value_storage(*id);
            self.constant_int_locals.remove(&storage);
            self.forget_class_value_local(storage, None);
        }
        if let HirExpr::Global { name, .. } = &target {
            if let Some(id) = self.captured_local_id(name.as_str()) {
                let storage = self.class_value_storage(id);
                self.constant_int_locals.remove(&storage);
                self.forget_class_value_local(storage, Some(name.as_str()));
            } else {
                self.class_value_globals.remove(name.as_str());
                self.unknown_class_value_globals
                    .insert(name.as_str().to_owned());
            }
        }
        if let HirExpr::Field {
            owner, field_name, ..
        } = &target
        {
            let owner_ty = owner.ty();
            let field_name = field_name.clone();
            let is_class_value_field = self
                .class_value_fields
                .get(&owner_ty)
                .is_some_and(|fields| fields.contains_key(&field_name));
            if is_class_value_field {
                let base = self.class_value_field_base(owner, scope.body_id());
                let conditional =
                    !self.class_value_branch_ids.is_empty() || self.try_body_skips == Some(true);
                self.unknown_class_value_fields
                    .entry(owner_ty)
                    .or_default()
                    .insert(field_name.clone());
                self.record_class_value_field_poison(
                    owner_ty,
                    &field_name,
                    base,
                    ClassValuePoisonFacts::default(),
                    false,
                );
                if !conditional
                    && let Some(classes) = self
                        .class_value_fields
                        .get_mut(&owner_ty)
                        .and_then(|fields| fields.get_mut(&field_name))
                {
                    classes.clear();
                }
            }
        }
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

    fn record_global_receiver_call(
        &mut self,
        senv: &SkeletonEnv<'_>,
        callee: &HirExpr,
        args: &[HirExpr],
        span: Span,
        scope: &BodyScope,
    ) {
        let HirExpr::Field {
            owner, field_name, ..
        } = callee
        else {
            return;
        };
        let receiver = match owner.as_ref() {
            HirExpr::Global { name, .. } => {
                if self.captured_local_id(name.as_str()).is_some() {
                    return;
                }
                ReceiverRef::Global(name.as_str().to_owned())
            }
            HirExpr::Field {
                owner: inner_owner,
                field_name: inner_field,
                ..
            } if matches!(inner_owner.as_ref(), HirExpr::Global { .. }) => {
                let HirExpr::Global { name, .. } = inner_owner.as_ref() else {
                    return;
                };
                if self.captured_local_id(name.as_str()).is_some() {
                    return;
                }
                ReceiverRef::StaticField {
                    class: name.as_str().to_owned(),
                    field: inner_field.as_str().to_owned(),
                }
            }
            other => match Self::place_receiver_ref(other, scope) {
                Some(receiver) => receiver,
                None => return,
            },
        };
        self.record_pending_receiver_call(senv, receiver, owner, field_name, args, span, scope);
    }

    fn place_receiver_ref(place: &HirExpr, scope: &BodyScope) -> Option<ReceiverRef> {
        let (root_id, fields) = Self::place_chain(place)?;
        let this_local = scope.lookup("this").map(|(id, _)| id);
        let root = if Some(root_id) == this_local {
            "this".to_owned()
        } else {
            scope.name_of_local(root_id)?
        };
        Some(ReceiverRef::Path {
            root,
            root_id,
            body_id: scope.body_id(),
            fields,
        })
    }

    fn place_chain(place: &HirExpr) -> Option<(LocalId, Vec<String>)> {
        match place {
            HirExpr::Local { id, .. } => Some((*id, Vec::new())),
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let (root, mut fields) = Self::place_chain(owner)?;
                fields.push(field_name.as_str().to_owned());
                Some((root, fields))
            }
            HirExpr::Index { owner, .. } => {
                let (root, mut fields) = Self::place_chain(owner)?;
                fields.push(crate::skeleton::INDEX_PATH_COMPONENT.to_owned());
                Some((root, fields))
            }
            _ => None,
        }
    }

    fn record_pending_receiver_call(
        &mut self,
        senv: &SkeletonEnv<'_>,
        receiver: ReceiverRef,
        owner: &HirExpr,
        field_name: &Atom,
        args: &[HirExpr],
        span: Span,
        scope: &BodyScope,
    ) {
        if !Self::method_writes_this(senv, owner.ty(), field_name.as_str()) {
            return;
        }
        let shadowed_locals: std::collections::HashMap<String, Option<TypeId>> = scope
            .names()
            .into_iter()
            .map(|name| {
                let ty = scope.type_of_visible(&name);
                (name, ty)
            })
            .collect();
        let this_local = scope.lookup("this").map(|(id, _)| id);
        let this_place = match (&receiver, this_local) {
            (ReceiverRef::Path { root_id, .. }, Some(this_id)) if *root_id == this_id => {
                Some(Vec::new())
            }
            _ => None,
        };
        let mut writer = ReceiverWriteScan::new(
            receiver.clone(),
            scope.visible_closure_bodies(),
            &self.class_types,
            shadowed_locals,
        );
        writer.enter_receiver_scope(this_local, this_place, true);
        for arg in args {
            writer.visit_expr(arg);
        }
        let arg_bindings = args.iter().map(|arg| writer.argument_body(arg)).collect();
        self.pending_global_receiver_calls
            .push(PendingGlobalReceiverCall {
                receiver,
                owner_ty: owner.ty(),
                method: field_name.as_str().to_owned(),
                direct_write: writer.direct_write,
                opaque: writer.opaque,
                call_sites: writer.call_sites,
                arg_bindings,
                span,
            });
    }

    fn walk_call(
        &mut self,
        senv: &mut SkeletonEnv,
        call: &CallExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        if matches!(call.callee, Expression::Super(_)) {
            let lowered = self.walk_super_call(senv, call, scope);
            self.note_may_throw();
            return lowered;
        }
        let previous_callee_position = self.in_callee_position;
        self.in_callee_position = true;
        let callee_expr = self.walk_expr(senv, &call.callee, scope);
        self.in_callee_position = previous_callee_position;
        let callee_expr = self.resolve_static_method_callee(
            senv,
            callee_expr,
            core_span_from_oxc(call.callee.span()),
            scope,
        );
        let static_method_name = match &callee_expr {
            HirExpr::Global { name, .. } if self.static_method_captures.contains_key(name) => {
                Some(name.clone())
            }
            _ => None,
        };
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
        if let Some(static_name) = static_method_name.as_ref()
            && let Some(captures) = self.static_method_captures.get(static_name).cloned()
        {
            let name = static_name.as_str();
            let span = core_span_from_oxc(call.callee.span());
            let source_arity = self
                .static_method_globals
                .values()
                .flat_map(|methods| methods.values())
                .find(|(global, _, _)| global == static_name)
                .and_then(|(_, _, fn_ty)| match senv.types.resolve(*fn_ty) {
                    Some(ts_aot_core::Type::Fn { params, .. }) => {
                        Some(params.len().saturating_sub(captures.len()))
                    }
                    _ => None,
                });
            if let Some(source_arity) = source_arity {
                while args.len() < source_arity {
                    args.push(HirExpr::Undefined(span));
                }
            }
            for StaticMethodCapture {
                source,
                local,
                body_id,
                ty: _,
            } in captures
            {
                let bound = if scope.body_id() == body_id {
                    scope.lookup(source.as_str())
                } else {
                    None
                };
                let in_scope = bound.is_some_and(|(bound, _)| {
                    bound == local
                        || self
                            .narrowed_storage(bound)
                            .is_some_and(|(storage_id, _)| storage_id == local)
                });
                if !in_scope {
                    senv.report_unsupported(
                        "E0419",
                        format!(
                            "the static method `{name}` reads `{source}`, a local variable of the \
                             function that declares the class, so it can only be called in that \
                             function while that variable is in scope; call it there or pass the \
                             value as a parameter",
                            source = source.as_str()
                        )
                        .as_str(),
                        call.callee.span(),
                    );
                    return HirExpr::Unit(core_span_from_oxc(call.span));
                }
                let Some((bound_id, bound_ty)) = bound else {
                    return HirExpr::Unit(core_span_from_oxc(call.span));
                };
                args.push(HirExpr::Local {
                    id: bound_id,
                    ty: bound_ty,
                    span,
                });
            }
        }
        self.record_global_receiver_call(
            senv,
            &callee_expr,
            &args,
            core_span_from_oxc(call.span),
            scope,
        );
        self.note_may_throw();
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
                    let owner_ty = match senv.types.resolve(owner.ty()) {
                        Some(ts_aot_core::Type::Optional { inner, .. }) => *inner,
                        _ => owner.ty(),
                    };
                    let static_receiver = matches!(
                        owner.as_ref(),
                        HirExpr::Global { name, .. }
                            if self.class_types.contains_key(name.as_str())
                    );
                    let declared = class_method_ret_in_program(
                        senv,
                        owner_ty,
                        field_name.as_str(),
                        static_receiver,
                    )
                    .or_else(|| {
                        self.class_method_rets
                            .get(&(owner_ty, field_name.clone(), static_receiver))
                            .copied()
                    })
                    .or_else(|| match owner.as_ref() {
                        HirExpr::Global { name, .. } => self
                            .class_types
                            .get(name.as_str())
                            .and_then(|class_ty| self.static_method_globals.get(class_ty))
                            .and_then(|methods| methods.get(field_name))
                            .map(|(_, ret, _)| *ret),
                        _ => None,
                    });
                    match declared {
                        Some(ty)
                            if !matches!(
                                senv.types.resolve(ty),
                                None | Some(ts_aot_core::Type::Error | ts_aot_core::Type::Void)
                            ) =>
                        {
                            ty
                        }
                        _ => senv.error_ty(),
                    }
                }
            }
            HirExpr::Local { ty, .. } => match senv.types.resolve(*ty) {
                Some(ts_aot_core::Type::Fn { ret, .. })
                    if !matches!(
                        senv.types.resolve(*ret),
                        None | Some(ts_aot_core::Type::Error | ts_aot_core::Type::Void)
                    ) =>
                {
                    *ret
                }
                _ => senv.error_ty(),
            },
            HirExpr::Global { ty, .. } => match senv.types.resolve(*ty) {
                Some(ts_aot_core::Type::Fn { ret, .. })
                    if !matches!(
                        senv.types.resolve(*ret),
                        None | Some(ts_aot_core::Type::Error | ts_aot_core::Type::Void)
                    ) =>
                {
                    *ret
                }
                _ if !matches!(
                    senv.types.resolve(*ty),
                    None | Some(ts_aot_core::Type::Error)
                ) =>
                {
                    *ty
                }
                _ => senv.error_ty(),
            },
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

    fn walk_super_member(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &oxc_ast::ast::StaticMemberExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        self.walk_super_property(
            senv,
            &Atom::from(s.property.name.as_str()),
            core_span_from_oxc(s.span),
            s.span,
            scope,
        )
    }

    fn walk_super_property(
        &mut self,
        senv: &mut SkeletonEnv,
        prop: &Atom,
        span: Span,
        raw_span: oxc_span::Span,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let Some(parent_name) = self.current_super_class.clone() else {
            senv.report_unsupported(
                "E0500",
                "`super` is only supported inside a method of a class that extends another class",
                raw_span,
            );
            return HirExpr::Unit(span);
        };
        if super_class_decl(senv, &parent_name).is_none() {
            senv.report_unsupported(
                "E0500",
                format!(
                    "the parent class `{}` must be declared before the class that calls \
                     `super.{prop}()`",
                    parent_name.as_str()
                )
                .as_str(),
                raw_span,
            );
            return HirExpr::Unit(span);
        }
        if !self.in_callee_position {
            senv.report_unsupported(
                "E0500",
                "`super.<member>` is only supported as a direct call `super.m(...)` in this AOT \
                 target; a `super` reference reads the parent's prototype (accessors and \
                 prototype properties), which is not modelled, and an instance field of the \
                 parent is not visible through `super` at all",
                raw_span,
            );
            return HirExpr::Unit(span);
        }
        let Some((owner_name, method)) = super_ancestor_declaring_method(senv, &parent_name, prop)
        else {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`super.{prop}` must name an instance method of the parent class \
                     `{}`",
                    parent_name.as_str()
                )
                .as_str(),
                raw_span,
            );
            return HirExpr::Unit(span);
        };
        let ret = method.ret;
        let Some((this_id, this_ty)) = scope.lookup("this") else {
            senv.report_unsupported(
                "E0500",
                "`super` inside a nested closure is not supported in this AOT target",
                raw_span,
            );
            return HirExpr::Unit(span);
        };
        let field_name = super_method_name(&owner_name, prop);
        if let Some(class_ty) = self.current_class_ty {
            self.class_method_rets
                .insert((class_ty, field_name.clone(), false), ret);
        }
        HirExpr::Field {
            owner: Box::new(HirExpr::Local {
                id: this_id,
                ty: this_ty,
                span,
            }),
            field: FieldId::from_raw(0),
            field_name,
            ty: ret,
            span,
        }
    }

    fn walk_super_call(
        &mut self,
        senv: &mut SkeletonEnv,
        call: &CallExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let span = core_span_from_oxc(call.span);
        let mut args = Vec::with_capacity(call.arguments.len());
        for arg in &call.arguments {
            match arg {
                arg @ match_expression!(Argument) => {
                    args.push(self.walk_expr(senv, arg.to_expression(), scope));
                }
                _ => {
                    senv.report_unwalked(
                        "spread argument is not supported in `super()`",
                        arg.span(),
                    );
                }
            }
        }
        let Some(parent) = self.current_super_class.clone() else {
            senv.report_unsupported(
                "E0500",
                "`super()` is only supported inside the constructor of a derived class",
                call.span,
            );
            return HirExpr::Unit(span);
        };
        let constructor = Atom::from("constructor");
        if let Some((owner_name, method)) =
            super_ancestor_declaring_method(senv, &parent, &constructor)
        {
            let ret = method.ret;
            let Some((this_id, this_ty)) = scope.lookup("this") else {
                senv.report_unsupported(
                    "E0500",
                    "`super()` inside a nested closure is not supported in this AOT target",
                    call.span,
                );
                return HirExpr::Unit(span);
            };
            let field_name = super_method_name(&owner_name, &constructor);
            if let Some(class_ty) = self.current_class_ty {
                self.class_method_rets
                    .insert((class_ty, field_name.clone(), false), ret);
            }
            let callee = HirExpr::Field {
                owner: Box::new(HirExpr::Local {
                    id: this_id,
                    ty: this_ty,
                    span,
                }),
                field: FieldId::from_raw(0),
                field_name,
                ty: ret,
                span,
            };
            return HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(callee)),
                args,
                type_args: Vec::new(),
                ty: ret,
                span,
            };
        }
        let ty = senv.types.intern(&Type::Void);
        args.push(HirExpr::Unit(span));
        HirExpr::Sequence {
            exprs: args,
            ty,
            span,
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
                let class_binding = scope.visible_class(id.name.as_str());
                let captured_shadows = match (
                    class_binding.as_ref(),
                    self.captured_local_id(id.name.as_str()),
                ) {
                    (Some(binding), Some(local)) => !self
                        .class_value_locals
                        .get(&self.class_value_storage(local))
                        .is_some_and(|classes| classes.contains(&binding.ty)),
                    _ => false,
                };
                if let Some(binding) = class_binding
                    && !captured_shadows
                {
                    binding.ty
                } else {
                    match self.class_value_globals.get(id.name.as_str()) {
                        Some(classes)
                            if classes.len() < 2
                                && (self.module_lets.contains(id.name.as_str())
                                    || self.module_vars.contains(id.name.as_str())) =>
                        {
                            senv.report_unsupported(
                                "E0419",
                                "`new` on a module-level `let` holding a class value cannot know \
                             which class to construct; use `const`, or name the class directly",
                                id.span,
                            );
                            senv.error_ty()
                        }
                        Some(classes) if classes.len() > 1 => {
                            senv.report_unsupported(
                                "E0419",
                                "`new` on a binding that may hold more than one class value is not \
                             supported; construct the class by name instead",
                                id.span,
                            );
                            senv.error_ty()
                        }
                        _ => match self
                            .captured_class_values(id.name.as_str(), id.span)
                            .as_deref()
                        {
                            Some([class_ty]) => *class_ty,
                            Some(_) => {
                                senv.report_unsupported(
                                    "E0419",
                                    "`new` on a binding that may hold more than one class value is \
                                 not supported; construct the class by name instead",
                                    id.span,
                                );
                                senv.error_ty()
                            }
                            None => {
                                if self.captured_local_id(id.name.as_str()).is_some() {
                                    senv.report_unsupported(
                                        "E0419",
                                        "`new` on a captured binding whose class value is not \
                                         known is not supported; construct the class by name \
                                         instead",
                                        id.span,
                                    );
                                    senv.error_ty()
                                } else {
                                    match self.class_types.get(id.name.as_str()) {
                                        Some(&class_ty) => class_ty,
                                        None => senv.error_ty(),
                                    }
                                }
                            }
                        },
                    }
                }
            }
            Expression::Identifier(id) => match scope.lookup(id.name.as_str()) {
                Some((local, _)) => match self
                    .class_value_locals
                    .get(&self.class_value_storage(local))
                {
                    Some(classes) if classes.len() == 1 => classes[0],
                    Some(_) => {
                        senv.report_unsupported(
                            "E0419",
                            "`new` on a binding that may hold more than one class value is not \
                             supported; construct the class by name instead",
                            id.span,
                        );
                        senv.error_ty()
                    }
                    None => senv.error_ty(),
                },
                None => senv.error_ty(),
            },
            _ => {
                let callee_ty = callee_expr.ty();
                let is_member = matches!(
                    &new_expr.callee,
                    Expression::StaticMemberExpression(_)
                        | Expression::ComputedMemberExpression(_)
                        | Expression::PrivateFieldExpression(_)
                );
                if is_member
                    && callee_expr.has_static_type()
                    && !matches!(
                        senv.types.resolve(callee_ty),
                        None | Some(ts_aot_core::Type::Error)
                    )
                    && self.class_types.values().any(|&known| known == callee_ty)
                {
                    callee_ty
                } else {
                    senv.error_ty()
                }
            }
        };
        self.check_local_class_construction(senv, ty, scope, core_span_from_oxc(new_expr.span));
        self.note_may_throw();
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
                if matches!(s.object, Expression::Super(_)) {
                    return self.walk_super_member(senv, s, scope);
                }
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
                if let Expression::Identifier(obj) = &s.object
                    && scope.visible_binding(obj.name.as_str()).is_none()
                    && !self.module_lets.contains(obj.name.as_str())
                    && !self.module_vars.contains(obj.name.as_str())
                    && self.captured_local_id(obj.name.as_str()).is_none()
                    && !SkeletonBuilder::captured_name_is_visible(senv, obj.name.as_str())
                    && self
                        .class_value_globals
                        .get(obj.name.as_str())
                        .is_none_or(|classes| classes.len() <= 1)
                    && let Some(class_ty) = self.class_types.get(obj.name.as_str()).copied()
                    && let Some((global, ty)) = self
                        .static_field_globals
                        .get(&class_ty)
                        .and_then(|statics| statics.get(&Atom::from(s.property.name.as_str())))
                        .cloned()
                {
                    return HirExpr::Global {
                        name: global,
                        ty,
                        span: core_span_from_oxc(s.span),
                    };
                }
                if let Expression::Identifier(obj) = &s.object
                    && !self.in_callee_position
                    && scope.visible_binding(obj.name.as_str()).is_none()
                    && !self.module_lets.contains(obj.name.as_str())
                    && !self.module_vars.contains(obj.name.as_str())
                    && self.captured_local_id(obj.name.as_str()).is_none()
                    && !SkeletonBuilder::captured_name_is_visible(senv, obj.name.as_str())
                    && self
                        .class_value_globals
                        .get(obj.name.as_str())
                        .is_none_or(|classes| classes.len() <= 1)
                    && let Some(class_ty) = self.class_types.get(obj.name.as_str()).copied()
                    && let Some((global, _ret, fn_ty)) = self
                        .static_method_globals
                        .get(&class_ty)
                        .and_then(|methods| methods.get(&Atom::from(s.property.name.as_str())))
                        .cloned()
                {
                    if self.static_method_captures.contains_key(&global) {
                        senv.report_unsupported(
                            "E0419",
                            format!(
                                "the static method `{}` reads a local variable of the function \
                                 that declares the class, so its value cannot be taken out of that \
                                 function; call it there instead",
                                s.property.name.as_str()
                            )
                            .as_str(),
                            s.span,
                        );
                        return HirExpr::Unit(core_span_from_oxc(s.span));
                    }
                    return HirExpr::Global {
                        name: global,
                        ty: fn_ty,
                        span: core_span_from_oxc(s.span),
                    };
                }
                let owner = self.walk_expr(senv, &s.object, scope);
                let owner = if s.optional {
                    Self::wrap_optional_chain(owner, core_span_from_oxc(s.object.span()))
                } else {
                    owner
                };
                let prop = s.property.name.as_str();
                let owner_ty = owner.ty();
                let classified = match &owner {
                    HirExpr::OptionalChain { base, .. } => base.as_ref(),
                    other => other,
                };
                let class_value_classes = match classified {
                    HirExpr::Global { name, .. } => {
                        if SkeletonBuilder::captured_name_is_visible(senv, name.as_str()) {
                            self.captured_class_values(name.as_str(), s.span)
                        } else if let Some(classes) =
                            self.captured_class_values(name.as_str(), s.span)
                        {
                            Some(classes)
                        } else {
                            let module_classes =
                                self.class_value_globals.get(name.as_str()).cloned();
                            let class_name = self
                                .class_types
                                .get(name.as_str())
                                .copied()
                                .map(|class_ty| vec![class_ty]);
                            if module_classes.is_some() || class_name.is_some() {
                                self.pending_class_value_global_reads
                                    .push(PendingGlobalRead {
                                        name: name.as_str().to_owned(),
                                        seen: module_classes.clone().unwrap_or_default(),
                                        span: s.span,
                                    });
                            }
                            module_classes.or(class_name)
                        }
                    }
                    HirExpr::Local { id, .. } => {
                        let storage = self.class_value_storage(*id);
                        let classes = self.class_value_locals.get(&storage).cloned();
                        if let Some(classes) = &classes
                            && !self.loop_frames.is_empty()
                        {
                            let name = match &s.object {
                                Expression::Identifier(ident) => {
                                    Some(ident.name.as_str().to_owned())
                                }
                                _ => None,
                            };
                            for frame in &mut self.loop_frames {
                                frame.push(crate::skeleton::PendingLoopRead {
                                    name: name.clone(),
                                    id: storage,
                                    seen: classes.clone(),
                                    span: s.span,
                                });
                            }
                        }
                        if let Some(name) = self.unknown_class_value_locals.get(&storage).cloned() {
                            senv.report_unsupported(
                                "E0419",
                                format!(
                                    "`{name}` held a class value but was reassigned a value that \
                                     is not one, so a static read through it has no class to \
                                     select; keep the binding a class value, or name the class \
                                     directly"
                                )
                                .as_str(),
                                s.span,
                            );
                        }
                        classes
                    }
                    HirExpr::DefaultConstruct { ty, .. } => Some(vec![*ty]),
                    HirExpr::Field {
                        owner, field_name, ..
                    } => {
                        let owner_ty = owner.ty();
                        let base = self.class_value_field_base(owner, scope.body_id());
                        let escaping = base
                            .as_ref()
                            .is_none_or(|base| !self.class_value_field_base_is_fresh(base));
                        let blocked = self
                            .unknown_class_value_fields
                            .get(&owner_ty)
                            .is_some_and(|fields| fields.contains(field_name))
                            || self
                                .class_value_field_poison_bases
                                .get(&owner_ty)
                                .and_then(|fields| fields.get(field_name))
                                .is_some_and(|bases| {
                                    bases.unknown_base
                                        || if escaping {
                                            !bases.bases.is_empty()
                                        } else {
                                            base.as_ref()
                                                .is_some_and(|base| bases.bases.contains_key(base))
                                        }
                                });
                        if blocked {
                            senv.report_unsupported(
                                "E0419",
                                format!(
                                    "`{field_name}` is a class-value field that was assigned a \
                                     value which is not a class value, so a static read through \
                                     it has no class to select; keep the field a class value"
                                )
                                .as_str(),
                                s.span,
                            );
                            None
                        } else {
                            let classes = if escaping {
                                self.class_value_fields
                                    .get(&owner_ty)
                                    .and_then(|fields| fields.get(field_name))
                                    .filter(|classes| !classes.is_empty())
                                    .cloned()
                            } else {
                                let classes = match &base {
                                    Some(base) => {
                                        self.fresh_field_base_classes(base, owner_ty, field_name)
                                    }
                                    None => {
                                        self.initial_class_value_field_classes(owner_ty, field_name)
                                    }
                                };
                                (!classes.is_empty()).then_some(classes)
                            };
                            if let Some(classes) = &classes {
                                self.pending_class_value_field_reads.push((
                                    owner_ty,
                                    field_name.clone(),
                                    classes.clone(),
                                    s.span,
                                    base.clone(),
                                ));
                            }
                            classes
                        }
                    }
                    _ => None,
                };
                if let Some(classes) = class_value_classes
                    && let Some(expr) =
                        self.static_field_access(&classes, prop, classified, s.span, senv)
                {
                    return expr;
                }
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
                let generator_result_inner = match senv.types.resolve(lookup_ty) {
                    Some(ts_aot_core::Type::GeneratorResult { inner }) => Some(*inner),
                    _ => None,
                };
                let field_ty = if matches!(
                    senv.types.resolve(lookup_ty),
                    Some(ts_aot_core::Type::Struct { .. })
                ) {
                    class_field_ty_in_program(senv, lookup_ty, prop).or_else(|| {
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
                } else if let Some(inner) = generator_result_inner {
                    match prop {
                        "value" => Some(senv.types.intern(&ts_aot_core::Type::Optional {
                            inner,
                            nullish: NullishKinds::UNDEFINED_ONLY,
                        })),
                        "done" => Some(senv.types.intern(&ts_aot_core::Type::Bool)),
                        _ => None,
                    }
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
                if matches!(computed.object, Expression::Super(_)) {
                    let span = core_span_from_oxc(computed.span);
                    let Some(prop) = super_computed_property_name(&computed.expression) else {
                        senv.report_unsupported(
                            "E0500",
                            "`super[...]` is only supported with a literal string or numeric \
                             property name in this AOT target",
                            computed.span,
                        );
                        return HirExpr::Unit(span);
                    };
                    return self.walk_super_property(senv, &prop, span, computed.span, scope);
                }
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
            ME::PrivateFieldExpression(p) => self.walk_private_field(senv, p, scope),
        }
    }

    fn walk_private_field(
        &mut self,
        senv: &mut SkeletonEnv,
        p: &oxc_ast::ast::PrivateFieldExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let span = core_span_from_oxc(p.span);
        let name = p.field.name.as_str().to_owned();
        let Some((declaring_ty, class_name, member)) = self.private_member(name.as_str()) else {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name}` is used outside the body of the class that declares it; a private \
                     member can only be read or written where its name is in scope"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        };
        if member.is_static {
            let receiver_ok = match &p.object {
                Expression::Identifier(obj) => {
                    scope.lookup(obj.name.as_str()).is_none()
                        && !SkeletonBuilder::captured_name_is_visible(senv, obj.name.as_str())
                        && self.class_types.get(obj.name.as_str()).copied() == Some(declaring_ty)
                }
                Expression::ThisExpression(_) => self.in_static_method,
                _ => false,
            };
            if !receiver_ok {
                senv.report_unsupported(
                    "E0500",
                    format!(
                        "the static private member `#{name}` can only be accessed through the \
                         class that declares it"
                    )
                    .as_str(),
                    p.span,
                );
                return HirExpr::Unit(span);
            }
            if let Some((global, ty)) = self
                .static_field_globals
                .get(&declaring_ty)
                .and_then(|statics| statics.get(&member.lowered))
                .cloned()
            {
                return HirExpr::Global {
                    name: global,
                    ty,
                    span,
                };
            }
            if let Some((global, ret, fn_ty)) = self
                .static_method_globals
                .get(&declaring_ty)
                .and_then(|methods| methods.get(&member.lowered))
                .cloned()
            {
                let ty = if self.in_callee_position { ret } else { fn_ty };
                return HirExpr::Global {
                    name: global,
                    ty,
                    span,
                };
            }
        }
        let class_receiver = (member.is_static
            && matches!(p.object, Expression::ThisExpression(_))
            && !class_name.as_str().is_empty())
        .then(|| {
            self.identifier_expr(
                senv,
                class_name.as_str(),
                scope,
                core_span_from_oxc(p.object.span()),
            )
        });
        let owner = match class_receiver {
            Some(owner) => owner,
            None => self.walk_expr(senv, &p.object, scope),
        };
        let owner = if p.optional {
            Self::wrap_optional_chain(owner, core_span_from_oxc(p.object.span()))
        } else {
            owner
        };
        let owner_ty = owner.ty();
        let (lookup_ty, nullish) = match senv.types.resolve(owner_ty) {
            Some(ts_aot_core::Type::Optional { inner, nullish }) => (*inner, Some(*nullish)),
            _ => (owner_ty, None),
        };
        let declared = class_field_ty_in_program(senv, lookup_ty, member.lowered.as_str())
            .or_else(|| {
                senv.program
                    .class_fields
                    .get(&lookup_ty)
                    .and_then(|fields| {
                        fields
                            .iter()
                            .find(|f| f.name == member.lowered)
                            .map(|f| f.ty)
                    })
            })
            .or_else(|| {
                self.class_method_rets
                    .get(&(lookup_ty, member.lowered.clone(), false))
                    .copied()
            });
        let bare_ty = declared.unwrap_or_else(|| senv.error_ty());
        let ty = match nullish {
            Some(_) => {
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
            field_name: member.lowered,
            ty,
            span,
        }
    }

    fn walk_private_in(
        &mut self,
        senv: &mut SkeletonEnv,
        p: &oxc_ast::ast::PrivateInExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let span = core_span_from_oxc(p.span);
        let bool_ty = senv.types.intern(&ts_aot_core::Type::Bool);
        let name = p.left.name.as_str().to_owned();
        let Some((_declaring_ty, _class_name, member)) = self.private_member(name.as_str()) else {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name} in obj` is used outside the body of the class that declares it; a \
                     private member can only be tested where its name is in scope"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        };
        if member.is_static {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name} in obj` for a static private member has no instance to test; this \
                     AOT target supports the ergonomic brand check only for instance private \
                     fields"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        }
        let object = self.walk_expr(senv, &p.right, scope);
        if matches!(
            senv.types.resolve(object.ty()),
            Some(ts_aot_core::Type::Optional { .. } | ts_aot_core::Type::Null)
        ) {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name} in obj` with an `obj` that may be null or undefined throws a \
                     TypeError in JavaScript; narrow the operand to a non-nullish class instance \
                     first"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        }
        let owner_ty = object.ty();
        if !matches!(
            senv.types.resolve(owner_ty),
            Some(ts_aot_core::Type::Struct { .. })
        ) {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name} in obj` needs the class of `obj` to be known statically; the AOT \
                     brand check is a plain field lookup on the object's struct"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        }
        if !member.brand_is_field {
            senv.report_unsupported(
                "E0500",
                format!(
                    "`#{name} in obj` for a private method or accessor is not supported in this \
                     AOT target; only the brand check for an instance private field is lowered"
                )
                .as_str(),
                p.span,
            );
            return HirExpr::Unit(span);
        }
        let present = class_field_ty_in_program(senv, owner_ty, member.lowered.as_str()).is_some()
            || senv
                .program
                .class_fields
                .get(&owner_ty)
                .is_some_and(|fields| fields.iter().any(|f| f.name == member.lowered));
        HirExpr::Sequence {
            exprs: vec![object, HirExpr::Bool(present, span)],
            ty: bool_ty,
            span,
        }
    }

    fn walk_chain(
        &mut self,
        senv: &mut SkeletonEnv,
        chain: &ChainExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let saved_unknown = self.unknown_class_value_locals.clone();
        self.enter_class_value_branch();
        let walked = match &chain.expression {
            ChainElement::CallExpression(call) => self.walk_call(senv, call, scope),
            ChainElement::TSNonNullExpression(non_null) => {
                self.walk_expr(senv, &non_null.expression, scope)
            }
            other @ match_member_expression!(ChainElement) => {
                self.walk_member(senv, other.to_member_expression(), scope)
            }
        };
        self.leave_class_value_branch();
        self.merge_unknown_class_value_locals(saved_unknown);
        walked
    }

    pub(crate) fn class_value_storage(&self, id: LocalId) -> LocalId {
        self.narrowed_stack
            .last()
            .and_then(|frame| frame.get(&id))
            .map_or(id, |(storage, _)| *storage)
    }

    fn forget_class_value_local(&mut self, id: LocalId, name: Option<&str>) {
        if self.class_value_locals.remove(&id).is_some()
            && let Some(name) = name
        {
            self.unknown_class_value_locals.insert(id, name.to_owned());
        }
    }

    fn class_value_field_base(&self, owner: &HirExpr, body_id: u32) -> Option<ClassValueFieldBase> {
        match owner {
            HirExpr::Local { id, .. } => Some(ClassValueFieldBase::Local(
                body_id,
                self.class_value_storage(*id),
            )),
            HirExpr::Global { name, .. } => {
                if self.captured_local_id(name.as_str()).is_some() {
                    return None;
                }
                Some(ClassValueFieldBase::Global(name.clone()))
            }
            _ => None,
        }
    }

    pub(super) fn class_value_poison_facts(
        &self,
        body_id: u32,
        rhs: &HirExpr,
    ) -> ClassValuePoisonFacts {
        if let Some(value) = constant_numeric_value(rhs) {
            return ClassValuePoisonFacts {
                definitely_falsy: value == 0.0,
                definitely_nullish: false,
            };
        }
        match rhs {
            HirExpr::Bool(false, _) => ClassValuePoisonFacts {
                definitely_falsy: true,
                definitely_nullish: false,
            },
            HirExpr::String(text, _) if text.as_str().is_empty() => ClassValuePoisonFacts {
                definitely_falsy: true,
                definitely_nullish: false,
            },
            HirExpr::Null(_) | HirExpr::Undefined(_) => ClassValuePoisonFacts {
                definitely_falsy: true,
                definitely_nullish: true,
            },
            HirExpr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                let then_facts = self.class_value_poison_facts(body_id, then_branch);
                let else_facts = self.class_value_poison_facts(body_id, else_branch);
                ClassValuePoisonFacts {
                    definitely_falsy: then_facts.definitely_falsy && else_facts.definitely_falsy,
                    definitely_nullish: then_facts.definitely_nullish
                        && else_facts.definitely_nullish,
                }
            }
            HirExpr::Local { id, .. } => {
                let storage = self.class_value_storage(*id);
                self.class_value_poisoned_locals
                    .get(&(body_id, *id))
                    .or_else(|| self.class_value_poisoned_locals.get(&(body_id, storage)))
                    .copied()
                    .unwrap_or_default()
            }
            HirExpr::Global { name, .. } => {
                if self.captured_local_id(name.as_str()).is_some() {
                    return ClassValuePoisonFacts::default();
                }
                self.class_value_poisoned_globals
                    .get(name.as_str())
                    .copied()
                    .unwrap_or_default()
            }
            _ => ClassValuePoisonFacts::default(),
        }
    }

    pub(super) fn join_class_value_field_poison(
        &mut self,
        branches: &[(usize, usize)],
        branch_states: &[ClassValueFieldState],
        entry_state: &ClassValueFieldState,
        branch_depth: u32,
    ) {
        let Some((first_start, first_end)) = branches.first().copied() else {
            return;
        };
        let join_branch = self.class_value_write_branch();
        let first_writes = class_value_definite_branch_writes(
            &self.class_value_poison_writes[first_start..first_end],
            branch_depth,
        );
        if first_writes.is_empty() {
            return;
        }
        let keys: Vec<(TypeId, Atom, ClassValueFieldBase)> = first_writes.keys().cloned().collect();
        for key in keys {
            let mut every_branch_holds_a_class = true;
            let mut every_branch_wrote = true;
            let mut joined_facts = ClassValuePoisonFacts {
                definitely_falsy: true,
                definitely_nullish: true,
            };
            for (index, (start, end)) in branches.iter().enumerate() {
                let writes = class_value_definite_branch_writes(
                    &self.class_value_poison_writes[*start..*end],
                    branch_depth,
                );
                let Some(&(holds_class_value, facts)) = writes.get(&key) else {
                    let clean = branch_states
                        .get(index)
                        .is_some_and(|state| state_holds_class_value(state, key.0, &key.1, &key.2));
                    if clean {
                        joined_facts.definitely_falsy &= false;
                        joined_facts.definitely_nullish &= false;
                        continue;
                    }
                    every_branch_wrote = false;
                    break;
                };
                if !holds_class_value
                    || !self.branch_exits_hold_class(*start, *end, &key, entry_state)
                {
                    every_branch_holds_a_class = false;
                }
                joined_facts.definitely_falsy &= facts.definitely_falsy;
                joined_facts.definitely_nullish &= facts.definitely_nullish;
            }
            if !every_branch_wrote {
                every_branch_holds_a_class = false;
                joined_facts = ClassValuePoisonFacts::default();
            }
            let (owner_ty, field, base) = key;
            if every_branch_holds_a_class {
                let mut cleared = false;
                if let Some(bases) = self
                    .class_value_field_poison_bases
                    .get_mut(&owner_ty)
                    .and_then(|fields| fields.get_mut(&field))
                    && !bases.unknown_base
                {
                    bases.bases.remove(&base);
                    bases.fact_depths.remove(&base);
                    bases.fact_branches.remove(&base);
                    cleared = bases.bases.is_empty();
                }
                if cleared {
                    if let Some(fields) = self.class_value_field_poison_bases.get_mut(&owner_ty) {
                        fields.remove(&field);
                    }
                    if let Some(unknown) = self.unknown_class_value_fields.get_mut(&owner_ty) {
                        unknown.remove(&field);
                    }
                }
                let definitely_a_class = !self
                    .unknown_class_value_fields
                    .get(&owner_ty)
                    .is_some_and(|fields| fields.contains(&field))
                    && self
                        .class_value_field_poison_bases
                        .get(&owner_ty)
                        .and_then(|fields| fields.get(&field))
                        .is_none_or(|bases| bases.bases.is_empty() && !bases.unknown_base);
                if definitely_a_class {
                    self.record_joined_class_value_field_class_write(
                        owner_ty,
                        &field,
                        base,
                        branch_depth.saturating_sub(1),
                    );
                }
                continue;
            }
            let bases = self
                .class_value_field_poison_bases
                .entry(owner_ty)
                .or_default()
                .entry(field.clone())
                .or_default();
            if !bases.unknown_base {
                bases.bases.insert(base.clone(), joined_facts);
                bases
                    .fact_depths
                    .insert(base.clone(), branch_depth.saturating_sub(1));
                bases.fact_branches.insert(base, join_branch);
            }
            self.unknown_class_value_fields
                .entry(owner_ty)
                .or_default()
                .insert(field.clone());
        }
    }

    fn class_value_write_depth(&self) -> u32 {
        self.conditional_depth + u32::from(self.try_body_skips == Some(true))
    }

    fn class_value_field_state_holds_class(
        &self,
        owner_ty: TypeId,
        field: &Atom,
        base: Option<&ClassValueFieldBase>,
    ) -> bool {
        if self
            .unknown_class_value_fields
            .get(&owner_ty)
            .is_some_and(|fields| fields.contains(field))
        {
            return false;
        }
        let bases = self
            .class_value_field_poison_bases
            .get(&owner_ty)
            .and_then(|fields| fields.get(field));
        let Some(bases) = bases else {
            return true;
        };
        if bases.unknown_base {
            return false;
        }
        match base {
            Some(base) => !bases.bases.contains_key(base),
            None => bases.bases.is_empty(),
        }
    }

    pub(super) fn note_may_throw(&mut self) {
        if let Some(skips) = self.try_body_skips.as_mut() {
            *skips = true;
        }
    }

    pub(super) fn note_may_exit(&mut self, leaves_a_loop: bool) {
        let breakable = if leaves_a_loop {
            self.class_value_breakable_depth
        } else {
            0
        };
        let exit = crate::skeleton::ClassValueExit {
            position: self.class_value_poison_writes.len(),
            depth: self.class_value_write_depth(),
            leaves_a_loop,
        };
        if leaves_a_loop {
            for frame in &mut self.class_value_loop_frames {
                if breakable <= frame.breakable {
                    frame.exits.push(exit);
                }
            }
        }
        if self.class_value_branch_frames.is_empty() {
            return;
        }
        self.class_value_exits.push(exit);
        for frame in &mut self.class_value_branch_frames {
            if breakable <= frame.breakable {
                frame.exits.push(exit);
            }
        }
    }

    pub(super) fn enter_class_value_loop_body(&mut self) {
        self.class_value_loop_frames
            .push(crate::skeleton::ClassValueLoopFrame {
                breakable: self.class_value_breakable_depth,
                exits: Vec::new(),
            });
    }

    pub(super) fn leave_class_value_loop_body(&mut self) {
        self.class_value_loop_frames.pop();
    }

    fn class_value_write_may_be_skipped(&self) -> bool {
        let position = self.class_value_poison_writes.len();
        self.class_value_loop_frames.iter().any(|frame| {
            frame
                .exits
                .iter()
                .any(|exit| exit.leaves_a_loop && exit.position <= position)
        })
    }

    fn exits_hold_class(
        &self,
        start: usize,
        exits: &[crate::skeleton::ClassValueExit],
        key: &(TypeId, Atom, ClassValueFieldBase),
        entry_state: &ClassValueFieldState,
    ) -> bool {
        let (owner_ty, field, base) = key;
        exits.iter().all(|exit| {
            let last = self.class_value_poison_writes[start..exit.position]
                .iter()
                .rev()
                .find(|write| {
                    write.owner_ty == *owner_ty && write.field == *field && write.base == *base
                });
            match last {
                Some(write) => write.holds_class_value && write.depth <= exit.depth,
                None => state_holds_class_value(entry_state, *owner_ty, field, base),
            }
        })
    }

    pub(super) fn degrade_class_value_fields_written_since(
        &mut self,
        writes_before: usize,
        exits_before: usize,
        entry_state: &ClassValueFieldState,
    ) {
        let exits = self.class_value_exits[exits_before..].to_vec();
        if exits.is_empty() {
            return;
        }
        let mut keys: Vec<(TypeId, Atom, ClassValueFieldBase)> = Vec::new();
        for write in &self.class_value_poison_writes[writes_before..] {
            if !write.holds_class_value {
                continue;
            }
            let key = (write.owner_ty, write.field.clone(), write.base.clone());
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            if self.exits_hold_class(writes_before, &exits, &key, entry_state) {
                continue;
            }
            let (owner_ty, field, _) = key;
            self.unknown_class_value_fields
                .entry(owner_ty)
                .or_default()
                .insert(field.clone());
            if let Some(classes) = self
                .class_value_fields
                .get_mut(&owner_ty)
                .and_then(|fields| fields.get_mut(&field))
            {
                classes.clear();
            }
            if let Some(bases) = self
                .class_value_field_poison_bases
                .get_mut(&owner_ty)
                .and_then(|fields| fields.get_mut(&field))
            {
                for facts in bases.bases.values_mut() {
                    *facts = ClassValuePoisonFacts::default();
                }
            }
        }
    }

    fn branch_exits_hold_class(
        &self,
        start: usize,
        end: usize,
        key: &(TypeId, Atom, ClassValueFieldBase),
        entry_state: &ClassValueFieldState,
    ) -> bool {
        let Some(record) = self
            .class_value_branch_records
            .iter()
            .rev()
            .find(|record| record.start == start && record.end == end)
        else {
            return true;
        };
        self.exits_hold_class(record.start, &record.exits, key, entry_state)
    }

    fn record_class_value_field_poison(
        &mut self,
        owner_ty: TypeId,
        field: &Atom,
        base: Option<ClassValueFieldBase>,
        facts: ClassValuePoisonFacts,
        plain_assign: bool,
    ) {
        let depth = self.class_value_write_depth();
        let branch = self.class_value_write_branch();
        let active = self.class_value_branch_ids.clone();
        let may_be_skipped = self.class_value_write_may_be_skipped();
        let bases = self
            .class_value_field_poison_bases
            .entry(owner_ty)
            .or_default()
            .entry(field.clone())
            .or_default();
        match base {
            Some(base) => {
                let previous_depth = bases.fact_depths.get(&base).copied();
                let previous_branch = bases.fact_branches.get(&base).copied().flatten();
                let dominates_previous = active.is_empty()
                    || match previous_branch {
                        None => true,
                        Some(previous) => active.contains(&previous),
                    };
                let replaces_previous = plain_assign
                    && !may_be_skipped
                    && previous_depth.is_none_or(|previous| depth <= previous)
                    && dominates_previous;
                self.class_value_poison_writes.push(ClassValuePoisonWrite {
                    owner_ty,
                    field: field.clone(),
                    base: base.clone(),
                    facts,
                    plain_assign,
                    depth,
                    holds_class_value: false,
                    branch,
                });
                if replaces_previous {
                    bases.bases.insert(base.clone(), facts);
                    bases.fact_depths.insert(base.clone(), depth);
                    bases.fact_branches.insert(base, branch);
                } else {
                    let previous = bases.bases.get(&base).copied().unwrap_or_default();
                    bases.bases.insert(
                        base.clone(),
                        ClassValuePoisonFacts {
                            definitely_falsy: previous.definitely_falsy && facts.definitely_falsy,
                            definitely_nullish: previous.definitely_nullish
                                && facts.definitely_nullish,
                        },
                    );
                    let established = depth;
                    let merged_branch = match (previous_branch, branch) {
                        (None, None) => None,
                        (None, Some(current)) => Some(current),
                        (Some(previous), Some(current))
                            if previous == current || active.contains(&previous) =>
                        {
                            Some(current)
                        }
                        _ => Some(crate::skeleton::MERGED_CLASS_VALUE_BRANCH),
                    };
                    bases.fact_depths.insert(base.clone(), established);
                    bases.fact_branches.insert(base, merged_branch);
                }
            }
            None => bases.unknown_base = true,
        }
    }

    fn record_class_value_field_class_write(
        &mut self,
        owner_ty: TypeId,
        field: &Atom,
        base: Option<ClassValueFieldBase>,
        plain_assign: bool,
        restored: bool,
    ) {
        let Some(base) = base else {
            return;
        };
        if !plain_assign && !restored {
            return;
        }
        let depth = self.class_value_write_depth();
        self.class_value_poison_writes.push(ClassValuePoisonWrite {
            owner_ty,
            field: field.clone(),
            base,
            facts: ClassValuePoisonFacts::default(),
            plain_assign,
            depth,
            holds_class_value: true,
            branch: self.class_value_write_branch(),
        });
    }

    fn record_joined_class_value_field_class_write(
        &mut self,
        owner_ty: TypeId,
        field: &Atom,
        base: ClassValueFieldBase,
        depth: u32,
    ) {
        self.class_value_poison_writes.push(ClassValuePoisonWrite {
            owner_ty,
            field: field.clone(),
            base,
            facts: ClassValuePoisonFacts::default(),
            plain_assign: true,
            depth,
            holds_class_value: true,
            branch: self.class_value_write_branch(),
        });
    }

    fn class_value_write_branch(&self) -> Option<u32> {
        self.class_value_branch_ids.last().copied()
    }

    fn rebind_class_value_holder_field(
        &mut self,
        holder_ty: TypeId,
        field: &Atom,
        base: &ClassValueFieldBase,
        source_base: &ClassValueFieldBase,
        conditional: bool,
    ) {
        let source = self
            .class_value_field_poison_bases
            .get(&holder_ty)
            .and_then(|fields| fields.get(field))
            .and_then(|bases| bases.bases.get(source_base))
            .copied();
        let source_unknown_base = self
            .class_value_field_poison_bases
            .get(&holder_ty)
            .and_then(|fields| fields.get(field))
            .is_some_and(|bases| bases.unknown_base);
        let mut source_classes = self
            .class_value_fields
            .get(&holder_ty)
            .and_then(|fields| fields.get(field))
            .cloned()
            .unwrap_or_default();
        if source_classes.is_empty() {
            source_classes = self.initial_class_value_field_classes(holder_ty, field);
        }
        if !conditional {
            self.clear_class_value_field_base(holder_ty, field, base);
        }
        if !source_classes.is_empty() {
            let target = self
                .class_value_fields
                .entry(holder_ty)
                .or_default()
                .entry(field.clone())
                .or_default();
            for class_ty in &source_classes {
                if !target.contains(class_ty) {
                    target.push(*class_ty);
                }
            }
        }
        if source.is_none() && !source_unknown_base && !source_classes.is_empty() {
            self.record_class_value_field_class_write(
                holder_ty,
                field,
                Some(base.clone()),
                true,
                false,
            );
        }
        if source_unknown_base || source.is_some() {
            let bases = self
                .class_value_field_poison_bases
                .entry(holder_ty)
                .or_default()
                .entry(field.clone())
                .or_default();
            if source_unknown_base {
                bases.unknown_base = true;
            }
            if let Some(facts) = source {
                let merged = if conditional {
                    let previous = bases.bases.get(base).copied().unwrap_or_default();
                    ClassValuePoisonFacts {
                        definitely_falsy: previous.definitely_falsy && facts.definitely_falsy,
                        definitely_nullish: previous.definitely_nullish && facts.definitely_nullish,
                    }
                } else {
                    facts
                };
                bases.bases.insert(base.clone(), merged);
                bases.fact_depths.entry(base.clone()).or_insert(0);
                bases.fact_branches.insert(
                    base.clone(),
                    Some(crate::skeleton::MERGED_CLASS_VALUE_BRANCH),
                );
            }
            self.unknown_class_value_fields
                .entry(holder_ty)
                .or_default()
                .insert(field.clone());
        }
    }

    fn class_value_branch_is_active(&self, branch: Option<u32>) -> bool {
        branch.is_none_or(|branch| self.class_value_branch_ids.contains(&branch))
    }

    pub(super) fn enter_class_value_branch(&mut self) {
        let id = self.next_class_value_branch_id;
        self.next_class_value_branch_id += 1;
        self.class_value_branch_ids.push(id);
        self.class_value_branch_frames
            .push(crate::skeleton::ClassValueBranchFrame {
                start: self.class_value_poison_writes.len(),
                breakable: self.class_value_breakable_depth,
                exits: Vec::new(),
            });
        self.conditional_depth += 1;
    }

    pub(super) fn leave_class_value_branch(&mut self) {
        self.class_value_branch_ids.pop();
        if let Some(frame) = self.class_value_branch_frames.pop() {
            self.class_value_branch_records
                .push(crate::skeleton::ClassValueBranchRecord {
                    start: frame.start,
                    end: self.class_value_poison_writes.len(),
                    exits: frame.exits,
                });
        }
        self.conditional_depth -= 1;
    }

    fn forget_class_value_field_poison(
        &mut self,
        owner_ty: TypeId,
        field: &Atom,
        base: &ClassValueFieldBase,
        constraint: ClassValuePoisonConstraint,
    ) -> bool {
        let depth = self.class_value_write_depth();
        let active = self.class_value_branch_ids.clone();
        let known = self
            .class_value_field_poison_bases
            .get(&owner_ty)
            .and_then(|fields| fields.get(field))
            .is_some_and(|bases| bases.restorable(constraint, depth, &active));
        if !known {
            return false;
        }
        let restored = self
            .class_value_field_poison_bases
            .get_mut(&owner_ty)
            .and_then(|fields| fields.get_mut(field))
            .is_some_and(|bases| {
                bases.bases.remove(base);
                bases.fact_depths.remove(base);
                bases.fact_branches.remove(base);
                bases.bases.is_empty() && !bases.unknown_base
            });
        if !restored {
            return false;
        }
        if let Some(fields) = self.class_value_field_poison_bases.get_mut(&owner_ty) {
            fields.remove(field);
        }
        if let Some(unknown) = self.unknown_class_value_fields.get_mut(&owner_ty) {
            unknown.remove(field);
        }
        true
    }

    fn rhs_final_class_value_classes(
        &mut self,
        rhs: &mut HirExpr,
        span: oxc_span::Span,
    ) -> Option<Vec<TypeId>> {
        match rhs {
            HirExpr::DefaultConstruct { ty, span, .. } => {
                let (class_ty, span) = (*ty, *span);
                let tag = self.class_value_tag(class_ty);
                *rhs = HirExpr::Int(tag, span);
                Some(vec![class_ty])
            }
            HirExpr::Global { name, .. } => {
                let name = name.as_str().to_owned();
                if self.captured_local_id(name.as_str()).is_some() {
                    return self.captured_class_values(name.as_str(), span);
                }
                if self.module_lets.contains(name.as_str())
                    || self.module_vars.contains(name.as_str())
                {
                    return None;
                }
                self.class_value_globals.get(&name).cloned().or_else(|| {
                    self.class_types
                        .get(&name)
                        .copied()
                        .map(|class_ty| vec![class_ty])
                })
            }
            HirExpr::Local { id, .. } => {
                let storage = self.class_value_storage(*id);
                if self.unknown_class_value_locals.contains_key(&storage) {
                    return None;
                }
                self.class_value_locals.get(&storage).cloned()
            }
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let owner_ty = owner.ty();
                if self
                    .unknown_class_value_fields
                    .get(&owner_ty)
                    .is_some_and(|fields| fields.contains(field_name))
                {
                    return None;
                }
                self.class_value_fields
                    .get(&owner_ty)
                    .and_then(|fields| fields.get(field_name))
                    .filter(|classes| !classes.is_empty())
                    .cloned()
            }
            _ => None,
        }
    }

    fn resolve_static_method_callee(
        &mut self,
        senv: &mut SkeletonEnv,
        callee: HirExpr,
        span: oxc_span::Span,
        scope: &BodyScope,
    ) -> HirExpr {
        let HirExpr::Field {
            owner, field_name, ..
        } = &callee
        else {
            return callee;
        };
        if let HirExpr::Global { name, .. } = owner.as_ref()
            && self.class_types.contains_key(name.as_str())
            && !self.class_value_globals.contains_key(name.as_str())
            && self.captured_local_id(name.as_str()).is_none()
            && !SkeletonBuilder::captured_name_is_visible(senv, name.as_str())
        {
            return callee;
        }
        let owner_ty = owner.ty();
        let base = self.class_value_field_base(owner, scope.body_id());
        let escaping = base
            .as_ref()
            .is_none_or(|base| !self.class_value_field_base_is_fresh(base));
        let blocked = self
            .unknown_class_value_fields
            .get(&owner_ty)
            .is_some_and(|fields| fields.contains(field_name))
            || self
                .class_value_field_poison_bases
                .get(&owner_ty)
                .and_then(|fields| fields.get(field_name))
                .is_some_and(|bases| {
                    bases.unknown_base
                        || if escaping {
                            !bases.bases.is_empty()
                        } else {
                            base.as_ref()
                                .is_some_and(|base| bases.bases.contains_key(base))
                        }
                });
        if blocked {
            senv.report_unsupported(
                "E0419",
                format!(
                    "`{field_name}` is a class-value field that was assigned a value which is not \
                     a class value, so a static call through it has no class to select; keep the \
                     field a class value"
                )
                .as_str(),
                span,
            );
            return HirExpr::Unit(core_span_from_oxc(span));
        }
        let mut owner_expr = owner.as_ref().clone();
        let Some(classes) = self.rhs_final_class_value_classes(&mut owner_expr, span) else {
            return callee;
        };
        if classes.is_empty() {
            return callee;
        }
        match self.static_method_access(&classes, field_name.as_str(), span, senv) {
            Some(replacement) => replacement,
            None => callee,
        }
    }

    fn static_method_access(
        &self,
        classes: &[TypeId],
        method: &str,
        span: oxc_span::Span,
        senv: &mut SkeletonEnv,
    ) -> Option<HirExpr> {
        let name = Atom::from(method);
        let mut found: Vec<(Atom, TypeId)> = Vec::new();
        for class_ty in classes {
            if let Some((global, ty, _fn_ty)) = self
                .static_method_globals
                .get(class_ty)
                .and_then(|methods| methods.get(&name))
                .cloned()
            {
                found.push((global, ty));
            }
        }
        if found.is_empty() {
            return None;
        }
        if classes.len() != 1 || found.len() != 1 {
            senv.report_unsupported(
                "E0419",
                format!(
                    "static method `{method}` cannot be called on a binding that may hold more \
                     than one class value; name the class directly"
                )
                .as_str(),
                span,
            );
            return Some(HirExpr::Unit(core_span_from_oxc(span)));
        }
        let (global, ty) = found.remove(0);
        Some(HirExpr::Global {
            name: global,
            ty,
            span: core_span_from_oxc(span),
        })
    }

    fn static_field_access(
        &mut self,
        classes: &[TypeId],
        field: &str,
        owner: &HirExpr,
        span: oxc_span::Span,
        senv: &mut SkeletonEnv,
    ) -> Option<HirExpr> {
        let arms: Vec<(TypeId, Atom, TypeId)> = classes
            .iter()
            .filter_map(|class_ty| {
                self.static_field_globals
                    .get(class_ty)
                    .and_then(|statics| statics.get(&Atom::from(field)))
                    .cloned()
                    .map(|(global, ty)| (*class_ty, global, ty))
            })
            .collect();
        let (_, first_global, first_ty) = arms.first()?.clone();
        let hir_span = core_span_from_oxc(span);
        if arms.len() != classes.len() {
            senv.report_unsupported(
                "E0419",
                format!(
                    "`{field}` is not a static field of every class this value may hold; the \
                     read is only defined for some of them"
                )
                .as_str(),
                span,
            );
        } else if arms.windows(2).any(|pair| pair[0].2 != pair[1].2) {
            senv.report_unsupported(
                "E0419",
                format!(
                    "static field `{field}` has a different type on the classes this value may \
                     hold; reading it cannot pick one type"
                )
                .as_str(),
                span,
            );
        }
        if arms.len() == 1 {
            return Some(HirExpr::Global {
                name: first_global,
                ty: first_ty,
                span: hir_span,
            });
        }
        let bool_ty = senv.types.intern(&ts_aot_core::Type::Bool);
        let mut expr = HirExpr::Global {
            name: arms.last()?.1.clone(),
            ty: arms.last()?.2,
            span: hir_span,
        };
        for (class_ty, global, ty) in arms.iter().rev().skip(1) {
            let tag = self.class_value_tag(*class_ty);
            let cond = HirExpr::Binary {
                op: HirBinaryOp::StrictEq,
                lhs: Box::new(owner.clone()),
                rhs: Box::new(HirExpr::Int(tag, hir_span)),
                ty: bool_ty,
                span: hir_span,
            };
            expr = HirExpr::Ternary {
                cond: Box::new(cond),
                then_branch: Box::new(HirExpr::Global {
                    name: global.clone(),
                    ty: *ty,
                    span: hir_span,
                }),
                else_branch: Box::new(expr),
                ty: first_ty,
                span: hir_span,
            };
        }
        Some(expr)
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
        let mut target = self.walk_simple_target(senv, simple_target, scope);
        if let HirExpr::Index {
            owner, index, span, ..
        } = &target
            && let HirExpr::String(name, _) = index.as_ref()
            && let Some(field_ty) = senv
                .program
                .class_fields
                .get(&owner.ty())
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|f| f.name.as_str() == name.as_str())
                        .map(|f| f.ty)
                })
        {
            target = HirExpr::Field {
                owner: owner.clone(),
                field: FieldId::from_raw(0),
                field_name: name.clone(),
                ty: field_ty,
                span: *span,
            };
        }
        if matches!(target, HirExpr::Ternary { .. }) {
            senv.report_unsupported(
                "E0419",
                "assigning a static field through a class value that may hold several classes is \
                 not supported; assign through the class name instead",
                a.span,
            );
        }
        let mut rhs = self.walk_expr(senv, &a.right, scope);
        if let HirExpr::Local { id, .. } = &target {
            if let Some(bodies) = crate::skeleton::closure_bodies_of(&rhs) {
                for body in bodies {
                    scope.set_closure_body(*id, body);
                }
            } else if crate::skeleton::may_hold_a_callable(&rhs) {
                scope.poison_closure_body(*id);
            }
        }
        if let HirExpr::Local { id, .. } = &target {
            let storage = self.class_value_storage(*id);
            match (a.operator, &rhs) {
                (AssignmentOperator::Assign, HirExpr::Int(value, _)) => {
                    self.constant_int_locals.insert(storage, *value);
                }
                _ => {
                    self.constant_int_locals.remove(&storage);
                }
            }
        }
        let op = compound_op(a.operator);
        let keeps_old_value = matches!(
            op,
            CompoundOp::Binary(HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or)
        );
        if let HirExpr::Global { name, .. } = &target {
            let name = name.as_str().to_owned();
            if self.captured_class_value_local(name.as_str()).is_some() {
                let _ = self.rhs_final_class_value_classes(&mut rhs, a.span);
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "assigning a class value captured by a closure is not supported; assign \
                         `{name}` in the function that declares it"
                    )
                    .as_str(),
                    a.span,
                );
            } else if self.captured_local_id(name.as_str()).is_some() {
            } else {
                let final_source = if let HirExpr::Global { name: source, .. } = &rhs {
                    let source = source.as_str().to_owned();
                    if self.captured_local_id(source.as_str()).is_none()
                        && !self.module_lets.contains(source.as_str())
                        && !self.module_vars.contains(source.as_str())
                    {
                        self.class_value_globals
                            .get(source.as_str())
                            .cloned()
                            .or_else(|| {
                                self.class_types
                                    .get(source.as_str())
                                    .copied()
                                    .map(|class_ty| vec![class_ty])
                            })
                    } else {
                        None
                    }
                } else {
                    None
                };
                let tracked: Option<Vec<TypeId>> =
                    if let HirExpr::DefaultConstruct { ty, span, .. } = &rhs {
                        let (class_ty, span) = (*ty, *span);
                        let tag = self.class_value_tag(class_ty);
                        rhs = HirExpr::Int(tag, span);
                        Some(vec![class_ty])
                    } else {
                        final_source
                    };
                match tracked {
                    Some(classes)
                        if self.module_lets.contains(&name) || self.module_vars.contains(&name) =>
                    {
                        let target_classes = self.class_value_globals.entry(name).or_default();
                        for class_ty in classes {
                            if !target_classes.contains(&class_ty) {
                                target_classes.push(class_ty);
                            }
                        }
                    }
                    _ => {
                        self.unknown_class_value_globals.insert(name);
                    }
                }
            }
        }
        if let HirExpr::Local { id, .. } = &target {
            let body_id = scope.body_id();
            let storage = self.class_value_storage(*id);
            self.class_value_poisoned_locals.remove(&(body_id, *id));
            self.class_value_poisoned_locals.remove(&(body_id, storage));
            let fresh_base = ClassValueFieldBase::Local(body_id, storage);
            if matches!(op, CompoundOp::Assign) && matches!(rhs, HirExpr::New { .. }) {
                self.note_fresh_field_base(fresh_base);
            } else {
                self.forget_fresh_field_base(&fresh_base);
            }
            let id = storage;
            let name = match simple_target {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                    Some(ident.name.as_str().to_owned())
                }
                _ => None,
            };
            if matches!(
                op,
                CompoundOp::Assign
                    | CompoundOp::Binary(
                        HirBinaryOp::And | HirBinaryOp::Or | HirBinaryOp::Coalesce
                    )
            ) {
                match &rhs {
                    HirExpr::DefaultConstruct { ty, span, .. } => {
                        let (class_ty, span) = (*ty, *span);
                        let tag = self.class_value_tag(class_ty);
                        rhs = HirExpr::Int(tag, span);
                        if !keeps_old_value {
                            self.unknown_class_value_locals.remove(&id);
                        }
                        let classes = self.class_value_locals.entry(id).or_default();
                        if !classes.contains(&class_ty) {
                            classes.push(class_ty);
                        }
                    }
                    HirExpr::Local { id: source, .. } => {
                        let source = self.class_value_storage(*source);
                        match self.class_value_locals.get(&source).cloned() {
                            Some(classes) => {
                                let source_unknown =
                                    self.unknown_class_value_locals.contains_key(&source);
                                self.record_loop_class_value_copies(
                                    &[(source, classes.clone())],
                                    a.span,
                                );
                                if !keeps_old_value {
                                    self.unknown_class_value_locals.remove(&id);
                                }
                                if source_unknown && let Some(target_name) = &name {
                                    self.unknown_class_value_locals
                                        .insert(id, target_name.clone());
                                }
                                let target_classes = self.class_value_locals.entry(id).or_default();
                                for class_ty in classes {
                                    if !target_classes.contains(&class_ty) {
                                        target_classes.push(class_ty);
                                    }
                                }
                            }
                            None => {
                                self.forget_class_value_local(id, name.as_deref());
                            }
                        }
                    }
                    _ => {
                        self.forget_class_value_local(id, name.as_deref());
                    }
                }
            } else {
                self.forget_class_value_local(id, name.as_deref());
            }
        }
        if matches!(op, CompoundOp::Assign)
            && let HirExpr::Local {
                id, ty: holder_ty, ..
            } = &target
        {
            let holder_ty = *holder_ty;
            let body_id = scope.body_id();
            let storage = self.class_value_storage(*id);
            let same_holder = matches!(
                &rhs,
                HirExpr::Local { id: source, .. } if self.class_value_storage(*source) == storage
            );
            if !same_holder {
                let fields = self.class_value_field_names(holder_ty);
                if !fields.is_empty() {
                    let fresh = matches!(
                        &rhs,
                        HirExpr::New { ty, .. } | HirExpr::DefaultConstruct { ty, .. }
                            if *ty == holder_ty
                    );
                    let source_base = match &rhs {
                        HirExpr::Local {
                            id: source,
                            ty: source_ty,
                            ..
                        } if *source_ty == holder_ty => Some(ClassValueFieldBase::Local(
                            body_id,
                            self.class_value_storage(*source),
                        )),
                        _ => None,
                    };
                    let conditional = !self.class_value_branch_ids.is_empty()
                        || self.try_body_skips == Some(true);
                    let base = ClassValueFieldBase::Local(body_id, storage);
                    for field in fields {
                        if let Some(source_base) = &source_base {
                            self.rebind_class_value_holder_field(
                                holder_ty,
                                &field,
                                &base,
                                source_base,
                                conditional,
                            );
                            continue;
                        }
                        if conditional {
                            let was_a_class = self.class_value_field_state_holds_class(
                                holder_ty,
                                &field,
                                Some(&base),
                            ) && self
                                .class_value_fields
                                .get(&holder_ty)
                                .and_then(|fields| fields.get(&field))
                                .is_some_and(|classes| !classes.is_empty());
                            let initials = if fresh {
                                self.initial_class_value_field_classes(holder_ty, &field)
                            } else {
                                Vec::new()
                            };
                            if was_a_class && !initials.is_empty() {
                                let target_classes = self
                                    .class_value_fields
                                    .entry(holder_ty)
                                    .or_default()
                                    .entry(field.clone())
                                    .or_default();
                                for class_ty in initials {
                                    if !target_classes.contains(&class_ty) {
                                        target_classes.push(class_ty);
                                    }
                                }
                                continue;
                            }
                            self.clear_class_value_field_base(holder_ty, &field, &base);
                            self.unknown_class_value_fields
                                .entry(holder_ty)
                                .or_default()
                                .insert(field.clone());
                            let bases = self
                                .class_value_field_poison_bases
                                .entry(holder_ty)
                                .or_default()
                                .entry(field.clone())
                                .or_default();
                            if !bases.unknown_base {
                                bases.bases.entry(base.clone()).or_default();
                                bases.fact_depths.entry(base.clone()).or_insert(0);
                                bases.fact_branches.insert(
                                    base.clone(),
                                    Some(crate::skeleton::MERGED_CLASS_VALUE_BRANCH),
                                );
                            }
                            continue;
                        }
                        self.clear_class_value_field_base(holder_ty, &field, &base);
                        let blocked = self
                            .unknown_class_value_fields
                            .get(&holder_ty)
                            .is_some_and(|unknown| unknown.contains(&field))
                            || self
                                .class_value_field_poison_bases
                                .get(&holder_ty)
                                .and_then(|fields| fields.get(&field))
                                .is_some_and(|bases| !bases.bases.is_empty() || bases.unknown_base);
                        if fresh && !blocked {
                            let initials =
                                self.initial_class_value_field_classes(holder_ty, &field);
                            if initials.is_empty() {
                                continue;
                            }
                            let target_classes = self
                                .class_value_fields
                                .entry(holder_ty)
                                .or_default()
                                .entry(field.clone())
                                .or_default();
                            for class_ty in initials {
                                if !target_classes.contains(&class_ty) {
                                    target_classes.push(class_ty);
                                }
                            }
                            self.record_class_value_field_class_write(
                                holder_ty,
                                &field,
                                Some(base.clone()),
                                true,
                                false,
                            );
                        } else if !fresh {
                            self.unknown_class_value_fields
                                .entry(holder_ty)
                                .or_default()
                                .insert(field.clone());
                            if let Some(classes) = self
                                .class_value_fields
                                .get_mut(&holder_ty)
                                .and_then(|fields| fields.get_mut(&field))
                            {
                                classes.clear();
                            }
                        }
                    }
                }
            }
        }
        let mut class_value_plain_assign = false;
        if let HirExpr::Field {
            owner, field_name, ..
        } = &target
        {
            let owner_ty = owner.ty();
            let field_name = field_name.clone();
            let base = self.class_value_field_base(owner, scope.body_id());
            let is_class_value_field = self
                .class_value_fields
                .get(&owner_ty)
                .is_some_and(|fields| fields.contains_key(&field_name));
            let self_assign = matches!(
                op,
                CompoundOp::Assign
                    | CompoundOp::Binary(
                        HirBinaryOp::And | HirBinaryOp::Or | HirBinaryOp::Coalesce
                    )
            ) && base.as_ref().is_some_and(|base| {
                matches!(
                    &rhs,
                    HirExpr::Field { owner: rhs_owner, field_name: rhs_field, .. }
                        if *rhs_field == field_name
                            && self
                                .class_value_field_base(rhs_owner, scope.body_id())
                                .as_ref()
                                == Some(base)
                )
            });
            if is_class_value_field {
                let proven_class = self
                    .class_value_poison_writes
                    .iter()
                    .rev()
                    .find(|write| {
                        write.owner_ty == owner_ty
                            && write.field == field_name
                            && base.as_ref().is_none_or(|base| &write.base == base)
                            && self.class_value_branch_is_active(write.branch)
                    })
                    .is_none_or(|write| write.holds_class_value)
                    && self.class_value_field_state_holds_class(
                        owner_ty,
                        &field_name,
                        base.as_ref(),
                    );
                let and_assign = matches!(op, CompoundOp::Binary(HirBinaryOp::And));
                let proven_class_assign = and_assign && proven_class;
                class_value_plain_assign = proven_class_assign;
                let tracked = if self_assign
                    || !matches!(
                        op,
                        CompoundOp::Assign
                            | CompoundOp::Binary(
                                HirBinaryOp::And | HirBinaryOp::Or | HirBinaryOp::Coalesce
                            )
                    ) {
                    None
                } else {
                    self.rhs_final_class_value_classes(&mut rhs, a.span)
                };
                if proven_class
                    && matches!(
                        op,
                        CompoundOp::Binary(HirBinaryOp::Or | HirBinaryOp::Coalesce)
                    )
                {
                    return target;
                }
                let rhs_facts = self.class_value_poison_facts(scope.body_id(), &rhs);
                let plain_assign = matches!(op, CompoundOp::Assign) || proven_class_assign;
                if !self_assign {
                    match tracked {
                        Some(classes) if !classes.is_empty() => {
                            let constraint = match op {
                                CompoundOp::Assign => Some(ClassValuePoisonConstraint::Always),
                                CompoundOp::Binary(HirBinaryOp::Or) => {
                                    Some(ClassValuePoisonConstraint::DefinitelyFalsy)
                                }
                                CompoundOp::Binary(HirBinaryOp::Coalesce) => {
                                    Some(ClassValuePoisonConstraint::DefinitelyNullish)
                                }
                                CompoundOp::Binary(HirBinaryOp::And) if proven_class => {
                                    Some(ClassValuePoisonConstraint::Always)
                                }
                                _ => None,
                            };
                            let restored =
                                if let (Some(constraint), Some(base)) = (constraint, &base) {
                                    self.forget_class_value_field_poison(
                                        owner_ty,
                                        &field_name,
                                        base,
                                        constraint,
                                    )
                                } else {
                                    false
                                };
                            self.record_class_value_field_class_write(
                                owner_ty,
                                &field_name,
                                base.clone(),
                                plain_assign,
                                restored,
                            );
                            if let Some(fresh_base) = base
                                .as_ref()
                                .filter(|base| self.class_value_field_base_is_fresh(base))
                                .cloned()
                            {
                                let mut updated = self.fresh_field_base_classes_or_initials(
                                    &fresh_base,
                                    owner_ty,
                                    &field_name,
                                );
                                for class_ty in &classes {
                                    if !updated.contains(class_ty) {
                                        updated.push(*class_ty);
                                    }
                                }
                                self.set_fresh_field_base_classes(
                                    &fresh_base,
                                    owner_ty,
                                    &field_name,
                                    updated,
                                );
                            }
                            let target = self
                                .class_value_fields
                                .entry(owner_ty)
                                .or_default()
                                .entry(field_name.clone())
                                .or_default();
                            for class_ty in classes {
                                if !target.contains(&class_ty) {
                                    target.push(class_ty);
                                }
                            }
                        }
                        _ => {
                            let escaping = base
                                .as_ref()
                                .is_none_or(|base| !self.class_value_field_base_is_fresh(base));
                            if escaping {
                                self.unknown_class_value_fields
                                    .entry(owner_ty)
                                    .or_default()
                                    .insert(field_name.clone());
                            } else if let Some(base) = &base {
                                let unconditional = self.class_value_branch_ids.is_empty()
                                    && self.try_body_skips != Some(true);
                                if plain_assign && unconditional {
                                    self.set_fresh_field_base_classes(
                                        base,
                                        owner_ty,
                                        &field_name,
                                        Vec::new(),
                                    );
                                }
                            }
                            let conditional = !self.class_value_branch_ids.is_empty()
                                || self.try_body_skips == Some(true);
                            self.record_class_value_field_poison(
                                owner_ty,
                                &field_name,
                                base,
                                rhs_facts,
                                plain_assign,
                            );
                            if escaping
                                && plain_assign
                                && !conditional
                                && let Some(classes) = self
                                    .class_value_fields
                                    .get_mut(&owner_ty)
                                    .and_then(|fields| fields.get_mut(&field_name))
                            {
                                classes.clear();
                            }
                        }
                    }
                }
                if and_assign && !proven_class {
                    senv.report_unsupported(
                        CLASS_VALUE_FIELD_LOGICAL_ASSIGN_CODE,
                        CLASS_VALUE_FIELD_LOGICAL_ASSIGN,
                        a.span,
                    );
                }
            }
        }
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
                target: Box::new(self.redirect_narrowed_target(target, scope)),
                value: Box::new(rhs),
                ty,
                span: core_span_from_oxc(a.span),
            },
            CompoundOp::Binary(HirBinaryOp::And) if class_value_plain_assign => {
                HirExpr::Assignment {
                    target: Box::new(self.redirect_narrowed_target(target, scope)),
                    value: Box::new(rhs),
                    ty,
                    span: core_span_from_oxc(a.span),
                }
            }
            CompoundOp::Binary(op) => {
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
                let name = id.name.as_str();
                if scope.lookup(name).is_none()
                    && !SkeletonBuilder::captured_name_is_visible(senv, name)
                    && self.visible_generator_alias(name).is_some()
                {
                    senv.report_unsupported(
                        "E0500",
                        format!(
                            "reassigning the local generator `{name}` is not supported; a \
                             generator function declaration is lowered as a direct callable and \
                             cannot be written to"
                        )
                        .as_str(),
                        core_span_from_oxc(id.span),
                    );
                }
                self.identifier_expr(senv, id.name.as_str(), scope, core_span_from_oxc(id.span))
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

fn generator_element_ty(
    builder: &mut SkeletonBuilder,
    senv: &mut SkeletonEnv<'_>,
    annotation: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
    resolved: TypeId,
    is_async: bool,
    scope: &BodyScope,
) -> TypeId {
    use oxc_ast::ast::{TSType, TSTypeName};
    if let Some(TSType::TSTypeReference(reference)) = annotation.map(|a| &a.type_annotation)
        && let TSTypeName::IdentifierReference(id) = &reference.type_name
        && is_generator_wrapper_name(id.name.as_str(), is_async)
        && let Some(arguments) = &reference.type_arguments
        && let Some(yield_ty) = arguments.params.first()
    {
        return builder.resolve_ts_type_with_params_in_scope(
            senv,
            Some(yield_ty),
            None,
            Some(scope),
            true,
        );
    }
    resolved
}

fn is_generator_wrapper_name(name: &str, is_async: bool) -> bool {
    match name {
        "Generator" | "IterableIterator" | "Iterator" => !is_async,
        "AsyncGenerator" | "AsyncIterableIterator" | "AsyncIterator" => is_async,
        _ => false,
    }
}

fn infer_generator_element_ty(body: &[HirStmt], types: &mut TypeTable) -> Option<TypeId> {
    struct YieldTyScan<'a> {
        types: &'a mut TypeTable,
        found: Vec<TypeId>,
    }
    impl Visitor for YieldTyScan<'_> {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Yield {
                expr: Some(inner), ..
            } = expr
                && let Some(ty) = literal_hir_ty(self.types, inner).or_else(|| {
                    match self.types.resolve(inner.ty()) {
                        None | Some(Type::Error) => None,
                        Some(_) => Some(inner.ty()),
                    }
                })
            {
                self.found.push(ty);
            }
            walk_expr(self, expr);
        }
    }
    let mut scan = YieldTyScan {
        types,
        found: Vec::new(),
    };
    for stmt in body {
        scan.visit_stmt(stmt);
    }
    let YieldTyScan { types, found } = scan;
    let first = *found.first()?;
    let all_numeric = found.iter().all(|ty| {
        matches!(
            types.resolve(*ty),
            Some(
                Type::F32
                    | Type::F64
                    | Type::I8
                    | Type::I16
                    | Type::I32
                    | Type::I64
                    | Type::U8
                    | Type::U16
                    | Type::U32
                    | Type::U64
            )
        )
    });
    let any_float = found
        .iter()
        .any(|ty| matches!(types.resolve(*ty), Some(Type::F32 | Type::F64)));
    if all_numeric && any_float {
        return Some(types.intern(&Type::F64));
    }
    Some(first)
}

fn declares_name(program: &ts_aot_ir_hir::HirProgram, name: &Atom) -> bool {
    fn scan(decls: &[HirDecl], name: &Atom) -> bool {
        decls.iter().any(|decl| match decl {
            HirDecl::Function(f) => f.name == *name,
            HirDecl::Class(c) => c.name == *name,
            HirDecl::Global { name: n, .. }
            | HirDecl::Enum { name: n, .. }
            | HirDecl::TypeAlias { name: n, .. }
            | HirDecl::Interface { name: n, .. } => n == name,
            HirDecl::Namespace { members, .. } => scan(members, name),
        })
    }
    scan(&program.declarations, name)
}

fn super_class_decl<'a>(senv: &'a SkeletonEnv<'_>, name: &Atom) -> Option<&'a HirClass> {
    senv.program
        .declarations
        .iter()
        .find_map(|decl| match decl {
            HirDecl::Class(class) if class.name == *name => Some(class),
            _ => None,
        })
}

fn super_ancestor_declaring_method(
    senv: &SkeletonEnv<'_>,
    direct_parent: &Atom,
    prop: &Atom,
) -> Option<(Atom, HirFunction)> {
    let mut current = direct_parent.clone();
    let mut visited: Vec<Atom> = Vec::new();
    loop {
        if visited.contains(&current) {
            return None;
        }
        visited.push(current.clone());
        let class = super_class_decl(senv, &current)?;
        if let Some(method) = class.methods.iter().find(|method| {
            method.name == *prop
                && method
                    .params
                    .first()
                    .is_some_and(|param| param.name.as_str() == "this")
        }) {
            return Some((class.name.clone(), method.clone()));
        }
        current = class.extends.clone()?;
    }
}

fn super_method_name(owner: &Atom, prop: &Atom) -> Atom {
    Atom::from(format!(
        "__super_{}_{}_{}",
        owner.as_str().chars().count(),
        owner.as_str(),
        prop.as_str()
    ))
}

fn super_computed_property_name(expr: &Expression<'_>) -> Option<Atom> {
    match expr {
        Expression::StringLiteral(s) => Some(Atom::from(s.value.as_str())),
        Expression::NumericLiteral(n) => Some(Atom::from(n.value.to_string().as_str())),
        Expression::ParenthesizedExpression(p) => super_computed_property_name(&p.expression),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref().map(|s| Atom::from(s.as_str()))),
        _ => None,
    }
}

fn class_field_ty_in_program(
    senv: &SkeletonEnv<'_>,
    ty: TypeId,
    field_name: &str,
) -> Option<TypeId> {
    let mut current = ty;
    let mut visited: Vec<TypeId> = Vec::new();
    loop {
        if visited.contains(&current) {
            return None;
        }
        visited.push(current);
        let class = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.ty == current => Some(class),
                _ => None,
            })?;
        if let Some(field) = class
            .fields
            .iter()
            .find(|field| field.name.as_str() == field_name)
        {
            return Some(field.ty);
        }
        let parent = class.extends.as_ref()?;
        current = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.name == *parent => Some(class.ty),
                _ => None,
            })?;
    }
}

fn class_method_ret_in_program(
    senv: &SkeletonEnv<'_>,
    ty: TypeId,
    method: &str,
    static_method: bool,
) -> Option<TypeId> {
    let mut current = ty;
    let mut visited: Vec<TypeId> = Vec::new();
    loop {
        if visited.contains(&current) {
            return None;
        }
        visited.push(current);
        let class = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.ty == current => Some(class),
                _ => None,
            })?;
        if let Some(found) = class.methods.iter().find(|method_def| {
            method_def.name.as_str() == method
                && method_def
                    .params
                    .first()
                    .is_none_or(|param| param.name.as_str() != "this")
                    == static_method
        }) {
            return match senv.types.resolve(found.ret) {
                None | Some(ts_aot_core::Type::Error) => None,
                Some(_) => Some(found.ret),
            };
        }
        let parent = class.extends.as_ref()?;
        current = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.name == *parent => Some(class.ty),
                _ => None,
            })?;
    }
}

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
        .unwrap_or_else(|| LocalId::from_raw(u32::MAX))
}

impl SkeletonBuilder {
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

    fn withdraw_captured_narrowings(
        &mut self,
        scope: &mut BodyScope,
        captures: &[HirExpr],
        written: &WrittenTargets,
    ) {
        let mut name_of_id: std::collections::HashMap<LocalId, String> =
            std::collections::HashMap::new();
        for name in scope.names() {
            if let Some(binding) = scope.visible_binding(&name) {
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

pub(crate) fn capture_binding_map(
    scope: &BodyScope,
    narrowed: &std::collections::HashMap<LocalId, (LocalId, TypeId)>,
) -> std::collections::HashMap<String, (LocalId, TypeId)> {
    let mut map = std::collections::HashMap::new();
    for name in scope.names() {
        let Some(id) = scope.visible_binding(&name) else {
            continue;
        };
        let Some(ty) = scope.type_of_visible(&name) else {
            continue;
        };
        let binding = narrowed.get(&id).copied().unwrap_or((id, ty));
        map.insert(name, binding);
    }
    map
}

pub(crate) fn capture_type_map(
    scope: &BodyScope,
    narrowed: &std::collections::HashMap<LocalId, (LocalId, TypeId)>,
) -> std::collections::HashMap<String, TypeId> {
    let mut map = std::collections::HashMap::new();
    for name in scope.names() {
        if let Some(id) = scope.visible_binding(&name)
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

struct WrittenTargets {
    ids: std::collections::HashSet<LocalId>,
    names: std::collections::HashSet<String>,
}

struct CallSearch {
    found: bool,
    this_id: LocalId,
}

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
