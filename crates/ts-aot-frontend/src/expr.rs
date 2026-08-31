use oxc_ast::ast::{
    Argument, AssignmentExpression, AssignmentTarget, BinaryExpression, BindingPattern,
    CallExpression, Expression, LogicalExpression, SequenceExpression, SimpleAssignmentTarget,
    TaggedTemplateExpression, TemplateLiteral, UnaryExpression, UpdateExpression,
    match_assignment_target, match_assignment_target_pattern, match_expression,
    match_member_expression,
};
use oxc_ecmascript::{ToBigInt, WithoutGlobalReferenceInformation};
use oxc_span::GetSpan;
use oxc_syntax::operator::UpdateOperator;
use ts_aot_core::{Atom, Diagnostic, FieldId, LocalId, Span, Type, TypeId};
use ts_aot_ir_hir::{HirBinaryOp, HirCallee, HirExpr, HirParam, HirStmt};

use crate::ops::{
    CompoundOp, compound_op, map_binary_op, map_logical_op, map_unary_op, number_to_hir,
};
use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv, TLA_ONLY_BINDING_CODE};
use crate::util::{binding_pattern_name, core_span_from_oxc};

impl SkeletonEnv<'_> {
    pub(crate) fn ident_to_expr(&mut self, name: &str, scope: &BodyScope, span: Span) -> HirExpr {
        if name == "undefined" {
            return HirExpr::Undefined(span);
        }
        if let Some((id, ty)) = scope.lookup(name) {
            HirExpr::Local { id, ty, span }
        } else if self.tla_only_bindings.contains(name) {
            self.diagnostics.push(Diagnostic::error(
                TLA_ONLY_BINDING_CODE,
                format!(
                    "binding `{name}` is initialized inside `__ts_aot_tla_main` and is not \
                     visible to other functions or globals; move the let into a smaller scope \
                     (e.g. inside the function that uses it) or use a const literal initializer \
                     to make it module-level"
                ),
                span,
            ));
            let ty = self.error_ty();
            HirExpr::Global {
                name: Atom::from(name),
                ty,
                span,
            }
        } else {
            let ty = self.error_ty();
            HirExpr::Global {
                name: Atom::from(name),
                ty,
                span,
            }
        }
    }
}

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
                    let ty = senv.error_ty();
                    HirExpr::Global {
                        name: Atom::from("this"),
                        ty,
                        span: core_span_from_oxc(this_expr.span),
                    }
                }
            }
            Expression::ParenthesizedExpression(p) => self.walk_expr(senv, &p.expression, scope),
            Expression::BinaryExpression(b) => self.walk_binary(senv, b, scope),
            Expression::LogicalExpression(l) => self.walk_logical(senv, l, scope),
            Expression::UnaryExpression(unary) => self.walk_unary(senv, unary, scope),
            Expression::UpdateExpression(update) => self.walk_update(senv, update, scope),
            Expression::CallExpression(call) => self.walk_call(senv, call, scope),
            Expression::NewExpression(new_expr) => self.walk_new_expression(senv, new_expr, scope),
            other @ match_member_expression!(Expression) => {
                self.walk_member(senv, other.to_member_expression(), scope)
            }
            Expression::AssignmentExpression(a) => self.walk_assignment(senv, a, scope),
            Expression::AwaitExpression(a) => {
                let inner = self.walk_expr(senv, &a.argument, scope);
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
                self.walk_tagged_template_expression(senv, t, scope)
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
                let ty = senv.error_ty();
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
            senv.diagnostics.push(Diagnostic::error(
                "E0502",
                "async arrow functions are not supported in this PR; \
                 only synchronous no-capture arrow functions are accepted",
                core_span_from_oxc(arrow.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(arrow.span));
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
            senv.diagnostics.push(Diagnostic::error(
                "E0502",
                "rest parameters (...args) are not supported in arrow functions in this PR; \
                 use plain identifier parameters with explicit type annotations instead",
                core_span_from_oxc(arrow.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(arrow.span));
        }
        let mut params: Vec<HirParam> = Vec::with_capacity(arrow.params.items.len());
        let mut param_locals: Vec<(Atom, TypeId)> = Vec::with_capacity(arrow.params.items.len());
        for param in &arrow.params.items {
            if matches!(param.pattern, BindingPattern::AssignmentPattern(_)) {
                senv.diagnostics.push(Diagnostic::error(
                    "E0502",
                    "default-value arrow parameters (e.g. (x = 1) => ...) are not supported \
                     in this PR; use a plain identifier parameter with an explicit type \
                     annotation instead",
                    core_span_from_oxc(param.pattern.span()),
                ));
                return HirExpr::Unit(core_span_from_oxc(arrow.span));
            }
            let Some(name) = binding_pattern_name(&param.pattern) else {
                senv.diagnostics.push(Diagnostic::error(
                    "E0502",
                    "destructuring arrow parameters (e.g. ({x}) => ..., ([x]) => ...) are not \
                     supported in this PR; use a plain identifier parameter with an explicit \
                     type annotation instead",
                    core_span_from_oxc(param.pattern.span()),
                ));
                return HirExpr::Unit(core_span_from_oxc(arrow.span));
            };
            let name = Atom::from(name);
            if param.type_annotation.is_none() {
                senv.diagnostics.push(Diagnostic::error(
                    "E0502",
                    "arrow function parameters require an explicit type annotation in this PR; \
                     type inference for closure parameters is not yet implemented",
                    core_span_from_oxc(arrow.span),
                ));
                return HirExpr::Unit(core_span_from_oxc(arrow.span));
            }
            let ty = self.resolve_ts_type_from_annotation(senv, param.type_annotation.as_deref());
            params.push(HirParam {
                name: name.clone(),
                ty,
            });
            param_locals.push((name, ty));
        }
        if arrow.return_type.is_none() {
            senv.diagnostics.push(Diagnostic::error(
                "E0502",
                "arrow function return type must be annotated in this PR; \
                 type inference for closure return types is not yet implemented",
                core_span_from_oxc(arrow.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(arrow.span));
        }
        let ret_ty = self.resolve_ts_type_from_annotation(senv, arrow.return_type.as_deref());
        let mut outer_locals: std::collections::HashSet<String> =
            scope.names().into_iter().collect();
        outer_locals.insert("this".to_owned());
        let body: Vec<HirStmt> = if arrow.expression {
            let mut inner_scope = BodyScope::new(u32::try_from(params.len()).unwrap_or(u32::MAX));
            for (idx, (name, ty)) in param_locals.iter().enumerate() {
                let id = LocalId::from_raw(u32::try_from(idx).unwrap_or(u32::MAX));
                inner_scope.declare_param(name.as_str(), id, *ty);
            }
            let mut stmts = self.walk_stmts(senv, &arrow.body.statements, &mut inner_scope);
            if let Some(HirStmt::Expr { expr }) = stmts.first().cloned() {
                stmts[0] = HirStmt::Return { value: Some(expr) };
            }
            stmts
        } else {
            self.walk_function_body(senv, Some(&arrow.body), &params, false)
        };
        if exprs_reference_outer_local(&body, &outer_locals) {
            senv.diagnostics.push(Diagnostic::error(
                "P0005",
                "capturing closures are not supported in this PR; \
                 only no-capture arrow functions are accepted (outer local referenced from arrow body)",
                core_span_from_oxc(arrow.span),
            ));
            return HirExpr::Unit(core_span_from_oxc(arrow.span));
        }
        let param_types: Vec<TypeId> = params.iter().map(|p| p.ty).collect();
        let fn_ty = senv.types.intern(&Type::Fn {
            params: param_types,
            ret: ret_ty,
            err: None,
        });
        let id = LocalId::from_raw(self.next_closure_id);
        self.next_closure_id += 1;
        HirExpr::Closure {
            id,
            params,
            captures: Vec::new(),
            body,
            ty: fn_ty,
            span: core_span_from_oxc(arrow.span),
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
            senv.diagnostics.push(Diagnostic::error(
                "E0501",
                "`yield*` (delegating yield) is not supported yet — yield values directly instead",
                core_span_from_oxc(y.span),
            ));
            if let Some(a) = y.argument.as_ref() {
                self.walk_expr(senv, a, scope);
            }
            return HirExpr::Unit(core_span_from_oxc(y.span));
        }
        let inner = y.argument.as_ref().map(|a| self.walk_expr(senv, a, scope));
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
        let ty = senv.error_ty();
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
        let ty = senv.error_ty();
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
        let ty = senv.error_ty();
        HirExpr::Binary {
            op: map_logical_op(l.operator),
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
        let ty = senv.error_ty();
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
        let ty = senv.error_ty();
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
                let owner = self.walk_expr(senv, &s.object, scope);
                let ty = senv.error_ty();
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
                let index = self.walk_expr(senv, &computed.expression, scope);
                let ty = senv.error_ty();
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

    fn walk_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        a: &AssignmentExpression<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let target = self.walk_assign_target(senv, &a.left, scope);
        let rhs = self.walk_expr(senv, &a.right, scope);
        let ty = senv.error_ty();
        match compound_op(a.operator) {
            CompoundOp::Assign => HirExpr::Assignment {
                target: Box::new(target),
                value: Box::new(rhs),
                ty,
                span: core_span_from_oxc(a.span),
            },
            CompoundOp::Binary(op) => HirExpr::CompoundUpdate {
                target: Box::new(target),
                op,
                rhs: Box::new(rhs),
                post: false,
                ty,
                span: core_span_from_oxc(a.span),
            },
            CompoundOp::Unsupported => {
                senv.report_unwalked("assignment operator is not supported", a.span);
                HirExpr::Unit(core_span_from_oxc(a.span))
            }
        }
    }

    fn walk_assign_target(
        &mut self,
        senv: &mut SkeletonEnv,
        t: &oxc_ast::ast::AssignmentTarget<'_>,
        scope: &mut BodyScope,
    ) -> HirExpr {
        match t {
            t @ match_assignment_target!(AssignmentTarget) => {
                self.walk_simple_target(senv, t.to_simple_assignment_target(), scope)
            }
            t @ match_assignment_target_pattern!(AssignmentTarget) => {
                senv.report_unwalked("destructuring assignment target is not supported", t.span());
                HirExpr::Unit(core_span_from_oxc(t.span()))
            }
        }
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

fn exprs_reference_outer_local(
    stmts: &[HirStmt],
    outer_locals: &std::collections::HashSet<String>,
) -> bool {
    use ts_aot_ir_hir::{Visitor, walk_expr};
    struct CaptureSearch<'a> {
        outer_locals: &'a std::collections::HashSet<String>,
        found: bool,
    }
    impl Visitor for CaptureSearch<'_> {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if self.found {
                return;
            }
            if let HirExpr::Global { name, .. } = expr
                && self.outer_locals.contains(name.as_str())
            {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }
    let mut search = CaptureSearch {
        outer_locals,
        found: false,
    };
    for stmt in stmts {
        search.visit_stmt(stmt);
        if search.found {
            return true;
        }
    }
    false
}
