use oxc_ast::ast::{
    Class, Declaration, Expression, Function, MemberExpression, MethodDefinitionKind, TSType,
    TSTypeName, match_member_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{Atom, Diagnostic, GenericParamId, Span as CoreSpan, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirClass, HirDecl, HirEnumVariant, HirExpr, HirField, HirFunction, HirParam, HirStmt,
};

use crate::skeleton::{SkeletonBuilder, SkeletonEnv, TlaMainItem};
use crate::type_resolver::{TypeParamMap, resolve_simple_type};
use crate::util::{binding_pattern_name, core_span_from_oxc};

const TYPE_RESOLUTION_FAILURE_CODE: &str = "E0400";
const UNSUPPORTED_DECL_CODE: &str = "E0300";

impl SkeletonEnv<'_> {
    pub(crate) fn report_unsupported(
        &mut self,
        code: &'static str,
        message: &str,
        span: oxc_span::Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, core_span_from_oxc(span)));
    }

    pub(crate) fn resolve_superclass_name(&mut self, expr: &Expression<'_>) -> Option<Atom> {
        match expr {
            Expression::Identifier(id) => Some(Atom::from(id.name.as_str())),
            match_member_expression!(Expression) => {
                if let Some(s) = expr.as_member_expression().and_then(|m| match m {
                    MemberExpression::StaticMemberExpression(s) => Some(s),
                    _ => None,
                }) {
                    Some(Atom::from(s.property.name.as_str()))
                } else {
                    self.report_unsupported(
                        UNSUPPORTED_DECL_CODE,
                        "extends must be an identifier or member access expression",
                        expr.span(),
                    );
                    None
                }
            }
            other => {
                self.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "extends must be an identifier or member access expression",
                    other.span(),
                );
                None
            }
        }
    }

    pub(crate) fn handle_enum(&mut self, e: &oxc_ast::ast::TSEnumDeclaration<'_>) {
        let name = Atom::from(e.id.name.as_str());
        let variants = e
            .body
            .members
            .iter()
            .map(|m| HirEnumVariant {
                name: match &m.id {
                    oxc_ast::ast::TSEnumMemberName::Identifier(ident) => {
                        Atom::from(ident.name.as_str())
                    }
                    oxc_ast::ast::TSEnumMemberName::String(lit) => Atom::from(lit.value.as_str()),
                    oxc_ast::ast::TSEnumMemberName::ComputedString(_)
                    | oxc_ast::ast::TSEnumMemberName::ComputedTemplateString(_) => Atom::from(""),
                },
                value: None,
            })
            .collect();
        self.program.push_decl(HirDecl::Enum { name, variants });
    }

    pub(crate) fn handle_interface(&mut self, i: &oxc_ast::ast::TSInterfaceDeclaration<'_>) {
        let name = Atom::from(i.id.name.as_str());
        self.program.push_decl(HirDecl::Interface { name });
    }
}

