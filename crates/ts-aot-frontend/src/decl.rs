use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Class, Declaration, Expression, Function, MemberExpression, MethodDefinitionKind, Program,
    TSType, TSTypeName, match_member_expression,
};
use oxc_span::GetSpan;
use ts_aot_core::{
    Atom, Diagnostic, FieldId, GenericParamId, LocalId, Span as CoreSpan, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirCallee, HirClass, HirDecl, HirEnumVariant, HirExpr, HirField, HirFunction, HirParam,
    HirStmt, HirUnaryOp, Visitor, VisitorMut, walk_expr, walk_expr_mut, walk_stmt,
};

use crate::body::FunctionBodyInput;
use crate::scope::{BodyScope, ClassLookup};
use crate::skeleton::{
    CaptureRecord, ClassCaptureFields, PrivateClassFrame, PrivateMember, SkeletonBuilder,
    SkeletonEnv, StaticMethodCapture, SwitchCaseEventKind, TlaMainItem, private_member_hir_name,
    rust_ident_escape,
};
use crate::type_resolver::reference::TypeLookup;
use crate::type_resolver::{TypeParamMap, resolve_simple_type};
use crate::util::{binding_pattern_name, core_span_from_oxc};

const TYPE_RESOLUTION_FAILURE_CODE: &str = "E0400";

pub(crate) const RETURN_TYPE_INFERENCE_CODE: &str = "E0415";
const UNSUPPORTED_DECL_CODE: &str = "E0300";
const COMPUTED_CLASS_KEY_MESSAGE: &str =
    "a computed class field key is not supported; declare the field with a literal name";
const STATIC_CAPTURE_PARAM_BASE: u32 = 0x1000_0000;
const CLOSURE_CAPTURE_LOCAL_BASE: u32 = 0x1000_8000;
const FIELD_INIT_RECEIVER_ID: u32 = 0x2000_0000;

fn static_capture_param_id(index: usize) -> LocalId {
    LocalId::from_raw(STATIC_CAPTURE_PARAM_BASE.saturating_add(u32::try_from(index).unwrap_or(0)))
}

fn closure_capture_local_id(index: usize) -> LocalId {
    LocalId::from_raw(CLOSURE_CAPTURE_LOCAL_BASE.saturating_add(u32::try_from(index).unwrap_or(0)))
}

fn member_chain_root(expr: &HirExpr) -> &HirExpr {
    let mut current = expr;
    loop {
        match current {
            HirExpr::Field { owner, .. } | HirExpr::Index { owner, .. } => current = owner.as_ref(),
            _ => return current,
        }
    }
}

fn is_mutable_reference_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.resolve(ty) {
        Some(
            Type::Struct { .. }
            | Type::Array { .. }
            | Type::Date
            | Type::WeakMap { .. }
            | Type::ArrayBuffer,
        ) => true,
        Some(Type::Optional { inner, .. }) => is_mutable_reference_type(types, *inner),
        Some(Type::Union { variants }) => variants
            .iter()
            .any(|variant| is_mutable_reference_type(types, *variant)),
        _ => false,
    }
}

fn mutation_root_entry(root: &HirExpr) -> Option<(Option<LocalId>, Option<String>)> {
    match root {
        HirExpr::Local { id, .. } => Some((Some(*id), None)),
        HirExpr::Global { name, .. } => Some((None, Some(name.as_str().to_owned()))),
        _ => None,
    }
}

fn is_loop_stmt(stmt: &HirStmt) -> bool {
    matches!(
        stmt,
        HirStmt::While { .. }
            | HirStmt::DoWhile { .. }
            | HirStmt::ForOf { .. }
            | HirStmt::ForAwaitOf { .. }
            | HirStmt::ForIn { .. }
    )
}

