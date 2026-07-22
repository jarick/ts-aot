use oxc_ast::ast::{
    Class, Declaration, Expression, Function, MemberExpression, MethodDefinitionKind, TSType,
    TSTypeName, match_member_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{Atom, Diagnostic, GenericParamId, Span as CoreSpan, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirClass, HirEnumVariant, HirField, HirFunction, HirParam};

use crate::skeleton::SkeletonBuilder;
use crate::type_resolver::{TypeParamMap, resolve_simple_type};
use crate::util::{binding_pattern_name, core_span_from_oxc};

const TYPE_RESOLUTION_FAILURE_CODE: &str = "E0400";
const UNSUPPORTED_DECL_CODE: &str = "E0300";

impl SkeletonBuilder<'_, '_> {
    pub(crate) fn walk_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::FunctionDeclaration(f) => {
                let hir_fn = self.build_function(f, false);
                self.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Function(hir_fn));
            }
            Declaration::ClassDeclaration(c) => {
                let hir_class = self.build_class(c, false);
                self.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
            }
            Declaration::TSTypeAliasDeclaration(a) => {
                self.handle_type_alias(a);
            }
            Declaration::TSEnumDeclaration(e) => {
                self.handle_enum(e);
            }
            Declaration::TSInterfaceDeclaration(i) => {
                self.handle_interface(i);
            }
            Declaration::VariableDeclaration(v) => {
                self.handle_variable_declaration(v);
            }
            Declaration::TSModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => {
                self.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "declaration form is not supported by foundation pass",
                    decl.span(),
                );
            }
            Declaration::TSImportEqualsDeclaration(_) => {
                self.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "TS import-equals declarations are not supported by foundation pass",
                    decl.span(),
                );
            }
        }
    }

    pub(crate) fn handle_export_named_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::FunctionDeclaration(f) => {
                let name = Atom::from(f.id.as_ref().map_or("", |id| id.name.as_str()));
                let hir_fn = self.build_function(f, true);
                if f.id.is_none() {
                    self.report_unsupported(
                        UNSUPPORTED_DECL_CODE,
                        "exported function declaration must have a name",
                        decl.span(),
                    );
                }
                self.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Function(hir_fn));
                if !name.as_str().is_empty() {
                    self.record_export(name.as_str());
                }
            }
            Declaration::ClassDeclaration(c) => {
                let name = Atom::from(c.id.as_ref().map_or("", |id| id.name.as_str()));
                let hir_class = self.build_class(c, true);
                self.program
                    .push_decl(ts_aot_ir_hir::HirDecl::Class(hir_class));
                if !name.as_str().is_empty() {
                    self.record_export(name.as_str());
                }
            }
            Declaration::TSTypeAliasDeclaration(a) => {
                let name = a.id.name.as_str().to_string();
                self.handle_type_alias(a);
                self.record_export(&name);
            }
            Declaration::TSEnumDeclaration(e) => {
                let name = e.id.name.as_str().to_string();
                self.handle_enum(e);
                self.record_export(&name);
            }
            Declaration::TSInterfaceDeclaration(i) => {
                let name = i.id.name.as_str().to_string();
                self.handle_interface(i);
                self.record_export(&name);
            }
            Declaration::VariableDeclaration(v) => {
                for declarator in &v.declarations {
                    if let Some(ident) = binding_pattern_name(&declarator.id) {
                        self.record_export(ident.as_str());
                    }
                }
                self.handle_variable_declaration(v);
            }
            _ => self.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "exported declaration form is not supported",
                decl.span(),
            ),
        }
    }

    fn record_export(&mut self, name: &str) {
        self.program.exports.push(ts_aot_ir_hir::HirExport {
            name: Atom::from(name),
            alias: None,
        });
    }

    fn build_function(&mut self, func: &Function<'_>, is_exported: bool) -> HirFunction {
        let name = func
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));

        let (type_param_ids, type_param_map) = build_type_param_context(
            self.types,
            &mut self.next_generic_param,
            func.type_parameters.as_deref(),
        );

        let mut params = Vec::with_capacity(func.params.items.len());
        for param in &func.params.items {
            let param_name =
                binding_pattern_name(&param.pattern).map_or_else(|| Atom::from("_"), Atom::from);
            let param_ty = self.resolve_ts_type_from_annotation_with_params(
                param.type_annotation.as_deref(),
                Some(&type_param_map),
            );
            params.push(HirParam {
                name: param_name,
                ty: param_ty,
            });
        }

        let ret = self.resolve_ts_type_from_annotation_with_params(
            func.return_type.as_deref(),
            Some(&type_param_map),
        );

        let body = self.walk_function_body(func.body.as_deref(), &params, func.generator);

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

    pub(crate) fn build_class(&mut self, class: &Class<'_>, _is_exported: bool) -> HirClass {
        let name = class
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));

        let ty = self.types.intern(&Type::Error);

        let (class_type_param_ids, class_type_param_map) = build_type_param_context(
            self.types,
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
                    self.build_method(md, ty, &class_type_param_map)
                }
                _ => None,
            })
            .collect();

        let extends = class
            .super_class
            .as_ref()
            .and_then(|expr| self.resolve_superclass_name(expr));

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
        md: &oxc_ast::ast::MethodDefinition<'_>,
        class_ty: TypeId,
        class_type_param_map: &TypeParamMap,
    ) -> Option<HirFunction> {
        if md.r#static {
            self.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "static class methods are not supported by the foundation pass",
                md.span,
            );
            return None;
        }

        if md.kind == MethodDefinitionKind::Get || md.kind == MethodDefinitionKind::Set {
            self.report_unsupported(
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
            self.types,
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
                param.type_annotation.as_deref(),
                Some(&combined_map),
            );
            params.push(HirParam {
                name: param_name,
                ty: param_ty,
            });
        }
        let ret = self.resolve_ts_type_from_annotation_with_params(
            value.return_type.as_deref(),
            Some(&combined_map),
        );
        let body = self.walk_function_body(value.body.as_deref(), &params, value.generator);
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

    fn resolve_superclass_name(&mut self, expr: &Expression<'_>) -> Option<Atom> {
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

    fn handle_type_alias(&mut self, a: &oxc_ast::ast::TSTypeAliasDeclaration<'_>) {
        let name = Atom::from(a.id.name.as_str());
        let target = self.resolve_ts_type(Some(&a.type_annotation));
        self.program
            .push_decl(ts_aot_ir_hir::HirDecl::TypeAlias { name, target });
    }

    fn handle_enum(&mut self, e: &oxc_ast::ast::TSEnumDeclaration<'_>) {
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
        self.program
            .push_decl(ts_aot_ir_hir::HirDecl::Enum { name, variants });
    }

    fn handle_interface(&mut self, i: &oxc_ast::ast::TSInterfaceDeclaration<'_>) {
        let name = Atom::from(i.id.name.as_str());
        self.program
            .push_decl(ts_aot_ir_hir::HirDecl::Interface { name });
    }

    fn handle_variable_declaration(&mut self, v: &oxc_ast::ast::VariableDeclaration<'_>) {
        for declarator in &v.declarations {
            let Some(ident) = binding_pattern_name(&declarator.id) else {
                self.report_unsupported(
                    UNSUPPORTED_DECL_CODE,
                    "variable declaration with destructuring is not supported in foundation",
                    v.span,
                );
                continue;
            };
            let name = Atom::from(ident.as_str());
            let ty = self.resolve_ts_type_from_annotation(declarator.type_annotation.as_deref());
            self.program.push_decl(ts_aot_ir_hir::HirDecl::Global {
                name,
                ty,
                init: None,
            });
        }
    }

    pub(crate) fn resolve_ts_type(&mut self, ty: Option<&oxc_ast::ast::TSType<'_>>) -> TypeId {
        self.resolve_ts_type_with_params(ty, None)
    }

    pub(crate) fn resolve_ts_type_from_annotation(
        &mut self,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
    ) -> TypeId {
        self.resolve_ts_type(ann.map(|a| &a.type_annotation))
    }

    pub(crate) fn resolve_ts_type_with_params(
        &mut self,
        ty: Option<&TSType<'_>>,
        type_params: Option<&TypeParamMap>,
    ) -> TypeId {
        if let Some(ts_type) = ty
            && let Some(name) = banned_type_name(ts_type)
        {
            let span = core_span_from_oxc(ts_type.span());
            self.diagnostics.push(Diagnostic::error(
                "E0401",
                format!(
                    "the type `{name}` is not supported in strict AOT mode. \
                     Use explicit types like `i64`, `string`, or a named struct instead.",
                ),
                span,
            ));
            return self.types.intern(&Type::Error);
        }
        if let Some(id) =
            resolve_simple_type(ty, self.types, Some(&self.resolved_aliases), type_params)
        {
            id
        } else {
            let span = ty.map_or_else(
                || CoreSpan::new(0, u32::try_from(self.source.len()).unwrap_or(u32::MAX)),
                |t| core_span_from_oxc(t.span()),
            );
            self.diagnostics.push(Diagnostic::warning(
                TYPE_RESOLUTION_FAILURE_CODE,
                "could not resolve type annotation",
                span,
            ));
            self.types.intern(&Type::Error)
        }
    }

    pub(crate) fn resolve_ts_type_from_annotation_with_params(
        &mut self,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        type_params: Option<&TypeParamMap>,
    ) -> TypeId {
        self.resolve_ts_type_with_params(ann.map(|a| &a.type_annotation), type_params)
    }

    pub(crate) fn report_unsupported(
        &mut self,
        code: &'static str,
        message: &str,
        span: oxc_span::Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error(code, message, core_span_from_oxc(span)));
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