impl SkeletonBuilder {
    pub(crate) fn walk_declaration(&mut self, senv: &mut SkeletonEnv, decl: &Declaration<'_>) {
        match decl {
            Declaration::FunctionDeclaration(f) => {
                let span = CoreSpan::new(f.span.start, f.span.end);
                let hir_fn = self.build_function(senv, f, false);
                self.top_level_function_spans
                    .insert(hir_fn.name.clone(), span);
                senv.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Function(hir_fn));
            }
            Declaration::ClassDeclaration(c) => {
                let hir_class = self.build_class(senv, c, false);
                senv.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
            }
            Declaration::TSTypeAliasDeclaration(a) => {
                self.handle_type_alias(senv, a);
            }
            Declaration::TSEnumDeclaration(e) => {
                senv.handle_enum(e);
            }
            Declaration::TSInterfaceDeclaration(i) => {
                senv.handle_interface(i);
            }
            Declaration::VariableDeclaration(v) => {
                self.handle_variable_declaration(senv, v);
            }
            Declaration::TSModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => {
                senv.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "declaration form is not supported by foundation pass",
                    decl.span(),
                );
            }
            Declaration::TSImportEqualsDeclaration(_) => {
                senv.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "TS import-equals declarations are not supported by foundation pass",
                    decl.span(),
                );
            }
        }
    }

    pub(crate) fn handle_export_named_declaration(
        &mut self,
        senv: &mut SkeletonEnv,
        decl: &Declaration<'_>,
    ) {
        match decl {
            Declaration::FunctionDeclaration(f) => {
                let name = Atom::from(f.id.as_ref().map_or("", |id| id.name.as_str()));
                let hir_fn = self.build_function(senv, f, true);
                if f.id.is_none() {
                    senv.report_unsupported(
                        UNSUPPORTED_DECL_CODE,
                        "exported function declaration must have a name",
                        decl.span(),
                    );
                }
                senv.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Function(hir_fn));
                if !name.as_str().is_empty() {
                    senv.record_export(name.as_str());
                }
            }
            Declaration::ClassDeclaration(c) => {
                let name = Atom::from(c.id.as_ref().map_or("", |id| id.name.as_str()));
                let hir_class = self.build_class(senv, c, true);
                senv.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
                if !name.as_str().is_empty() {
                    senv.record_export(name.as_str());
                }
            }
            Declaration::TSTypeAliasDeclaration(a) => {
                let name = a.id.name.as_str().to_string();
                self.handle_type_alias(senv, a);
                senv.record_export(&name);
            }
            Declaration::TSEnumDeclaration(e) => {
                let name = e.id.name.as_str().to_string();
                senv.handle_enum(e);
                senv.record_export(&name);
            }
            Declaration::TSInterfaceDeclaration(i) => {
                let name = i.id.name.as_str().to_string();
                senv.handle_interface(i);
                senv.record_export(&name);
            }
            Declaration::VariableDeclaration(v) => {
                for declarator in &v.declarations {
                    if let Some(ident) = binding_pattern_name(&declarator.id) {
                        senv.record_export(ident.as_str());
                    }
                }
                self.handle_variable_declaration(senv, v);
            }
            _ => senv.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "exported declaration form is not supported",
                decl.span(),
            ),
        }
    }

    fn build_function(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &Function<'_>,
        is_exported: bool,
    ) -> HirFunction {
        let name = func
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));

        let (type_param_ids, type_param_map) = build_type_param_context(
            senv.types,
            &mut self.next_generic_param,
            func.type_parameters.as_deref(),
        );

        let mut params = Vec::with_capacity(func.params.items.len());
        for param in &func.params.items {
            let param_name =
                binding_pattern_name(&param.pattern).map_or_else(|| Atom::from("_"), Atom::from);
            let param_ty = self.resolve_ts_type_from_annotation_with_params(
                senv,
                param.type_annotation.as_deref(),
                Some(&type_param_map),
            );
            params.push(HirParam {
                name: param_name,
                ty: param_ty,
            });
        }

        let ret = self.resolve_ts_type_from_annotation_with_params(
            senv,
            func.return_type.as_deref(),
            Some(&type_param_map),
        );

        self.type_param_stack.push(type_param_map.clone());
        let body = self.walk_function_body(senv, func.body.as_deref(), &params, func.generator);
        self.type_param_stack.pop();

        HirFunction {
            name,
            params,
            ret,
            throws: None,
            body,
            is_async: func.r#async,
            is_generator: func.generator,
            is_exported,
            type_params: type_param_ids,
            async_info: None,
        }
    }

    pub(crate) fn build_class(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        _is_exported: bool,
    ) -> HirClass {
        let name = class
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));

        let ty = senv.types.intern(&Type::Error);

        let (class_type_param_ids, class_type_param_map) = build_type_param_context(
            senv.types,
            &mut self.next_generic_param,
            class.type_parameters.as_deref(),
        );

        let fields = class
            .body
            .body
            .iter()
            .filter_map(|m| match m {
                oxc_ast::ast::ClassElement::PropertyDefinition(p) => {
                    let field_name = p
                        .key
                        .static_name()
                        .map_or_else(|| Atom::from(""), |n| Atom::from(n.as_ref()));
                    let field_ty = self.resolve_ts_type_from_annotation_with_params(
                        senv,
                        p.type_annotation.as_deref(),
                        Some(&class_type_param_map),
                    );
                    Some(HirField {
                        name: field_name,
                        ty: field_ty,
                    })
                }
                _ => None,
            })
            .collect();

        let methods = class
            .body
            .body
            .iter()
            .filter_map(|m| match m {
                oxc_ast::ast::ClassElement::MethodDefinition(md) => {
                    self.build_method(senv, md, ty, &class_type_param_map)
                }
                _ => None,
            })
            .collect();

        let extends = class
            .super_class
            .as_ref()
            .and_then(|expr| senv.resolve_superclass_name(expr));

        HirClass {
            name,
            ty,
            fields,
            methods,
            extends,
            type_params: class_type_param_ids,
        }
    }

    fn build_method(
        &mut self,
        senv: &mut SkeletonEnv,
        md: &oxc_ast::ast::MethodDefinition<'_>,
        class_ty: TypeId,
        class_type_param_map: &TypeParamMap,
    ) -> Option<HirFunction> {
        if md.r#static {
            senv.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "static class methods are not supported by the foundation pass",
                md.span,
            );
            return None;
        }

        if md.kind == MethodDefinitionKind::Get || md.kind == MethodDefinitionKind::Set {
            senv.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "accessor class methods (get/set) are not supported by the foundation pass",
                md.span,
            );
            return None;
        }

        let value = &*md.value;
        let method_name = md
            .key
            .static_name()
            .map_or_else(|| Atom::from(""), |n| Atom::from(n.as_ref()));

        let (method_type_param_ids, method_param_map) = build_type_param_context(
            senv.types,
            &mut self.next_generic_param,
            value.type_parameters.as_deref(),
        );
        let mut combined_map = TypeParamMap::new();
        for (k, v) in class_type_param_map
            .iter_bindings()
            .chain(method_param_map.iter_bindings())
        {
            combined_map.bind(k, v);
        }

        let mut params = Vec::with_capacity(value.params.items.len() + 1);
        let needs_synthetic_this = matches!(
            md.kind,
            MethodDefinitionKind::Method | MethodDefinitionKind::Constructor
        );
        if needs_synthetic_this {
            params.push(HirParam {
                name: Atom::from("this"),
                ty: class_ty,
            });
        }
        for param in &value.params.items {
            let param_name =
                binding_pattern_name(&param.pattern).map_or_else(|| Atom::from("_"), Atom::from);
            let param_ty = self.resolve_ts_type_from_annotation_with_params(
                senv,
                param.type_annotation.as_deref(),
                Some(&combined_map),
            );
            params.push(HirParam {
                name: param_name,
                ty: param_ty,
            });
        }
        let ret = self.resolve_ts_type_from_annotation_with_params(
            senv,
            value.return_type.as_deref(),
            Some(&combined_map),
        );
        self.type_param_stack.push(combined_map.clone());
        let body = self.walk_function_body(senv, value.body.as_deref(), &params, value.generator);
        self.type_param_stack.pop();
        Some(HirFunction {
            name: method_name,
            params,
            ret,
            throws: None,
            body,
            is_async: value.r#async,
            is_generator: value.generator,
            is_exported: false,
            type_params: method_type_param_ids,
            async_info: None,
        })
    }

    fn handle_type_alias(
        &mut self,
        senv: &mut SkeletonEnv,
        a: &oxc_ast::ast::TSTypeAliasDeclaration<'_>,
    ) {
        let name = Atom::from(a.id.name.as_str());
        let target = self.resolve_ts_type(senv, Some(&a.type_annotation));
        senv.program
            .push_decl(ts_aot_ir_hir::HirDecl::TypeAlias { name, target });
    }

    fn handle_variable_declaration(
        &mut self,
        senv: &mut SkeletonEnv,
        v: &oxc_ast::ast::VariableDeclaration<'_>,
    ) {
        for declarator in &v.declarations {
            let Some(ident) = binding_pattern_name(&declarator.id) else {
                senv.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "variable declaration with destructuring is not supported in foundation",
                    v.span,
                );
                continue;
            };
            let name = Atom::from(ident.as_str());
            let ty =
                self.resolve_ts_type_from_annotation(senv, declarator.type_annotation.as_deref());
            let init = declarator
                .init
                .as_ref()
                .map(|init_expr| self.walk_top_level_init(senv, init_expr));
            self.emit_top_level_let(senv, name, ty, init, v.span);
        }
    }

    fn emit_top_level_let(
        &mut self,
        senv: &mut SkeletonEnv,
        name: Atom,
        ty: TypeId,
        init: Option<HirExpr>,
        span: oxc_span::Span,
    ) {
        if let Some(init_expr) = init {
            if is_const_initializer(&init_expr) {
                senv.tla_only_bindings.remove(name.as_str());
                senv.program.push_decl(HirDecl::Global {
                    name,
                    ty,
                    init: Some(init_expr),
                });
            } else if self.module {
                let id = self.tla_top_level_scope.declare(name.as_str(), ty);
                let stmt = HirStmt::Let {
                    id,
                    name: name.clone(),
                    ty,
                    init: Some(init_expr),
                };
                self.record_tla_main_let_stmt(stmt);
            } else {
                senv.diagnostics.push(Diagnostic::error(
                    UNSUPPORTED_DECL_CODE,
                    format!(
                        "top-level `let {name} = ...` with a non-constant initializer is not \
                         supported in script mode. Wrap the source as a module (e.g. rename \
                         to .mts or pass --module) so the initializer runs inside __tla_main, \
                         or use a const literal initializer (number, bool)."
                    ),
                    core_span_from_oxc(span),
                ));
            }
        } else {
            senv.tla_only_bindings.remove(name.as_str());
            senv.program.push_decl(HirDecl::Global {
                name,
                ty,
                init: None,
            });
        }
    }

    fn record_tla_main_let_stmt(&mut self, stmt: HirStmt) {
        self.tla_main_body.push(TlaMainItem::Let(stmt));
    }

    pub(crate) fn resolve_ts_type(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: Option<&oxc_ast::ast::TSType<'_>>,
    ) -> TypeId {
        self.resolve_ts_type_with_params(senv, ty, None)
    }

    pub(crate) fn resolve_ts_type_from_annotation(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
    ) -> TypeId {
        self.resolve_ts_type(senv, ann.map(|a| &a.type_annotation))
    }

    pub(crate) fn resolve_ts_type_with_params(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: Option<&TSType<'_>>,
        type_params: Option<&TypeParamMap>,
    ) -> TypeId {
        if let Some(ts_type) = ty
            && let Some(name) = banned_type_name(ts_type)
        {
            let span = core_span_from_oxc(ts_type.span());
            senv.diagnostics.push(Diagnostic::error(
                "E0401",
                format!(
                    "the type `{name}` is not supported in strict AOT mode. \
                     Use explicit types like `i64`, `string`, or a named struct instead.",
                ),
                span,
            ));
            return senv.types.intern(&Type::Error);
        }
        if let Some(id) = resolve_simple_type(
            ty,
            senv.types,
            Some(&self.resolved_aliases),
            type_params,
            Some(&mut senv.diagnostics),
        ) {
            id
        } else {
            let span = ty.map_or_else(
                || CoreSpan::new(0, self.source_len),
                |t| core_span_from_oxc(t.span()),
            );
            senv.diagnostics.push(Diagnostic::warning(
                TYPE_RESOLUTION_FAILURE_CODE,
                "could not resolve type annotation",
                span,
            ));
            senv.types.intern(&Type::Error)
        }
    }

    pub(crate) fn resolve_ts_type_from_annotation_with_params(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        type_params: Option<&TypeParamMap>,
    ) -> TypeId {
        self.resolve_ts_type_with_params(senv, ann.map(|a| &a.type_annotation), type_params)
    }
}