fn collect_for_header_writes(
    stmts: &[oxc_ast::ast::Statement<'_>],
    out: &mut Vec<(String, CoreSpan)>,
) {
    use oxc_ast::ast::{ForStatementInit, Statement, VariableDeclarationKind};
    for stmt in stmts {
        match stmt {
            Statement::BlockStatement(block) => collect_for_header_writes(&block.body, out),
            Statement::IfStatement(it) => {
                collect_for_header_writes(std::slice::from_ref(&it.consequent), out);
                if let Some(alternate) = &it.alternate {
                    collect_for_header_writes(std::slice::from_ref(alternate), out);
                }
            }
            Statement::WhileStatement(while_stmt) => {
                collect_for_header_writes(std::slice::from_ref(&while_stmt.body), out);
            }
            Statement::DoWhileStatement(do_stmt) => {
                collect_for_header_writes(std::slice::from_ref(&do_stmt.body), out);
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(update) = &for_stmt.update
                    && let Some(ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init
                    && matches!(
                        decl.kind,
                        VariableDeclarationKind::Let | VariableDeclarationKind::Const
                    )
                {
                    let span = core_span_from_oxc(update.span());
                    for declarator in &decl.declarations {
                        if let Some(name) = binding_pattern_name(&declarator.id) {
                            out.push((name.to_string(), span));
                        }
                    }
                }
                collect_for_header_writes(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::ForInStatement(for_stmt) => {
                collect_for_header_writes(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::ForOfStatement(for_stmt) => {
                collect_for_header_writes(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::SwitchStatement(switch_stmt) => {
                for case in &switch_stmt.cases {
                    collect_for_header_writes(&case.consequent, out);
                }
            }
            Statement::TryStatement(try_stmt) => {
                collect_for_header_writes(&try_stmt.block.body, out);
                if let Some(handler) = &try_stmt.handler {
                    collect_for_header_writes(&handler.body.body, out);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    collect_for_header_writes(&finalizer.body, out);
                }
            }
            Statement::LabeledStatement(labeled) => {
                collect_for_header_writes(std::slice::from_ref(&labeled.body), out);
            }
            _ => {}
        }
    }
}

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
                let hir_class = self.build_class(senv, c, true, true, None);
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
                let hir_class = self.build_class(senv, c, true, true, None);
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
        let ret_hint = crate::param_infer::return_hint_ty(
            senv,
            func.return_type.is_some().then_some(ret),
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
            None,
        );
        let (params, extra_locals) = self.build_params_and_locals(
            senv,
            &func.params.items,
            0,
            Some(&type_param_map),
            Some(&hints),
            None,
        );

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
                outer_classes: None,
            },
            func.generator,
            false,
            true,
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
        top_level: bool,
        scope: Option<&BodyScope>,
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
        let saved_own_class_ty = self.current_class_ty.replace(ty);
        self.private_class_stack.push(PrivateClassFrame {
            ty,
            class_name: name.clone(),
            members: collect_private_members(class, ty),
        });
        self.register_static_fields(senv, class, ty, scope);

        let (class_type_param_ids, class_type_param_map) = build_type_param_context(
            senv.types,
            &mut self.next_generic_param,
            class.type_parameters.as_deref(),
        );

        let already_processed = senv.program.class_field_inits.contains_key(&ty);
        let mut fields: Vec<HirField> =
            if let Some(cached) = senv.program.class_fields.get(&ty).cloned() {
                cached
            } else {
                let computed =
                    self.collect_class_fields(senv, class, ty, Some(&class_type_param_map), scope);
                senv.program.class_fields.insert(ty, computed.clone());
                computed
            };
        let mut field_inits: Vec<(Atom, HirExpr)> = Vec::new();
        let mut emitted_statics: HashSet<Atom> = HashSet::new();
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::PropertyDefinition(p) = element else {
                continue;
            };
            let field_name = self.lowered_class_key_hir_name(ty, &p.key);
            if p.r#static {
                let Some(field_name) = field_name else {
                    if !matches!(p.key, oxc_ast::ast::PropertyKey::PrivateIdentifier(_)) {
                        senv.report_unsupported(
                            UNSUPPORTED_DECL_CODE,
                            COMPUTED_CLASS_KEY_MESSAGE,
                            p.key.span(),
                        );
                    }
                    continue;
                };
                if !already_processed {
                    let is_redeclaration = !emitted_statics.insert(field_name.clone());
                    let field_ty = self
                        .static_field_globals
                        .get(&ty)
                        .and_then(|statics| statics.get(&field_name))
                        .map_or_else(|| senv.error_ty(), |(_, field_ty)| *field_ty);
                    let span = p.value.as_ref().map_or_else(
                        || core_span_from_oxc(p.span),
                        |value| core_span_from_oxc(value.span()),
                    );
                    let global = static_field_global_name(name.as_str(), field_name.as_str(), ty);
                    let literal = p
                        .value
                        .as_ref()
                        .and_then(|value| field_init_hir(value, Some(field_ty), senv.types, span));
                    let seedable = self.module
                        && (top_level || self.module_init_depth > 0)
                        && p.value.is_some();
                    let goes_to_module = is_redeclaration
                        || literal.is_none()
                        || matches!(literal, Some(HirExpr::String(..)));
                    let const_init = if is_redeclaration || goes_to_module || seedable {
                        None
                    } else {
                        literal.clone()
                    };
                    let module_init = if seedable {
                        match literal {
                            Some(value) => Some(value),
                            None => p
                                .value
                                .as_ref()
                                .map(|value| self.walk_top_level_init(senv, value)),
                        }
                    } else {
                        None
                    };
                    let mut class_value_init = false;
                    let module_init = match module_init {
                        Some(HirExpr::DefaultConstruct { .. }) => {
                            class_value_init = true;
                            senv.report_unsupported(
                                "E0419",
                                format!(
                                    "static field `{}.{}` holds a class value, which this target \
                                     cannot store in a static field; name the class directly",
                                    name.as_str(),
                                    field_name.as_str()
                                )
                                .as_str(),
                                span,
                            );
                            None
                        }
                        other => other,
                    };
                    if p.value.is_some()
                        && const_init.is_none()
                        && module_init.is_none()
                        && !class_value_init
                    {
                        senv.diagnostics.push(Diagnostic::warning(
                            "E0406",
                            format!(
                                "static field initializer for `{}.{}` is not supported here; the \
                                 field will default to its zero value",
                                name.as_str(),
                                field_name.as_str()
                            ),
                            span,
                        ));
                    }
                    if !is_redeclaration {
                        senv.program.push_decl(HirDecl::Global {
                            name: global.clone(),
                            ty: field_ty,
                            init: const_init,
                        });
                    }
                    if let Some(value) = module_init {
                        self.tla_main_body
                            .push(TlaMainItem::Expr(HirExpr::Assignment {
                                target: Box::new(HirExpr::Global {
                                    name: global,
                                    ty: field_ty,
                                    span,
                                }),
                                value: Box::new(value),
                                ty: field_ty,
                                span,
                            }));
                    }
                }
                continue;
            }
            let field_name = field_name.unwrap_or_else(|| Atom::from(""));
            let Some(value) = &p.value else {
                continue;
            };
            let span = core_span_from_oxc(value.span());
            let field_ty = fields.iter().find(|f| f.name == field_name).map(|f| f.ty);
            let init = field_init_hir(value, field_ty, senv.types, span);
            match init {
                Some(init) => field_inits.push((field_name.clone(), init)),
                None => {
                    if let Expression::ClassExpression(class_expr) = value {
                        let holds_tag = field_ty
                            .is_some_and(|ty| matches!(senv.types.resolve(ty), Some(Type::I64)));
                        if already_processed {
                        } else if holds_tag {
                            let prebuilt = self
                                .class_value_fields
                                .get(&ty)
                                .and_then(|fields| fields.get(&field_name))
                                .and_then(|classes| classes.first().copied());
                            let class_ty = if let Some(class_ty) = prebuilt {
                                Some(class_ty)
                            } else {
                                let mut scratch = crate::scope::BodyScope::new(0);
                                let built =
                                    self.walk_class_expression(senv, class_expr, &mut scratch);
                                match built {
                                    HirExpr::DefaultConstruct { ty: class_ty, .. } => {
                                        self.class_value_fields
                                            .entry(ty)
                                            .or_default()
                                            .entry(field_name.clone())
                                            .or_default()
                                            .push(class_ty);
                                        self.class_value_field_initials
                                            .entry(ty)
                                            .or_default()
                                            .entry(field_name.clone())
                                            .or_default()
                                            .push(class_ty);
                                        Some(class_ty)
                                    }
                                    _ => None,
                                }
                            };
                            if let Some(class_ty) = class_ty {
                                let tag = self.class_value_tag(class_ty);
                                let span = core_span_from_oxc(value.span());
                                field_inits.push((field_name.clone(), HirExpr::Int(tag, span)));
                            }
                        } else {
                            senv.report_unsupported(
                                "E0419",
                                "a class value in an instance field is only supported when the \
                                 field has no type annotation, because the field stores the \
                                 class-value tag; drop the annotation, or name the class \
                                 directly at the use site",
                                value.span(),
                            );
                        }
                    } else if !already_processed && scope.is_none() {
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
        let extends = class
            .super_class
            .as_ref()
            .and_then(|expr| Self::resolve_class_parent_name(senv, expr, scope));

        let own_method_names = collect_own_method_names(class, ty);
        let saved_own_method_names =
            std::mem::replace(&mut self.current_class_own_methods, own_method_names);
        let mut methods = Vec::new();
        let mut accessor_names = HashSet::new();
        let mut ordinary_names = HashSet::new();
        let capture_frame =
            scope.map(|scope| crate::expr::capture_type_map(scope, &HashMap::new()));
        if let Some(frame) = capture_frame.clone() {
            senv.capture_types.push(frame);
        }
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::MethodDefinition(md) = element else {
                continue;
            };
            let is_accessor = matches!(
                md.kind,
                MethodDefinitionKind::Get | MethodDefinitionKind::Set
            );
            let saved_super = std::mem::replace(&mut self.current_super_class, extends.clone());
            let func = self.build_method(senv, md, ty, &class_type_param_map, scope);
            self.current_super_class = saved_super;
            if is_accessor {
                accessor_names.insert(func.name.clone());
            } else {
                ordinary_names.insert(func.name.clone());
            }
            methods.push(func);
        }
        if capture_frame.is_some() {
            senv.capture_types.pop();
        }
        self.current_class_own_methods = saved_own_method_names;
        if let Some(scope) = scope {
            self.rewrite_local_class_method_captures(
                senv,
                ty,
                scope,
                &mut fields,
                &mut field_inits,
                &mut methods,
                already_processed,
            );
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

        if !already_processed {
            senv.program.class_field_inits.insert(ty, field_inits);
        }

        match prior_binding {
            Some(prior) => {
                self.class_types.insert(name.as_str().to_owned(), prior);
            }
            None => {
                self.class_types.remove(name.as_str());
            }
        }

        let hir_class = HirClass {
            name,
            ty,
            pre_pass_ty: None,
            fields,
            methods,
            extends,
            type_params: class_type_param_ids,
        };
        self.register_static_methods(senv, &hir_class, &hir_class.name.clone());
        self.private_class_stack.pop();
        self.current_class_ty = saved_own_class_ty;
        hir_class
    }

    pub(crate) fn walk_local_class_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let Some(id) = class.id.as_ref() else {
            senv.report_unsupported(
                UNSUPPORTED_DECL_CODE,
                "a class declaration in a function body must be named",
                class.span,
            );
            return;
        };
        let name = Atom::from(id.name.as_str());
        let mut hir_class = self.build_class(senv, class, false, false, Some(scope));
        let class_ty = hir_class.ty;
        self.walk_local_class_field_inits(senv, class, class_ty, scope);
        let unique = self.fresh_anon_class_name(senv);
        hir_class.name = unique.clone();
        self.register_static_methods(senv, &hir_class, &unique);
        senv.program.push_decl(HirDecl::Class(hir_class));
        let tag = self.class_value_tag(class_ty);
        let int_ty = senv.types.intern(&Type::I64);
        let span = core_span_from_oxc(class.span);
        let local = scope.declare(name.as_str(), int_ty);
        scope.declare_class(name.as_str(), class_ty, unique);
        self.class_value_locals.insert(local, vec![class_ty]);
        self.constant_int_locals.remove(&local);
        self.note_switch_case_declaration(local, &name, scope.depth(), SwitchCaseEventKind::Decl);
        out.push(HirStmt::Let {
            id: local,
            name,
            ty: int_ty,
            init: Some(HirExpr::Int(tag, span)),
        });
    }

    fn rewrite_local_class_method_captures(
        &mut self,
        senv: &mut SkeletonEnv,
        class_ty: TypeId,
        scope: &BodyScope,
        fields: &mut Vec<HirField>,
        field_inits: &mut Vec<(Atom, HirExpr)>,
        methods: &mut [HirFunction],
        already_processed: bool,
    ) {
        struct CaptureRewrite<'a> {
            scope: &'a BodyScope,
            class_ty: TypeId,
            is_static: bool,
            in_closure: bool,
            static_accessor: bool,
            closure_bindings: &'a HashMap<String, (LocalId, TypeId)>,
            outer_captured: &'a HashSet<String>,
            reference_names: &'a HashSet<String>,
            declared_fields: &'a HashSet<Atom>,
            captures: &'a mut Vec<(Atom, Atom, CoreSpan)>,
            static_params: &'a mut Vec<(Atom, StaticMethodCapture)>,
            closure_names: &'a mut Vec<(Atom, CoreSpan)>,
            touched: &'a mut Vec<Atom>,
            unsupported: Option<&'static str>,
            unsupported_source: Option<Atom>,
        }
        impl VisitorMut for CaptureRewrite<'_> {
            fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
                if let HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. } =
                    expr
                    && let HirExpr::Global { name, .. } = target.as_ref()
                    && self.scope.type_of_visible(name.as_str()).is_some()
                {
                    self.unsupported = Some("write");
                    self.unsupported_source = Some(name.clone());
                    return;
                }
                if let HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. } =
                    expr
                    && !matches!(target.as_ref(), HirExpr::Global { .. })
                    && let HirExpr::Global { name, .. } = member_chain_root(target)
                    && self.reference_names.contains(name.as_str())
                {
                    self.unsupported = Some("mutate");
                    self.unsupported_source = Some(name.clone());
                    return;
                }
                if let HirExpr::Unary {
                    op: HirUnaryOp::Delete,
                    expr: deleted,
                    ..
                } = expr
                    && let HirExpr::Global { name, .. } = member_chain_root(deleted)
                    && self.reference_names.contains(name.as_str())
                {
                    self.unsupported = Some("mutate");
                    self.unsupported_source = Some(name.clone());
                    return;
                }
                if let HirExpr::Call {
                    callee: HirCallee::Indirect(callee),
                    ..
                } = expr
                    && let HirExpr::Field { owner, .. } = callee.as_ref()
                    && let HirExpr::Global { name, .. } = member_chain_root(owner)
                    && self.reference_names.contains(name.as_str())
                {
                    self.unsupported = Some("mutate");
                    self.unsupported_source = Some(name.clone());
                    return;
                }
                if let HirExpr::Global { name, ty, span, .. } = expr {
                    let visible = self.scope.type_of_visible(name.as_str()).is_some();
                    let outer = if visible {
                        None
                    } else {
                        self.closure_bindings.get(name.as_str()).copied()
                    };
                    if visible || outer.is_some() {
                        if self.is_static {
                            if self.scope.visible_class(name.as_str()).is_some() {
                                return;
                            }
                            if self.static_accessor || !visible {
                                self.unsupported = Some("static");
                                self.unsupported_source = Some(name.clone());
                                return;
                            }
                            let index = if let Some(index) = self
                                .static_params
                                .iter()
                                .position(|(_, capture)| capture.source == *name)
                            {
                                index
                            } else {
                                let Some((id, capture_ty)) = self.scope.lookup(name.as_str())
                                else {
                                    self.unsupported = Some("static");
                                    self.unsupported_source = Some(name.clone());
                                    return;
                                };
                                let mut candidate =
                                    Atom::from(format!("__capture_{}", name.as_str()));
                                let mut suffix = 0u32;
                                while self
                                    .static_params
                                    .iter()
                                    .any(|(param, _)| *param == candidate)
                                    || self.declared_fields.contains(&candidate)
                                {
                                    suffix = suffix.saturating_add(1);
                                    candidate =
                                        Atom::from(format!("__capture_{}_{suffix}", name.as_str()));
                                }
                                self.static_params.push((
                                    candidate,
                                    StaticMethodCapture {
                                        source: name.clone(),
                                        local: id,
                                        body_id: self.scope.body_id(),
                                        ty: capture_ty,
                                    },
                                ));
                                self.static_params.len().saturating_sub(1)
                            };
                            if self.in_closure {
                                self.closure_names.push((name.clone(), *span));
                                return;
                            }
                            *expr = HirExpr::Local {
                                id: static_capture_param_id(index),
                                ty: *ty,
                                span: *span,
                            };
                            return;
                        }
                        if self.scope.visible_class(name.as_str()).is_some() {
                            return;
                        }
                        if self.in_closure {
                            self.closure_names.push((name.clone(), *span));
                            return;
                        }
                        self.touched.push(name.clone());
                        let field_name = if let Some((_, field, _)) =
                            self.captures.iter().find(|(source, _, _)| source == name)
                        {
                            field.clone()
                        } else {
                            let mut candidate = Atom::from(format!("__capture_{}", name.as_str()));
                            let mut suffix = 0u32;
                            while self
                                .captures
                                .iter()
                                .any(|(_, field, _)| *field == candidate)
                                || self.declared_fields.contains(&candidate)
                            {
                                suffix = suffix.saturating_add(1);
                                candidate =
                                    Atom::from(format!("__capture_{}_{suffix}", name.as_str()));
                            }
                            self.captures.push((name.clone(), candidate.clone(), *span));
                            candidate
                        };
                        *expr = HirExpr::Field {
                            owner: Box::new(HirExpr::Local {
                                id: LocalId::from_raw(0),
                                ty: self.class_ty,
                                span: *span,
                            }),
                            field: FieldId::from_raw(0),
                            field_name,
                            ty: *ty,
                            span: *span,
                        };
                        return;
                    }
                    if self.outer_captured.contains(name.as_str()) {
                        self.unsupported = Some("outer");
                        self.unsupported_source = Some(name.clone());
                        return;
                    }
                }
                if matches!(expr, HirExpr::Closure { .. }) {
                    let saved = self.in_closure;
                    self.in_closure = true;
                    walk_expr_mut(self, expr);
                    self.in_closure = saved;
                    return;
                }
                walk_expr_mut(self, expr);
            }
        }
        struct InjectClosureCaptures<'a> {
            names: &'a [(Atom, LocalId, TypeId, CoreSpan)],
        }
        impl VisitorMut for InjectClosureCaptures<'_> {
            fn visit_expr_mut(&mut self, expr: &mut HirExpr) {
                if let HirExpr::Closure { captures, .. } = expr {
                    for (_, id, ty, span) in self.names {
                        if !captures.iter().any(
                            |capture| matches!(capture, HirExpr::Local { id: capture_id, .. } if capture_id == id),
                        ) {
                            captures.push(HirExpr::Local {
                                id: *id,
                                ty: *ty,
                                span: *span,
                            });
                        }
                    }
                }
                if let HirExpr::Global { name, ty, span, .. } = expr
                    && let Some((_, id, _, _)) =
                        self.names.iter().find(|(source, _, _, _)| source == name)
                {
                    *expr = HirExpr::Local {
                        id: *id,
                        ty: *ty,
                        span: *span,
                    };
                    return;
                }
                walk_expr_mut(self, expr);
            }
        }
        let mut captures: Vec<(Atom, Atom, CoreSpan)> = Vec::new();
        let mut non_ctor_sources: HashSet<Atom> = HashSet::new();
        let closure_bindings: HashMap<String, (LocalId, TypeId)> = self
            .pending_closure_captures
            .last()
            .filter(|frame| frame.closure_depth == self.body_depth)
            .map_or_else(HashMap::new, |frame| frame.enclosing_bindings.clone());
        let outer_captured: HashSet<String> = senv
            .capture_types
            .iter()
            .flat_map(|frame| frame.keys().cloned())
            .collect();
        let reference_names: HashSet<String> = scope
            .names()
            .into_iter()
            .chain(closure_bindings.keys().cloned())
            .filter(|name| {
                scope
                    .type_of_visible(name)
                    .or_else(|| closure_bindings.get(name).map(|(_, ty)| *ty))
                    .is_some_and(|ty| is_mutable_reference_type(senv.types, ty))
            })
            .collect();
        let declared_fields: HashSet<Atom> =
            fields.iter().map(|field| field.name.clone()).collect();
        for method in methods.iter_mut() {
            let is_static = method
                .params
                .first()
                .is_none_or(|param| param.name.as_str() != "this");
            let mut static_params: Vec<(Atom, StaticMethodCapture)> = Vec::new();
            let mut closure_names: Vec<(Atom, CoreSpan)> = Vec::new();
            let mut touched: Vec<Atom> = Vec::new();
            let static_accessor = is_static
                && (method.name.as_str().starts_with("__get_")
                    || method.name.as_str().starts_with("__set_"));
            let mut rewriter = CaptureRewrite {
                scope,
                class_ty,
                is_static,
                in_closure: false,
                static_accessor,
                closure_bindings: &closure_bindings,
                outer_captured: &outer_captured,
                reference_names: &reference_names,
                declared_fields: &declared_fields,
                captures: &mut captures,
                static_params: &mut static_params,
                closure_names: &mut closure_names,
                touched: &mut touched,
                unsupported: None,
                unsupported_source: None,
            };
            for stmt in &mut method.body {
                rewriter.visit_stmt_mut(stmt);
            }
            let mut unsupported = rewriter.unsupported;
            let unsupported_source = rewriter.unsupported_source.clone();
            let reads_at_construction = !is_static && method.name.as_str() == "constructor";
            if !reads_at_construction {
                for name in &touched {
                    non_ctor_sources.insert(name.clone());
                }
            }
            for (source, _) in &closure_names {
                non_ctor_sources.insert(source.clone());
            }
            if unsupported.is_none() && !closure_names.is_empty() {
                let mut closure_locals: Vec<(Atom, Atom, LocalId, TypeId)> = Vec::new();
                let mut injections: Vec<(Atom, LocalId, TypeId, CoreSpan)> = Vec::new();
                for (source, span) in &closure_names {
                    if is_static {
                        let Some(index) = static_params
                            .iter()
                            .position(|(_, capture)| capture.source == *source)
                        else {
                            unsupported = Some("static");
                            break;
                        };
                        injections.push((
                            source.clone(),
                            static_capture_param_id(index),
                            static_params[index].1.ty,
                            *span,
                        ));
                        continue;
                    }
                    let Some((_id, ty)) = scope
                        .lookup(source.as_str())
                        .or_else(|| closure_bindings.get(source.as_str()).copied())
                    else {
                        unsupported = Some("closure");
                        break;
                    };
                    let field_name = if let Some((_, field, _)) =
                        captures.iter().find(|(name, _, _)| name == source)
                    {
                        field.clone()
                    } else {
                        let mut candidate = Atom::from(format!("__capture_{}", source.as_str()));
                        let mut suffix = 0u32;
                        while captures.iter().any(|(_, field, _)| *field == candidate)
                            || declared_fields.contains(&candidate)
                        {
                            suffix = suffix.saturating_add(1);
                            candidate =
                                Atom::from(format!("__capture_{}_{suffix}", source.as_str()));
                        }
                        captures.push((source.clone(), candidate.clone(), *span));
                        candidate
                    };
                    let entry_id = closure_capture_local_id(closure_locals.len());
                    closure_locals.push((
                        field_name,
                        Atom::from(format!("__capture_{}", source.as_str())),
                        entry_id,
                        ty,
                    ));
                    injections.push((source.clone(), entry_id, ty, *span));
                }
                if unsupported.is_none() && !injections.is_empty() {
                    let mut injector = InjectClosureCaptures { names: &injections };
                    for stmt in &mut method.body {
                        injector.visit_stmt_mut(stmt);
                    }
                    if !closure_locals.is_empty() {
                        let mut prefix: Vec<HirStmt> = Vec::with_capacity(closure_locals.len());
                        for (field_name, local_name, id, ty) in &closure_locals {
                            prefix.push(HirStmt::Let {
                                id: *id,
                                name: local_name.clone(),
                                ty: *ty,
                                init: Some(HirExpr::Field {
                                    owner: Box::new(HirExpr::Local {
                                        id: LocalId::from_raw(0),
                                        ty: class_ty,
                                        span: CoreSpan::new(0, 0),
                                    }),
                                    field: FieldId::from_raw(0),
                                    field_name: field_name.clone(),
                                    ty: *ty,
                                    span: CoreSpan::new(0, 0),
                                }),
                            });
                        }
                        prefix.append(&mut method.body);
                        method.body = prefix;
                    }
                }
            }
            if is_static && !static_params.is_empty() {
                for (index, (param, capture)) in static_params.iter().enumerate() {
                    method.params.push(HirParam {
                        id: static_capture_param_id(index),
                        name: param.clone(),
                        ty: capture.ty,
                        optional: false,
                    });
                }
                self.pending_static_captures
                    .insert((class_ty, method.name.clone()), static_params);
            }
            match unsupported {
                Some("static") => senv.diagnostics.push(Diagnostic::error(
                    "E0419",
                    format!(
                        "the static method `{}` of a local class cannot reference a local \
                         variable of the enclosing function; pass the value as a parameter",
                        method.name.as_str()
                    ),
                    CoreSpan::new(0, 0),
                )),
                Some("outer") => senv.diagnostics.push(Diagnostic::error(
                    "E0419",
                    format!(
                        "the method `{}` of a local class reads `{}`, a local variable of an \
                         enclosing function that is only reachable through a closure or through \
                         the method of another local class; the class is lifted out of that body, \
                         so the variable cannot be bound there; pass the value as a parameter",
                        method.name.as_str(),
                        unsupported_source
                            .as_ref()
                            .map_or("<local>", |source| source.as_str())
                    ),
                    CoreSpan::new(0, 0),
                )),
                Some("mutate") => senv.diagnostics.push(Diagnostic::error(
                    "E0419",
                    format!(
                        "the method `{}` of a local class modifies the object held by `{}`, a local \
                         variable of the enclosing function; the class keeps a copy of the value \
                         taken at construction, so the modification would be lost; pass the object \
                         as a parameter instead of capturing it",
                        method.name.as_str(),
                        unsupported_source
                            .as_ref()
                            .map_or("<local>", |source| source.as_str())
                    ),
                    CoreSpan::new(0, 0),
                )),
                Some(_) => senv.diagnostics.push(Diagnostic::error(
                    "E0419",
                    "a method of a local class cannot assign to a local variable of the enclosing \
                     function, and a closure inside such a method cannot capture one; keep the \
                     variable read-only or pass the value through the class",
                    CoreSpan::new(0, 0),
                )),
                None => {}
            }
        }
        if captures.is_empty() {
            return;
        }
        let mut resolved: Vec<(Atom, Atom, CoreSpan, LocalId, TypeId)> = Vec::new();
        for (source, field_name, span) in &captures {
            let from_scope = scope.lookup(source.as_str());
            let from_closure = if from_scope.is_none() {
                closure_bindings.get(source.as_str()).copied()
            } else {
                None
            };
            let Some((binding, binding_ty)) = from_scope.or(from_closure) else {
                continue;
            };
            if from_closure.is_some()
                && let Some(frame) = self.pending_closure_captures.last_mut()
            {
                frame.require(binding, binding_ty, *span);
            }
            resolved.push((
                source.clone(),
                field_name.clone(),
                *span,
                binding,
                binding_ty,
            ));
        }
        for (_, field_name, span, binding, binding_ty) in &resolved {
            if fields.iter().any(|field| field.name == *field_name) {
                continue;
            }
            fields.push(HirField {
                name: field_name.clone(),
                ty: *binding_ty,
            });
            field_inits.push((
                field_name.clone(),
                HirExpr::Local {
                    id: *binding,
                    ty: *binding_ty,
                    span: *span,
                },
            ));
        }
        self.class_capture_fields.insert(
            class_ty,
            resolved
                .iter()
                .map(|(source, field_name, span, binding, binding_ty)| {
                    (
                        field_name.clone(),
                        source.clone(),
                        *binding,
                        *binding_ty,
                        *span,
                    )
                })
                .collect(),
        );
        self.class_decl_body.insert(class_ty, scope.body_id());
        if let Some(frame) = self.capture_check_frames.last_mut() {
            for (source, _, span, binding, binding_ty) in &resolved {
                frame.record(CaptureRecord {
                    binding: *binding,
                    source: source.clone(),
                    span: *span,
                    construction_only: !non_ctor_sources.contains(source),
                    mutable_reference: is_mutable_reference_type(senv.types, *binding_ty),
                });
            }
        }
        if !already_processed {
            senv.program.class_fields.insert(class_ty, fields.clone());
        }
    }

    pub(crate) fn check_local_class_construction(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: TypeId,
        scope: &BodyScope,
        span: CoreSpan,
    ) {
        if self.class_capture_fields.is_empty() {
            return;
        }
        let parents = collect_class_parents(&senv.program.declarations);
        let mut chain: Vec<TypeId> = Vec::new();
        let mut current = Some(ty);
        while let Some(class_ty) = current {
            if chain.contains(&class_ty) {
                break;
            }
            chain.push(class_ty);
            current = parents.get(&class_ty).copied();
        }
        let body_id = scope.body_id();
        let mut missing: Vec<Atom> = Vec::new();
        for class_ty in chain {
            let Some(decl_body) = self.class_decl_body.get(&class_ty).copied() else {
                continue;
            };
            let needed = self
                .class_capture_fields
                .get(&class_ty)
                .cloned()
                .unwrap_or_default();
            if needed.is_empty() || decl_body == body_id {
                continue;
            }
            let in_declaring_closure = self.pending_closure_captures.last().is_some_and(|frame| {
                frame.closure_depth == self.body_depth && frame.enclosing_body_id == decl_body
            });
            if in_declaring_closure {
                if let Some(frame) = self.pending_closure_captures.last_mut() {
                    for (_, _, binding, binding_ty, capture_span) in needed {
                        frame.require(binding, binding_ty, capture_span);
                    }
                }
                continue;
            }
            for (_, source, _, _, _) in needed {
                if !missing.iter().any(|name| name.as_str() == source.as_str()) {
                    missing.push(source);
                }
            }
        }
        if missing.is_empty() {
            return;
        }
        let capture_list = missing
            .iter()
            .map(|source| format!("`{}`", source.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        senv.report_unsupported(
            "E0419",
            format!(
                "a local class constructed here captures the local variable {capture_list} of the \
                 function that declares the class; the construction happens in a different body, \
                 where that variable is not in scope; construct the class where it is declared or \
                 pass the value as a parameter"
            )
            .as_str(),
            span,
        );
    }

    pub(crate) fn finish_capture_check_frame(
        &mut self,
        senv: &mut SkeletonEnv,
        body: &[HirStmt],
        ast: &[oxc_ast::ast::Statement<'_>],
    ) {
        #[derive(Clone)]
        struct WriteEntry {
            id: Option<LocalId>,
            name: Option<String>,
            span: CoreSpan,
            in_closure: bool,
            loops: Vec<u32>,
        }
        impl WriteEntry {
            fn new(
                id: Option<LocalId>,
                name: Option<String>,
                span: CoreSpan,
                in_closure: bool,
                loops: &[u32],
            ) -> Self {
                Self {
                    id,
                    name,
                    span,
                    in_closure,
                    loops: loops.to_vec(),
                }
            }
        }
        struct WriteScan {
            writes: Vec<WriteEntry>,
            mutations: Vec<WriteEntry>,
            closure_depth: u32,
            loops: Vec<u32>,
            next_loop_id: u32,
        }
        impl Visitor for WriteScan {
            fn visit_stmt(&mut self, stmt: &HirStmt) {
                if is_loop_stmt(stmt) {
                    let id = self.next_loop_id;
                    self.next_loop_id = self.next_loop_id.saturating_add(1);
                    self.loops.push(id);
                    walk_stmt(self, stmt);
                    self.loops.pop();
                    return;
                }
                walk_stmt(self, stmt);
            }

            fn visit_expr(&mut self, expr: &HirExpr) {
                let in_closure = self.closure_depth > 0;
                let loops = self.loops.clone();
                if let HirExpr::Assignment { target, span, .. }
                | HirExpr::CompoundUpdate { target, span, .. } = expr
                {
                    match target.as_ref() {
                        HirExpr::Local { id, .. } => {
                            self.writes.push(WriteEntry::new(
                                Some(*id),
                                None,
                                *span,
                                in_closure,
                                &loops,
                            ));
                        }
                        HirExpr::Global { name, .. } => {
                            self.writes.push(WriteEntry::new(
                                None,
                                Some(name.as_str().to_owned()),
                                *span,
                                in_closure,
                                &loops,
                            ));
                        }
                        other => {
                            if let Some((id, name)) = mutation_root_entry(member_chain_root(other))
                            {
                                self.mutations
                                    .push(WriteEntry::new(id, name, *span, in_closure, &loops));
                            }
                        }
                    }
                }
                let mutation = match expr {
                    HirExpr::Unary {
                        op: HirUnaryOp::Delete,
                        expr: deleted,
                        ..
                    } => mutation_root_entry(member_chain_root(deleted)),
                    HirExpr::Call {
                        callee: HirCallee::Indirect(callee),
                        ..
                    } => match callee.as_ref() {
                        HirExpr::Field { owner, .. } => {
                            mutation_root_entry(member_chain_root(owner))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((id, name)) = mutation {
                    self.mutations
                        .push(WriteEntry::new(id, name, expr.span(), in_closure, &loops));
                }
                if matches!(expr, HirExpr::Closure { .. }) {
                    self.closure_depth = self.closure_depth.saturating_add(1);
                    walk_expr(self, expr);
                    self.closure_depth = self.closure_depth.saturating_sub(1);
                    return;
                }
                walk_expr(self, expr);
            }
        }
        struct ConstructionScan<'a> {
            capture_fields: &'a ClassCaptureFields,
            bases: &'a HashMap<TypeId, TypeId>,
            first: HashMap<LocalId, (u32, Vec<u32>)>,
            loops: Vec<u32>,
            next_loop_id: u32,
        }
        impl Visitor for ConstructionScan<'_> {
            fn visit_stmt(&mut self, stmt: &HirStmt) {
                if is_loop_stmt(stmt) {
                    let id = self.next_loop_id;
                    self.next_loop_id = self.next_loop_id.saturating_add(1);
                    self.loops.push(id);
                    walk_stmt(self, stmt);
                    self.loops.pop();
                    return;
                }
                walk_stmt(self, stmt);
            }

            fn visit_expr(&mut self, expr: &HirExpr) {
                let construction = match expr {
                    HirExpr::New { ty, span, .. } | HirExpr::DefaultConstruct { ty, span } => {
                        Some((*ty, *span))
                    }
                    _ => None,
                };
                if let Some((ty, span)) = construction {
                    let mut current = Some(ty);
                    let mut seen: HashSet<TypeId> = HashSet::new();
                    while let Some(class_ty) = current {
                        if !seen.insert(class_ty) {
                            break;
                        }
                        if let Some(entries) = self.capture_fields.get(&class_ty) {
                            for (field_name, _, binding, _, _) in entries {
                                if is_field_initializer_capture(field_name) {
                                    continue;
                                }
                                let replace = self
                                    .first
                                    .get(binding)
                                    .is_none_or(|(first, _)| span.start < *first);
                                if replace {
                                    self.first
                                        .insert(*binding, (span.start, self.loops.clone()));
                                }
                            }
                        }
                        current = self.bases.get(&class_ty).copied();
                    }
                }
                walk_expr(self, expr);
            }
        }
        struct AliasScan<'a> {
            edges: &'a mut HashMap<LocalId, LocalId>,
        }
        impl Visitor for AliasScan<'_> {
            fn visit_stmt(&mut self, stmt: &HirStmt) {
                if let HirStmt::Let {
                    id,
                    init: Some(HirExpr::Local { id: source, .. }),
                    ..
                } = stmt
                {
                    self.edges.insert(*id, *source);
                }
                walk_stmt(self, stmt);
            }
        }
        let Some(frame) = self.capture_check_frames.pop() else {
            return;
        };
        if frame.captures.is_empty() && frame.inherited.is_empty() {
            return;
        }
        let mut exempt: Vec<(String, CoreSpan)> = Vec::new();
        collect_for_header_writes(ast, &mut exempt);
        let mut scan = WriteScan {
            writes: Vec::new(),
            mutations: Vec::new(),
            closure_depth: 0,
            loops: Vec::new(),
            next_loop_id: 0,
        };
        for stmt in body {
            scan.visit_stmt(stmt);
        }
        let mut alias_edges: HashMap<LocalId, LocalId> = HashMap::new();
        for stmt in body {
            AliasScan {
                edges: &mut alias_edges,
            }
            .visit_stmt(stmt);
        }
        let resolve_alias = |mut id: LocalId| -> LocalId {
            let mut seen: Vec<u32> = Vec::new();
            while let Some(next) = alias_edges.get(&id) {
                if seen.contains(&id.raw()) {
                    break;
                }
                seen.push(id.raw());
                id = *next;
            }
            id
        };
        let bases = collect_class_parents(&senv.program.declarations);
        let mut constructions = ConstructionScan {
            capture_fields: &self.class_capture_fields,
            bases: &bases,
            first: HashMap::new(),
            loops: Vec::new(),
            next_loop_id: 0,
        };
        for stmt in body {
            constructions.visit_stmt(stmt);
        }
        let first_construction = constructions.first;
        let before_construction =
            |binding: Option<LocalId>, span: CoreSpan, in_closure: bool, loops: &[u32]| {
                !in_closure
                    && binding.is_some_and(|binding| {
                        first_construction
                            .get(&binding)
                            .is_some_and(|(first, first_loops)| {
                                span.end <= *first
                                    && !loops.iter().any(|id| first_loops.contains(id))
                            })
                    })
            };
        let matches_entry = |entry: &WriteEntry, binding: Option<LocalId>, source: &Atom| {
            (binding.is_some_and(|binding| entry.id == Some(binding))
                || entry.name.as_deref() == Some(source.as_str()))
                && !exempt.iter().any(|(exempt_name, exempt_span)| {
                    exempt_name.as_str() == source.as_str() && *exempt_span == entry.span
                })
        };
        let written = |binding: Option<LocalId>, source: &Atom| {
            scan.writes.iter().any(|entry| {
                matches_entry(entry, binding, source)
                    && !before_construction(binding, entry.span, entry.in_closure, &entry.loops)
            })
        };
        let mutated = |binding: Option<LocalId>, source: &Atom| {
            scan.mutations.iter().any(|entry| {
                let through_alias = binding.is_some_and(|binding| {
                    entry
                        .id
                        .is_some_and(|id| resolve_alias(id).raw() == binding.raw())
                });
                (through_alias || matches_entry(entry, binding, source))
                    && !before_construction(binding, entry.span, entry.in_closure, &entry.loops)
            })
        };
        let mut reported: Vec<Atom> = Vec::new();
        let report_write = |source: &Atom, span: CoreSpan, senv: &mut SkeletonEnv| {
            senv.diagnostics.push(Diagnostic::error(
                "E0419",
                format!(
                    "the local `{}` is read by a method of a local class but is assigned in the \
                     enclosing function; the class keeps a snapshot taken at construction, so the \
                     method could read a stale value; declare the variable `const`, keep it \
                     unassigned, or pass the value as a parameter",
                    source.as_str()
                ),
                span,
            ));
        };
        let report_mutate = |source: &Atom, span: CoreSpan, senv: &mut SkeletonEnv| {
            senv.diagnostics.push(Diagnostic::error(
                "E0419",
                format!(
                    "the local `{}` is read by a method of a local class but the object it holds is \
                     modified in the enclosing function after the class is constructed; the class \
                     keeps a copy taken at construction, so the method could read stale contents; \
                     modify the object before constructing the class or pass it as a parameter",
                    source.as_str()
                ),
                span,
            ));
        };
        for record in &frame.captures {
            if record.construction_only {
                continue;
            }
            if written(Some(record.binding), &record.source) {
                report_write(&record.source, record.span, senv);
                reported.push(record.source.clone());
            } else if record.mutable_reference && mutated(Some(record.binding), &record.source) {
                report_mutate(&record.source, record.span, senv);
                reported.push(record.source.clone());
            }
        }
        for (source, span) in &frame.inherited {
            if written(None, source) {
                report_write(source, *span, senv);
                reported.push(source.clone());
            }
        }
        if let Some(parent) = self.capture_check_frames.last_mut() {
            for record in &frame.captures {
                if !reported
                    .iter()
                    .any(|name| name.as_str() == record.source.as_str())
                {
                    parent.inherit(record.source.clone(), record.span);
                }
            }
            for (source, span) in &frame.inherited {
                if !reported.iter().any(|name| name.as_str() == source.as_str()) {
                    parent.inherit(source.clone(), *span);
                }
            }
        }
    }

    pub(crate) fn walk_local_class_field_inits(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        ty: TypeId,
        scope: &mut BodyScope,
    ) {
        let fields = senv
            .program
            .class_fields
            .get(&ty)
            .cloned()
            .unwrap_or_default();
        let class_name = class
            .id
            .as_ref()
            .map_or_else(|| Atom::from(""), |id| Atom::from(id.name.as_str()));
        self.private_class_stack.push(PrivateClassFrame {
            ty,
            class_name,
            members: collect_private_members(class, ty),
        });
        scope.push();
        scope.declare_param("this", LocalId::from_raw(FIELD_INIT_RECEIVER_ID), ty);
        let body_id = scope.body_id();
        let mut enclosing_reads: Vec<(Atom, LocalId, TypeId, CoreSpan)> = Vec::new();
        let mut walked_inits: Vec<(Atom, HirExpr)> = Vec::new();
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::PropertyDefinition(p) = element else {
                continue;
            };
            if p.r#static {
                continue;
            }
            let Some(value) = &p.value else {
                continue;
            };
            let Some(field_name) = self.lowered_class_key_hir_name(ty, &p.key) else {
                continue;
            };
            let display_name = p.key.private_name().map_or_else(
                || field_name.as_str().to_owned(),
                |private| format!("#{}", private.as_str()),
            );
            let span = core_span_from_oxc(value.span());
            let field_ty = fields.iter().find(|f| f.name == field_name).map(|f| f.ty);
            if field_init_hir(value, field_ty, senv.types, span).is_some() {
                continue;
            }
            if matches!(value, Expression::ClassExpression(_)) {
                continue;
            }
            if field_ty.is_none_or(|ty| matches!(senv.types.resolve(ty), None | Some(Type::Error)))
            {
                senv.diagnostics.push(Diagnostic::warning(
                    "E0406",
                    format!(
                        "class field initializer for `{display_name}` is not supported in AOT \
                         (only literal number/string/bool initializers are); the field will \
                         default to its zero value"
                    ),
                    span,
                ));
                continue;
            }
            let walked = self.walk_expr(senv, value, scope);
            if field_init_reads_the_instance(&walked) {
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "the initializer of `{display_name}` reads the instance it is stored in \
                         (`this` or one of its members); a field initializer is evaluated before \
                         that instance exists in this AOT target, so the initializer cannot be \
                         represented; compute the value in the constructor instead"
                    )
                    .as_str(),
                    value.span(),
                );
                continue;
            }
            collect_enclosing_local_reads(&walked, scope, &mut enclosing_reads);
            walked_inits.push((field_name, walked));
        }
        scope.pop();
        self.private_class_stack.pop();
        if !enclosing_reads.is_empty() {
            self.class_decl_body.entry(ty).or_insert(body_id);
            let entries = self.class_capture_fields.entry(ty).or_default();
            for (source, binding, binding_ty, capture_span) in enclosing_reads {
                if entries
                    .iter()
                    .any(|(_, _, existing, _, _)| existing.raw() == binding.raw())
                {
                    continue;
                }
                entries.push((Atom::from(""), source, binding, binding_ty, capture_span));
            }
        }
        if walked_inits.is_empty() {
            return;
        }
        let inits = senv.program.class_field_inits.entry(ty).or_default();
        for init in walked_inits {
            if inits.iter().any(|(name, _)| *name == init.0) {
                continue;
            }
            inits.push(init);
        }
    }

    pub(crate) fn fresh_anon_class_name(&mut self, senv: &SkeletonEnv<'_>) -> Atom {
        let module_id = senv.program.module.raw();
        let seq = self.next_anon_class_id;
        self.next_anon_class_id = self.next_anon_class_id.saturating_add(1);
        Atom::from(format!("__class_m{module_id}_{seq}"))
    }

    pub(crate) fn register_static_methods(
        &mut self,
        senv: &mut SkeletonEnv,
        hir_class: &HirClass,
        class_name: &Atom,
    ) {
        if class_name.as_str().is_empty() {
            return;
        }
        let mut methods: HashMap<Atom, (Atom, TypeId, TypeId)> = HashMap::new();
        for method in &hir_class.methods {
            let is_static = method
                .params
                .first()
                .is_none_or(|param| param.name.as_str() != "this");
            if !is_static {
                continue;
            }
            if method.name.as_str().starts_with("__get_")
                || method.name.as_str().starts_with("__set_")
            {
                continue;
            }
            let fn_ty = senv.types.intern(&Type::Fn {
                params: method.params.iter().map(|param| param.ty).collect(),
                optional: method.params.iter().map(|param| param.optional).collect(),
                ret: method.ret,
                err: None,
            });
            methods.insert(
                method.name.clone(),
                (
                    Atom::from(format!("{}::{}", class_name.as_str(), method.name.as_str())),
                    method.ret,
                    fn_ty,
                ),
            );
        }
        if !methods.is_empty() {
            for (method_name, (global, _, _)) in &methods {
                if let Some(captures) = self
                    .pending_static_captures
                    .get(&(hir_class.ty, method_name.clone()))
                {
                    let captured = captures
                        .iter()
                        .map(|(_, capture)| capture.clone())
                        .collect();
                    self.static_method_captures.insert(global.clone(), captured);
                }
            }
            self.static_method_globals.insert(hir_class.ty, methods);
        }
    }

    fn resolve_class_parent_name(
        senv: &mut SkeletonEnv,
        expr: &Expression<'_>,
        scope: Option<&BodyScope>,
    ) -> Option<Atom> {
        if let Some(scope) = scope
            && let Expression::Identifier(id) = expr
        {
            match scope.nearest_class_binding(id.name.as_str()) {
                ClassLookup::Class(binding) => return Some(binding.unique),
                ClassLookup::Shadowed => {
                    senv.report_unsupported(
                        UNSUPPORTED_DECL_CODE,
                        format!(
                            "the class extends `{name}`, but the nearest `{name}` in scope is not \
                             a class value; the local binding shadows any outer class of that \
                             name, so the parent cannot be resolved",
                            name = id.name.as_str()
                        )
                        .as_str(),
                        id.span,
                    );
                    return None;
                }
                ClassLookup::Missing => {}
            }
        }
        senv.resolve_superclass_name(expr)
    }

    fn class_property_ty(
        &mut self,
        senv: &mut SkeletonEnv,
        p: &oxc_ast::ast::PropertyDefinition<'_>,
        type_params: Option<&TypeParamMap>,
        scope: Option<&BodyScope>,
    ) -> TypeId {
        let mut ty = self.resolve_ts_type_from_annotation_with_params_in_scope(
            senv,
            p.type_annotation.as_deref(),
            type_params,
            scope,
        );
        if p.type_annotation.is_none()
            && matches!(senv.types.resolve(ty), None | Some(Type::Error))
            && let Some(value) = &p.value
        {
            if matches!(value, Expression::ClassExpression(_)) {
                return senv.types.intern(&Type::I64);
            }
            if let Some(init) = literal_value_hir(value, core_span_from_oxc(value.span()))
                && let Some(inferred) = crate::expr::literal_hir_ty(senv.types, &init)
            {
                ty = inferred;
            }
        }
        ty
    }

    fn collect_class_fields(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        class_ty: TypeId,
        type_params: Option<&TypeParamMap>,
        scope: Option<&BodyScope>,
    ) -> Vec<HirField> {
        class
            .body
            .body
            .iter()
            .filter_map(|m| {
                let oxc_ast::ast::ClassElement::PropertyDefinition(p) = m else {
                    return None;
                };
                if p.r#static {
                    return None;
                }
                let Some(name) = self.lowered_class_key_hir_name(class_ty, &p.key) else {
                    if !matches!(p.key, oxc_ast::ast::PropertyKey::PrivateIdentifier(_)) {
                        senv.report_unsupported(
                            UNSUPPORTED_DECL_CODE,
                            COMPUTED_CLASS_KEY_MESSAGE,
                            p.key.span(),
                        );
                    }
                    return None;
                };
                let ty = self.class_property_ty(senv, p, type_params, scope);
                Some(HirField { name, ty })
            })
            .collect()
    }

    pub(crate) fn register_static_fields(
        &mut self,
        senv: &mut SkeletonEnv,
        class: &Class<'_>,
        ty: TypeId,
        scope: Option<&BodyScope>,
    ) {
        let class_name = class.id.as_ref().map_or("", |id| id.name.as_str());
        let mut statics: HashMap<Atom, (Atom, TypeId)> = HashMap::new();
        for element in &class.body.body {
            let oxc_ast::ast::ClassElement::PropertyDefinition(p) = element else {
                continue;
            };
            if !p.r#static {
                continue;
            }
            let Some(field_name) = self.lowered_class_key_hir_name(ty, &p.key) else {
                continue;
            };
            let field_ty = self.class_property_ty(senv, p, None, scope);
            statics.insert(
                field_name.clone(),
                (
                    static_field_global_name(class_name, field_name.as_str(), ty),
                    field_ty,
                ),
            );
        }
        if !statics.is_empty() {
            self.static_field_globals.insert(ty, statics);
        }
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
            let Some(ty) = self.class_types.get(id.name.as_str()).copied() else {
                continue;
            };
            self.register_static_fields(senv, class, ty, None);
            if class
                .type_parameters
                .as_ref()
                .is_some_and(|p| !p.params.is_empty())
            {
                continue;
            }
            if senv.program.class_fields.contains_key(&ty) {
                continue;
            }
            let fields = {
                self.private_class_stack.push(PrivateClassFrame {
                    ty,
                    class_name: Atom::from(id.name.as_str()),
                    members: collect_private_members(class, ty),
                });
                let fields = self.collect_class_fields(senv, class, ty, None, None);
                self.private_class_stack.pop();
                fields
            };
            for element in &class.body.body {
                let oxc_ast::ast::ClassElement::PropertyDefinition(p) = element else {
                    continue;
                };
                if p.r#static {
                    continue;
                }
                let (Some(value), Some(key_name)) = (&p.value, p.key.static_name()) else {
                    continue;
                };
                let Expression::ClassExpression(class_expr) = value else {
                    continue;
                };
                let field_name = Atom::from(key_name.as_ref());
                let holds_tag = fields
                    .iter()
                    .find(|f| f.name == field_name)
                    .is_some_and(|f| matches!(senv.types.resolve(f.ty), Some(Type::I64)));
                if !holds_tag {
                    continue;
                }
                let mut scratch = crate::scope::BodyScope::new(0);
                let built = self.walk_class_expression(senv, class_expr, &mut scratch);
                if let HirExpr::DefaultConstruct { ty: class_ty, .. } = built {
                    self.class_value_fields
                        .entry(ty)
                        .or_default()
                        .entry(field_name.clone())
                        .or_default()
                        .push(class_ty);
                    self.class_value_field_initials
                        .entry(ty)
                        .or_default()
                        .entry(field_name)
                        .or_default()
                        .push(class_ty);
                }
            }
            senv.program.class_fields.insert(ty, fields);
        }
    }

    fn build_method(
        &mut self,
        senv: &mut SkeletonEnv,
        md: &oxc_ast::ast::MethodDefinition<'_>,
        class_ty: TypeId,
        class_type_param_map: &TypeParamMap,
        scope: Option<&BodyScope>,
    ) -> HirFunction {
        let value = &*md.value;
        let base_name = self
            .lowered_class_key_hir_name(class_ty, &md.key)
            .unwrap_or_else(|| Atom::from(""));
        let method_name = match md.kind {
            MethodDefinitionKind::Get => Atom::from(format!("__get_{base_name}")),
            MethodDefinitionKind::Set => Atom::from(format!("__set_{base_name}")),
            _ => base_name.clone(),
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
        let ret = if value.return_type.is_some() {
            if value.generator {
                self.resolve_generator_ret_ty(
                    senv,
                    value.return_type.as_deref(),
                    Some(&combined_map),
                    value.r#async,
                )
            } else {
                self.resolve_ts_type_from_annotation_with_params_in_scope(
                    senv,
                    value.return_type.as_deref(),
                    Some(&combined_map),
                    scope,
                )
            }
        } else {
            senv.error_ty()
        };
        let ret_hint = crate::param_infer::return_hint_ty(
            senv,
            value.return_type.is_some().then_some(ret),
            value.r#async,
            value.generator,
        );
        let hints = Self::infer_param_hints(
            senv,
            value.body.as_deref(),
            &value.params.items,
            ret_hint,
            Some(class_ty),
            false,
            scope,
        );
        let (method_params, extra_locals) = self.build_params_and_locals(
            senv,
            &value.params.items,
            u32::from(needs_synthetic_this),
            Some(&combined_map),
            Some(&hints),
            scope,
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
                outer_classes: scope,
            },
            value.generator,
            true,
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
        self.class_method_rets
            .insert((class_ty, method_name.clone(), md.r#static), ret);
        if matches!(md.kind, MethodDefinitionKind::Get) {
            self.class_method_rets
                .entry((class_ty, base_name.clone(), md.r#static))
                .or_insert(ret);
        }

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
            match v.kind {
                oxc_ast::ast::VariableDeclarationKind::Let => {
                    self.module_lets.insert(name.as_str().to_owned());
                }
                oxc_ast::ast::VariableDeclarationKind::Var => {
                    self.module_vars.insert(name.as_str().to_owned());
                }
                oxc_ast::ast::VariableDeclarationKind::Const
                | oxc_ast::ast::VariableDeclarationKind::Using
                | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => {
                    if let Some(init_expr) = init.as_ref().filter(|e| is_const_initializer(e)) {
                        let facts = self.class_value_poison_facts(0, init_expr);
                        self.class_value_poisoned_globals
                            .insert(name.as_str().to_owned(), facts);
                    }
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
        let mut ty = ty;
        let mut init = init;
        if let Some(HirExpr::ArrayLiteral { ty: arr_ty, .. }) = init.as_mut()
            && matches!(
                senv.types.resolve(ty),
                Some(ts_aot_core::Type::Array { .. })
            )
        {
            *arr_ty = ty;
        }
        let generic_class = matches!(
            &init,
            Some(HirExpr::DefaultConstruct { ty, .. })
                if senv.program.declarations.iter().any(|d| matches!(
                    d,
                    ts_aot_ir_hir::HirDecl::Class(c)
                        if c.ty == *ty && !c.type_params.is_empty()
                ))
        );
        if let Some(HirExpr::DefaultConstruct {
            ty: class_ty, span, ..
        }) = &init
            && !generic_class
        {
            let (class_ty, class_span) = (*class_ty, *span);
            self.class_types.insert(name.as_str().to_owned(), class_ty);
            self.class_value_globals
                .insert(name.as_str().to_owned(), vec![class_ty]);
            let tag = self.class_value_tag(class_ty);
            init = Some(HirExpr::Int(tag, class_span));
            ty = senv.types.intern(&ts_aot_core::Type::I64);
        }
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
        self.resolve_ts_type_with_params_in_scope(senv, ty, type_params, None, false)
    }

    pub(crate) fn resolve_ts_type_with_params_in_scope(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: Option<&TSType<'_>>,
        type_params: Option<&TypeParamMap>,
        scope: Option<&BodyScope>,
        lexical_class_priority: bool,
    ) -> TypeId {
        if let Some(ts_type) = ty
            && let Some(name) = banned_type_name(ts_type)
        {
            let span = core_span_from_oxc(ts_type.span());
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
        let scoped_classes = scope.and_then(|scope| {
            let mut merged = scope.visible_class_types();
            if merged.is_empty() {
                return None;
            }
            for (name, ty) in &self.class_types {
                let prefer_scope = lexical_class_priority
                    && merged
                        .get(name)
                        .is_some_and(|scope_ty| *scope_ty != *ty && scope_ty.raw() > ty.raw());
                if prefer_scope || Some(*ty) != self.current_class_ty {
                    merged.entry(name.clone()).or_insert(*ty);
                } else {
                    merged.insert(name.clone(), *ty);
                }
            }
            Some(merged)
        });
        let class_types = scoped_classes.as_ref().unwrap_or(&self.class_types);
        if let Some(id) = {
            let lookup = TypeLookup {
                aliases: Some(&self.resolved_aliases),
                class_types: Some(class_types),
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

    pub(crate) fn resolve_ts_type_from_annotation_in_scope(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        scope: Option<&BodyScope>,
    ) -> TypeId {
        self.resolve_ts_type_with_params_in_scope(
            senv,
            ann.map(|a| &a.type_annotation),
            None,
            scope,
            true,
        )
    }

    pub(crate) fn resolve_ts_type_from_annotation_in_declaration_scope(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        scope: Option<&BodyScope>,
    ) -> TypeId {
        self.resolve_ts_type_with_params_in_scope(
            senv,
            ann.map(|a| &a.type_annotation),
            None,
            scope,
            true,
        )
    }

    pub(crate) fn resolve_ts_type_from_annotation_with_params(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        type_params: Option<&TypeParamMap>,
    ) -> TypeId {
        self.resolve_ts_type_with_params(senv, ann.map(|a| &a.type_annotation), type_params)
    }

    pub(crate) fn resolve_ts_type_from_annotation_with_params_in_scope(
        &mut self,
        senv: &mut SkeletonEnv,
        ann: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
        type_params: Option<&TypeParamMap>,
        scope: Option<&BodyScope>,
    ) -> TypeId {
        self.resolve_ts_type_with_params_in_scope(
            senv,
            ann.map(|a| &a.type_annotation),
            type_params,
            scope,
            true,
        )
    }
}

impl SkeletonBuilder {
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
fn field_init_hir(
    value: &Expression<'_>,
    field_ty: Option<TypeId>,
    types: &TypeTable,
    span: CoreSpan,
) -> Option<HirExpr> {
    match value {
        Expression::NumericLiteral(n) => {
            if field_ty.is_some_and(|t| matches!(types.resolve(t), Some(Type::F64 | Type::F32))) {
                Some(HirExpr::Float(n.value.to_bits(), span))
            } else {
                Some(crate::ops::number_to_hir(n.value, span))
            }
        }
        Expression::BooleanLiteral(b) => Some(HirExpr::Bool(b.value, span)),
        Expression::StringLiteral(s) => Some(HirExpr::String(Atom::from(s.value.as_str()), span)),
        _ => None,
    }
}

fn collect_class_parents(decls: &[HirDecl]) -> HashMap<TypeId, TypeId> {
    fn collect_names(decls: &[HirDecl], out: &mut HashMap<Atom, TypeId>) {
        for decl in decls {
            match decl {
                HirDecl::Class(class) => {
                    out.insert(class.name.clone(), class.ty);
                }
                HirDecl::Namespace { members, .. } => collect_names(members, out),
                _ => {}
            }
        }
    }
    fn collect_parents(
        decls: &[HirDecl],
        by_name: &HashMap<Atom, TypeId>,
        out: &mut HashMap<TypeId, TypeId>,
    ) {
        for decl in decls {
            match decl {
                HirDecl::Class(class) => {
                    if let Some(base) = &class.extends
                        && let Some(base_ty) = by_name.get(base)
                    {
                        out.insert(class.ty, *base_ty);
                    }
                }
                HirDecl::Namespace { members, .. } => collect_parents(members, by_name, out),
                _ => {}
            }
        }
    }
    let mut by_name: HashMap<Atom, TypeId> = HashMap::new();
    collect_names(decls, &mut by_name);
    let mut out: HashMap<TypeId, TypeId> = HashMap::new();
    collect_parents(decls, &by_name, &mut out);
    out
}

fn field_init_reads_the_instance(expr: &HirExpr) -> bool {
    struct ReceiverScan {
        found: bool,
    }
    impl Visitor for ReceiverScan {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if self.found {
                return;
            }
            if matches!(expr, HirExpr::Local { id, .. } if id.raw() == FIELD_INIT_RECEIVER_ID)
                || matches!(expr, HirExpr::Global { name, .. } if name.as_str() == "this")
            {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }
    let mut scan = ReceiverScan { found: false };
    scan.visit_expr(expr);
    scan.found
}

fn is_field_initializer_capture(field_name: &Atom) -> bool {
    field_name.as_str().is_empty()
}

fn collect_enclosing_local_reads(
    expr: &HirExpr,
    scope: &BodyScope,
    out: &mut Vec<(Atom, LocalId, TypeId, CoreSpan)>,
) {
    struct ReadScan<'a> {
        scope: &'a BodyScope,
        out: &'a mut Vec<(Atom, LocalId, TypeId, CoreSpan)>,
    }
    impl Visitor for ReadScan<'_> {
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Local { id, ty, span } = expr
                && id.raw() != FIELD_INIT_RECEIVER_ID
                && !self
                    .out
                    .iter()
                    .any(|(_, existing, _, _)| existing.raw() == id.raw())
                && let Some(name) = self.scope.name_of_local(*id)
            {
                self.out.push((Atom::from(name), *id, *ty, *span));
            }
            walk_expr(self, expr);
        }
    }
    ReadScan { scope, out }.visit_expr(expr);
}

fn static_field_global_name(class: &str, field: &str, ty: TypeId) -> Atom {
    Atom::from(format!(
        "{}__{}_{}",
        rust_ident_escape(class),
        rust_ident_escape(field),
        ty.raw()
    ))
}

fn class_key_hir_name(class_ty: TypeId, key: &oxc_ast::ast::PropertyKey<'_>) -> Option<Atom> {
    if let Some(private) = key.private_name() {
        return Some(private_member_hir_name(class_ty, private.as_str()));
    }
    key.static_name().map(|name| Atom::from(name.as_ref()))
}

fn collect_own_method_names(class: &Class<'_>, class_ty: TypeId) -> Vec<Atom> {
    let mut names = Vec::new();
    for element in &class.body.body {
        let oxc_ast::ast::ClassElement::MethodDefinition(md) = element else {
            continue;
        };
        if md.r#static {
            continue;
        }
        let Some(base) = class_key_hir_name(class_ty, &md.key) else {
            continue;
        };
        let name = match md.kind {
            MethodDefinitionKind::Get => Atom::from(format!("__get_{base}")),
            MethodDefinitionKind::Set => Atom::from(format!("__set_{base}")),
            _ => base,
        };
        names.push(name);
    }
    names
}

fn collect_private_members(class: &Class<'_>, class_ty: TypeId) -> HashMap<Atom, PrivateMember> {
    let mut public_names: HashSet<Atom> = HashSet::new();
    for element in &class.body.body {
        let (key, kind) = match element {
            oxc_ast::ast::ClassElement::MethodDefinition(md) => (&md.key, Some(md.kind)),
            oxc_ast::ast::ClassElement::PropertyDefinition(p) => (&p.key, None),
            _ => continue,
        };
        if key.private_name().is_some() {
            continue;
        }
        match kind {
            Some(kind) => {
                if let Some(base) = class_key_hir_name(class_ty, key) {
                    let name = match kind {
                        MethodDefinitionKind::Get => format!("__get_{base}"),
                        MethodDefinitionKind::Set => format!("__set_{base}"),
                        _ => base.to_string(),
                    };
                    public_names.insert(Atom::from(name));
                }
            }
            None => {
                if let Some(name) = key.static_name() {
                    public_names.insert(Atom::from(name.as_ref()));
                }
            }
        }
    }
    let mut members = HashMap::new();
    for element in &class.body.body {
        let (key, is_static, brand_is_field) = match element {
            oxc_ast::ast::ClassElement::PropertyDefinition(p) => (&p.key, p.r#static, true),
            oxc_ast::ast::ClassElement::MethodDefinition(md) => (&md.key, md.r#static, false),
            _ => continue,
        };
        let Some(private) = key.private_name() else {
            continue;
        };
        let private_name = private.as_str();
        let name = Atom::from(private_name);
        if let std::collections::hash_map::Entry::Vacant(slot) = members.entry(name) {
            let mut lowered = private_member_hir_name(class_ty, private_name);
            while public_names.contains(&lowered) {
                lowered = Atom::from(format!("{}__dup", lowered.as_str()));
            }
            public_names.insert(lowered.clone());
            slot.insert(PrivateMember {
                lowered,
                is_static,
                brand_is_field,
            });
        }
    }
    members
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
