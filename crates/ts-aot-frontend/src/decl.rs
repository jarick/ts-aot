use std::collections::HashSet;

use oxc_ast::ast::{
    Class, Declaration, Expression, Function, MemberExpression, MethodDefinitionKind, Program,
    TSType, TSTypeName, match_member_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{
    Atom, Diagnostic, GenericParamId, LocalId, Span as CoreSpan, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirClass, HirDecl, HirEnumVariant, HirExpr, HirField, HirFunction, HirParam, HirStmt,
};

use crate::body::FunctionBodyInput;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv, TlaMainItem};
use crate::type_resolver::reference::TypeLookup;
use crate::type_resolver::{TypeParamMap, resolve_simple_type};
use crate::util::{binding_pattern_name, core_span_from_oxc};

const TYPE_RESOLUTION_FAILURE_CODE: &str = "E0400";

/// A function/method without a return type annotation whose body does not
/// determine the return type.
pub(crate) const RETURN_TYPE_INFERENCE_CODE: &str = "E0415";
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

    pub(crate) fn report_unsupported_warning(
        &mut self,
        code: &'static str,
        message: &str,
        span: oxc_span::Span,
    ) {
        self.diagnostics
            .push(Diagnostic::warning(code, message, core_span_from_oxc(span)));
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
                    self.report_unsupported_warning(
                        UNSUPPORTED_DECL_CODE,
                        "extends must be an identifier or static member access expression; \
                         the class is lowered without its parent",
                        expr.span(),
                    );
                    None
                }
            }
            other => {
                self.report_unsupported_warning(
                    UNSUPPORTED_DECL_CODE,
                    "extends must be an identifier or static member access expression; \
                     the class is lowered without its parent",
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
                let hir_class = self.build_class(senv, c, true);
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

        let strict = self.strict || crate::util::body_has_use_strict(func.body.as_deref());
        crate::util::validate_unique_param_bindings(senv, &func.params, strict);
        let prev_strict = self.strict;
        self.strict = strict;
        let (params, extra_locals) =
            self.build_params_and_locals(senv, &func.params.items, 0, Some(&type_param_map));

        let ret = if func.return_type.is_some() {
            if func.generator {
                self.resolve_generator_ret_ty(
                    senv,
                    func.return_type.as_deref(),
                    Some(&type_param_map),
                    func.r#async,
                )
            } else {
                self.resolve_ts_type_from_annotation_with_params(
                    senv,
                    func.return_type.as_deref(),
                    Some(&type_param_map),
                )
            }
        } else {
            senv.error_ty()
        };

        self.type_param_stack.push(type_param_map.clone());
        let (body, _) = self.walk_function_body(
            senv,
            &FunctionBodyInput {
                body: func.body.as_deref(),
                params: &params,
                extra_locals: &extra_locals,
                items: &func.params.items,
                item_offset: 0,
                param_base: 0,
            },
            func.generator,
            false,
        );
        self.type_param_stack.pop();
        self.strict = prev_strict;

        let ret = if func.return_type.is_none() {
            Self::infer_or_report_return_ty(
                &body,
                core_span_from_oxc(func.span()),
                func.r#async,
                func.generator,
                senv,
            )
        } else {
            ret
        };

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
        reuse: bool,
    ) -> HirClass {
        let name = class
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));

        let ty = if reuse && let Some(pre_ty) = self.class_types.get(name.as_str()).copied() {
            pre_ty
        } else {
            let struct_id = ts_aot_core::StructId::from_raw(self.next_class_struct_id);
            self.next_class_struct_id = self.next_class_struct_id.saturating_add(1);
            senv.types.intern(&Type::Struct { id: struct_id })
        };
        let prior_binding = self.class_types.insert(name.as_str().to_owned(), ty);

        let (class_type_param_ids, class_type_param_map) = build_type_param_context(
            senv.types,
            &mut self.next_generic_param,
            class.type_parameters.as_deref(),
        );

        let already_processed = senv.program.class_field_inits.contains_key(&ty);
        let fields: Vec<HirField> =
            if let Some(cached) = senv.program.class_fields.get(&ty).cloned() {
                cached
            } else {
                let computed = self.collect_class_fields(senv, class, Some(&class_type_param_map));
                senv.program.class_fields.insert(ty, computed.clone());
                computed
            };
        let mut field_inits: Vec<(Atom, HirExpr)> = Vec::new();
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::PropertyDefinition(p) = element else {
                continue;
            };
            let field_name = p
                .key
                .static_name()
                .map_or_else(|| Atom::from(""), |n| Atom::from(n.as_ref()));
            let Some(value) = &p.value else {
                continue;
            };
            let span = core_span_from_oxc(value.span());
            let field_ty = fields.iter().find(|f| f.name == field_name).map(|f| f.ty);
            let init = match value {
                Expression::NumericLiteral(n) => {
                    if field_ty.is_some_and(|t| {
                        matches!(senv.types.resolve(t), Some(Type::F64 | Type::F32))
                    }) {
                        Some(HirExpr::Float(n.value.to_bits(), span))
                    } else {
                        Some(crate::ops::number_to_hir(n.value, span))
                    }
                }
                Expression::BooleanLiteral(b) => Some(HirExpr::Bool(b.value, span)),
                Expression::StringLiteral(s) => {
                    Some(HirExpr::String(Atom::from(s.value.as_str()), span))
                }
                _ => None,
            };
            match init {
                Some(init) => field_inits.push((field_name.clone(), init)),
                None => {
                    if !already_processed {
                        senv.diagnostics.push(Diagnostic::warning(
                            "E0406",
                            format!(
                                "class field initializer for `{}` is not supported in AOT \
                                 (only literal number/string/bool initializers are); the \
                                 field will default to its zero value",
                                field_name.as_str()
                            ),
                            span,
                        ));
                    }
                }
            }
        }
        if !already_processed {
            senv.program.class_field_inits.insert(ty, field_inits);
        }

        let mut methods = Vec::new();
        let mut accessor_names = HashSet::new();
        let mut ordinary_names = HashSet::new();
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::MethodDefinition(md) = element else {
                continue;
            };
            let is_accessor = matches!(
                md.kind,
                MethodDefinitionKind::Get | MethodDefinitionKind::Set
            );
            let func = self.build_method(senv, md, ty, &class_type_param_map);
            if is_accessor {
                accessor_names.insert(func.name.clone());
            } else {
                ordinary_names.insert(func.name.clone());
            }
            methods.push(func);
        }
        for colliding in accessor_names.intersection(&ordinary_names) {
            senv.diagnostics.push(Diagnostic::error(
                UNSUPPORTED_DECL_CODE,
                format!(
                    "class method `{name}` collides with the lowered HIR name of an accessor \
                     (`get`/`set`); rename the method or the accessor so their names stay distinct",
                    name = colliding.as_str()
                ),
                core_span_from_oxc(class.span),
            ));
        }

        let extends = class
            .super_class
            .as_ref()
            .and_then(|expr| senv.resolve_superclass_name(expr));

        match prior_binding {
            Some(prior) => {
                self.class_types.insert(name.as_str().to_owned(), prior);
            }
            None => {
                self.class_types.remove(name.as_str());
            }
        }

        HirClass {
            name,
            ty,
            pre_pass_ty: None,
            fields,
            methods,
            extends,
            type_params: class_type_param_ids,
        }
    }

    fn collect_class_fields(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        type_params: Option<&TypeParamMap>,
    ) -> Vec<HirField> {
        class
            .body
            .body
            .iter()
            .filter_map(|m| {
                let oxc_ast::ast::ClassElement::PropertyDefinition(p) = m else {
                    return None;
                };
                let name = p
                    .key
                    .static_name()
                    .map_or_else(|| Atom::from(""), |n| Atom::from(n.as_ref()));
                let mut ty = self.resolve_ts_type_from_annotation_with_params(
                    senv,
                    p.type_annotation.as_deref(),
                    type_params,
                );
                if p.type_annotation.is_none()
                    && matches!(senv.types.resolve(ty), None | Some(Type::Error))
                    && let Some(value) = &p.value
                    && let Some(init) = literal_value_hir(value, core_span_from_oxc(value.span()))
                    && let Some(inferred) = crate::expr::literal_hir_ty(senv.types, &init)
                {
                    ty = inferred;
                }
                Some(HirField { name, ty })
            })
            .collect()
    }

    pub(crate) fn pre_register_class_fields(
        &mut self,
        senv: &mut SkeletonEnv,
        program: &Program<'_>,
    ) {
        for stmt in &program.body {
            let class_opt: Option<&Class<'_>> = if let Some(decl) = stmt.as_declaration() {
                if let Declaration::ClassDeclaration(c) = decl {
                    Some(c)
                } else {
                    None
                }
            } else if let Some(m) = stmt.as_module_declaration() {
                if let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                    && let Some(Declaration::ClassDeclaration(c)) = e.declaration.as_ref()
                {
                    Some(c)
                } else {
                    None
                }
            } else {
                None
            };
            let Some(class) = class_opt else {
                continue;
            };
            let Some(id) = class.id.as_ref() else {
                continue;
            };
            if class
                .type_parameters
                .as_ref()
                .is_some_and(|p| !p.params.is_empty())
            {
                continue;
            }
            let Some(ty) = self.class_types.get(id.name.as_str()).copied() else {
                continue;
            };
            if senv.program.class_fields.contains_key(&ty) {
                continue;
            }
            let fields = self.collect_class_fields(senv, class, None);
            senv.program.class_fields.insert(ty, fields);
        }
    }

    fn build_method(
        &mut self,
        senv: &mut SkeletonEnv,
        md: &oxc_ast::ast::MethodDefinition<'_>,
        class_ty: TypeId,
        class_type_param_map: &TypeParamMap,
    ) -> HirFunction {
        let value = &*md.value;
        let base_name = md
            .key
            .static_name()
            .map_or_else(|| Atom::from(""), |n| Atom::from(n.as_ref()));
        let method_name = match md.kind {
            MethodDefinitionKind::Get => Atom::from(format!("__get_{base_name}")),
            MethodDefinitionKind::Set => Atom::from(format!("__set_{base_name}")),
            _ => base_name,
        };

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

        let needs_synthetic_this = !md.r#static;
        let strict = true;
        crate::util::validate_unique_param_bindings(senv, &value.params, strict);
        let prev_strict = self.strict;
        self.strict = strict;
        let (method_params, extra_locals) = self.build_params_and_locals(
            senv,
            &value.params.items,
            u32::from(needs_synthetic_this),
            Some(&combined_map),
        );
        let mut params = Vec::with_capacity(method_params.len() + 1);
        if needs_synthetic_this {
            params.push(HirParam {
                id: LocalId::from_raw(0),
                name: Atom::from("this"),
                ty: class_ty,
                optional: false,
            });
        }
        params.extend(method_params);
        let ret = if value.return_type.is_some() {
            if value.generator {
                self.resolve_generator_ret_ty(
                    senv,
                    value.return_type.as_deref(),
                    Some(&combined_map),
                    value.r#async,
                )
            } else {
                self.resolve_ts_type_from_annotation_with_params(
                    senv,
                    value.return_type.as_deref(),
                    Some(&combined_map),
                )
            }
        } else {
            senv.error_ty()
        };
        self.type_param_stack.push(combined_map.clone());
        let saved_static = self.in_static_method;
        self.in_static_method = md.r#static;
        let (body, _) = self.walk_function_body(
            senv,
            &FunctionBodyInput {
                body: value.body.as_deref(),
                params: &params,
                extra_locals: &extra_locals,
                items: &value.params.items,
                item_offset: usize::from(needs_synthetic_this),
                param_base: 0,
            },
            value.generator,
            true,
        );
        self.in_static_method = saved_static;
        self.type_param_stack.pop();
        self.strict = prev_strict;

        let ret = if value.return_type.is_none() {
            Self::infer_or_report_return_ty(
                &body,
                core_span_from_oxc(value.span()),
                value.r#async,
                value.generator,
                senv,
            )
        } else {
            ret
        };

        HirFunction {
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
        }
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
            let mut ty = if declarator.type_annotation.is_some() {
                self.resolve_ts_type_from_annotation(senv, declarator.type_annotation.as_deref())
            } else {
                senv.error_ty()
            };
            let init = declarator
                .init
                .as_ref()
                .map(|init_expr| self.walk_top_level_init(senv, init_expr));
            if declarator.type_annotation.is_none()
                && matches!(senv.types.resolve(ty), None | Some(Type::Error))
            {
                let inferred = init.as_ref().and_then(|e| match e {
                    HirExpr::Int(..) => Some(senv.types.intern(&Type::I64)),
                    HirExpr::Float(..) => Some(senv.types.intern(&Type::F64)),
                    HirExpr::String(..) => Some(senv.types.intern(&Type::String)),
                    HirExpr::Bool(..) => Some(senv.types.intern(&Type::Bool)),
                    _ => e
                        .has_static_type()
                        .then(|| e.ty())
                        .filter(|t| !matches!(senv.types.resolve(*t), None | Some(Type::Error))),
                });
                if let Some(t) = inferred {
                    ty = t;
                }
            }
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
                senv.program.push_decl(HirDecl::Global {
                    name,
                    ty,
                    init: Some(init_expr),
                });
            } else if self.module {
                senv.program.push_decl(HirDecl::Global {
                    name: name.clone(),
                    ty,
                    init: None,
                });
                let span = core_span_from_oxc(span);
                let assign = HirExpr::Assignment {
                    target: Box::new(HirExpr::Global { name, ty, span }),
                    value: Box::new(init_expr),
                    ty,
                    span,
                };
                self.tla_main_body.push(TlaMainItem::Expr(assign));
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
            senv.program.push_decl(HirDecl::Global {
                name,
                ty,
                init: None,
            });
        }
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

    /// A generator's annotation is its **yield** type in this dialect
    /// (`function* g(): i64 { yield 1; }`), but the standard TypeScript
    /// spellings `Generator<T>` / `IterableIterator<T>` / `Iterator<T>` (and
    /// the `Async*` variants) are accepted and unwrapped to `T` - otherwise the
    /// annotation resolves to an unknown type and the generator is lowered with
    /// an error yield type.
    fn resolve_generator_ret_ty(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        type_params: Option<&TypeParamMap>,
        is_async: bool,
    ) -> TypeId {
        let inner = ann.map(|a| &a.type_annotation);
        if let Some(TSType::TSTypeReference(reference)) = inner
            && let TSTypeName::IdentifierReference(id) = &reference.type_name
            && is_generator_wrapper(id.name.as_str(), is_async)
            && let Some(arguments) = &reference.type_arguments
            && let Some(yield_ty) = arguments.params.first()
        {
            return self.resolve_ts_type_with_params(senv, Some(yield_ty), type_params);
        }
        self.resolve_ts_type_with_params(senv, inner, type_params)
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
            // `never` is uninhabited (no runtime value can exist), which is a
            // different reason from the "no `any`/`unknown`" rule and worth
            // spelling out.
            let detail = if name == "never" {
                ": no value of that type exists, so nothing can be stored, passed or returned. \
                 Drop the annotation (a function that always throws can return `void`) or use a \
                 concrete type."
            } else {
                ". Use explicit types like `i64`, `string`, or a named struct instead."
            };
            senv.diagnostics.push(Diagnostic::error(
                "E0401",
                format!("the type `{name}` is not supported in strict AOT mode{detail}"),
                span,
            ));
            return senv.types.intern(&Type::Error);
        }
        if let Some(id) = {
            let lookup = TypeLookup {
                aliases: Some(&self.resolved_aliases),
                class_types: Some(&self.class_types),
                type_params,
            };
            resolve_simple_type(ty, senv.types, &lookup, Some(&mut *senv.diagnostics))
        } {
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

impl SkeletonBuilder {
    /// Return type for a declaration that has no return type annotation.
    ///
    /// The body determines the type when every value-returning path agrees on
    /// one statically known type and the body cannot fall through; a body that
    /// never returns a value is `void`, and an `async` body wraps the inferred
    /// type (or `void`) in `Promise`, mirroring the annotated form. Anything
    /// else - calls or aggregates whose static type the skeleton does not know,
    /// mixed returns, generators - needs an explicit annotation, which is
    /// reported here with the declaration's span instead of failing later in the
    /// backend without a source location.
    fn infer_or_report_return_ty(
        body: &[HirStmt],
        span: CoreSpan,
        is_async: bool,
        is_generator: bool,
        senv: &mut SkeletonEnv,
    ) -> TypeId {
        use crate::util::ReturnInference;

        if is_async && is_generator {
            senv.diagnostics.push(Diagnostic::warning(
                RETURN_TYPE_INFERENCE_CODE,
                "an async generator has no return type annotation, so its element type cannot be \
                 inferred from the body; annotate the return type explicitly",
                span,
            ));
            return senv.error_ty();
        }
        match crate::util::infer_function_return_ty(body, senv.types, senv.program) {
            ReturnInference::Void if is_async => {
                let ok = senv.types.intern(&Type::Void);
                senv.types.intern(&Type::Promise { ok, err: None })
            }
            ReturnInference::Inferred(ty) if is_async => {
                // An async function's result is the *awaited* value, so an
                // inferred `Promise<T>` (`async function f() { return p; }` with
                // `p: Promise<T>`) is unwrapped once instead of being wrapped
                // again into `Promise<Promise<T>>`. Non-Promise results keep
                // being wrapped as before.
                let ok = match senv.types.resolve(ty).cloned() {
                    Some(Type::Promise { ok, .. }) => ok,
                    _ => ty,
                };
                senv.types.intern(&Type::Promise { ok, err: None })
            }
            ReturnInference::Inferred(ty) if !is_generator => ty,
            ReturnInference::Void if !is_generator => senv.types.intern(&Type::Void),
            ReturnInference::Void | ReturnInference::Inferred(_) => {
                senv.diagnostics.push(Diagnostic::warning(
                    RETURN_TYPE_INFERENCE_CODE,
                    "a generator has no return type annotation, so its element type cannot be \
                     inferred from the body; annotate the return type explicitly",
                    span,
                ));
                senv.error_ty()
            }
            ReturnInference::Unknown => {
                senv.diagnostics.push(Diagnostic::error(
                    RETURN_TYPE_INFERENCE_CODE,
                    "this function has no return type annotation and its return type cannot be \
                     inferred from the body; add an explicit return type annotation (the AOT \
                     target does not guess a type it cannot see)",
                    span,
                ));
                senv.error_ty()
            }
        }
    }
}
fn literal_value_hir(value: &Expression<'_>, span: CoreSpan) -> Option<HirExpr> {
    Some(match value {
        Expression::NumericLiteral(n) => crate::ops::number_to_hir(n.value, span),
        Expression::BooleanLiteral(b) => HirExpr::Bool(b.value, span),
        Expression::StringLiteral(s) => HirExpr::String(Atom::from(s.value.as_str()), span),
        _ => return None,
    })
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
        TSType::TSNeverKeyword(_) => Some("never"),
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

/// The TypeScript generator wrapper names whose single type argument is the
/// yielded type.
fn is_generator_wrapper(name: &str, is_async: bool) -> bool {
    match name {
        "Generator" | "IterableIterator" | "Iterator" => !is_async,
        "AsyncGenerator" | "AsyncIterableIterator" | "AsyncIterator" => is_async,
        _ => false,
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