fn build_type_param_context(
    types: &mut TypeTable,
    next_id: &mut u32,
    params: Option<&oxc_ast::ast::TSTypeParameterDeclaration<'_>>,
) -> (Vec<GenericParamId>, TypeParamMap) {
    let mut ids = Vec::new();
    let mut map = TypeParamMap::new();
    let Some(params) = params else {
        return (ids, map);
    };
    for p in &params.params {
        let id = GenericParamId::from_raw(*next_id);
        *next_id = next_id.saturating_add(1);
        let type_id = types.intern(&Type::GenericParam { id });
        map.bind(p.name.name.as_str(), type_id);
        ids.push(id);
    }
    (ids, map)
}

fn banned_type_name(ty: &TSType<'_>) -> Option<&'static str> {
    match ty {
        TSType::TSAnyKeyword(_) => Some("any"),
        TSType::TSUnknownKeyword(_) => Some("unknown"),
        TSType::TSTypeReference(r) => {
            if let TSTypeName::IdentifierReference(id) = &r.type_name
                && id.name.as_str() == "Object"
            {
                return Some("Object");
            }
            r.type_arguments
                .as_ref()
                .and_then(|args| args.params.iter().find_map(banned_type_name))
        }
        TSType::TSArrayType(element) => banned_type_name(&element.element_type),
        _ => None,
    }
}

fn is_const_initializer(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Unit(_)
            | HirExpr::Bool(_, _)
            | HirExpr::Int(_, _)
            | HirExpr::Float(_, _)
            | HirExpr::Null(_)
    )
}

pub(crate) fn is_const_initializer_ast(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::BooleanLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        _ => false,
    }
}
