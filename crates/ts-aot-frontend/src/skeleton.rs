use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{Declaration, Expression, Program, Statement, TSType, TSTypeName};
use oxc_span::GetSpan;
use ts_aot_core::{Atom, Diagnostic, DiagnosticBag, Span as CoreSpan, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{
    HirDecl, HirExport, HirExpr, HirFunction, HirParam, HirProgram, HirStmt, Visitor, walk_expr,
};

use crate::decl::is_const_initializer_ast;
use crate::scope::BodyScope;
use crate::type_resolver::TypeParamMap;
use crate::util::{binding_pattern_name, core_span_from_oxc};

const ALIAS_CYCLE_CODE: &str = "E0401";
const UNSUPPORTED_BODY_CODE: &str = "E0500";
const RESERVED_NAME_MAIN_CODE: &str = "E0510";
const RESERVED_NAME_TLA_MAIN_CODE: &str = "E0511";
const RESERVED_NAME_TLA_GENERATED_CODE: &str = "E0512";
pub(crate) const TLA_ONLY_BINDING_CODE: &str = "E0513";

fn reserved_name_reason(name: &str) -> Option<(&'static str, &'static str)> {
    if name == "main" {
        return Some((RESERVED_NAME_MAIN_CODE, "synthesized entry point"));
    }
    if name == "__ts_aot_tla_main" {
        return Some((RESERVED_NAME_TLA_MAIN_CODE, "mangled TLA main entry name"));
    }
    if is_tla_main_reserved_name(name) {
        return Some((
            RESERVED_NAME_TLA_GENERATED_CODE,
            "generated TLA main name namespace",
        ));
    }
    None
}
const TLA_MAIN_RESERVED_PREFIX: &str = "__tla_main_";
const TLA_MAIN_FIXED_NAME: &str = "__tla_main_0";

pub(crate) fn is_tla_main_reserved_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix(TLA_MAIN_RESERVED_PREFIX) else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
}

fn fixed_tla_main_name() -> Atom {
    Atom::from(TLA_MAIN_FIXED_NAME)
}

struct AwaitFinder {
    found: bool,
}

impl Visitor for AwaitFinder {
    fn visit_expr(&mut self, expr: &HirExpr) {
        if self.found {
            return;
        }
        if matches!(expr, HirExpr::Await { .. }) {
            self.found = true;
            return;
        }
        walk_expr(self, expr);
    }
}

fn body_contains_await(body: &[HirStmt]) -> bool {
    let mut finder = AwaitFinder { found: false };
    for stmt in body {
        if finder.found {
            break;
        }
        finder.visit_stmt(stmt);
    }
    finder.found
}

pub(crate) struct SkeletonBuilder {
    pub(crate) source_len: u32,
    pub(crate) next_generic_param: u32,
    pub(crate) next_anon_class_id: u32,
    pub(crate) next_closure_id: u32,
    pub(crate) resolved_aliases: HashMap<String, TypeId>,
    pub(crate) is_generator_stack: Vec<bool>,
    pub(crate) type_param_stack: Vec<TypeParamMap>,
    pub(crate) module: bool,
    pub(crate) tla_main_body: Vec<TlaMainItem>,
    pub(crate) tla_main_name: Atom,
    pub(crate) tla_top_level_scope: BodyScope,
    pub(super) top_level_function_spans: HashMap<Atom, CoreSpan>,
}

pub(crate) struct SkeletonEnv<'b> {
    pub(crate) types: &'b mut TypeTable,
    pub(crate) diagnostics: &'b mut DiagnosticBag,
    pub(crate) program: &'b mut HirProgram,
    pub(crate) next_destructured_id: Cell<u32>,
    pub(crate) tla_only_bindings: HashSet<String>,
}

impl SkeletonEnv<'_> {
    pub(crate) fn error_ty(&mut self) -> TypeId {
        self.types.intern(&Type::Error)
    }

    pub(crate) fn record_export(&mut self, name: &str) {
        self.program.exports.push(HirExport {
            name: Atom::from(name),
            alias: None,
        });
    }

    pub(crate) fn report_unwalked(&mut self, message: &str, span: oxc_span::Span) {
        self.diagnostics.push(Diagnostic::warning(
            UNSUPPORTED_BODY_CODE,
            message,
            core_span_from_oxc(span),
        ));
    }
}

pub(crate) enum TlaMainItem {
    Let(HirStmt),
    Expr(HirExpr),
}

impl SkeletonBuilder {
    pub(crate) fn new(source_len: u32, module: bool) -> Self {
        let tla_main_name = if module {
            fixed_tla_main_name()
        } else {
            Atom::from("")
        };
        Self {
            source_len,
            next_generic_param: 0,
            next_anon_class_id: 0,
            next_closure_id: 0,
            resolved_aliases: HashMap::new(),
            is_generator_stack: Vec::new(),
            type_param_stack: Vec::new(),
            module,
            tla_main_body: Vec::new(),
            tla_main_name,
            tla_top_level_scope: BodyScope::new(0),
            top_level_function_spans: HashMap::new(),
        }
    }

    pub(crate) fn build(mut self, senv: &mut SkeletonEnv<'_>, program: &Program<'_>) {
        self.pre_resolve_all_aliases(senv, program);
        self.pre_collect_tla_only_bindings(senv, program);
        for stmt in &program.body {
            self.walk_top_level(senv, stmt);
        }
        self.check_reserved_names(senv);
        self.finalize_tla_main(senv);
    }

    fn pre_collect_tla_only_bindings(&mut self, senv: &mut SkeletonEnv, program: &Program<'_>) {
        if !self.module {
            return;
        }
        for stmt in &program.body {
            let decl = if let Some(d) = stmt.as_declaration() {
                Some(d)
            } else if let Some(m) = stmt.as_module_declaration()
                && let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
            {
                e.declaration.as_ref()
            } else {
                None
            };
            let Some(Declaration::VariableDeclaration(v)) = decl else {
                continue;
            };
            for declarator in &v.declarations {
                if let Some(ident) = binding_pattern_name(&declarator.id)
                    && let Some(init) = declarator.init.as_ref()
                    && !is_const_initializer_ast(init)
                {
                    senv.tla_only_bindings.insert(ident.to_string());
                }
            }
        }
    }

    fn check_reserved_names(&mut self, senv: &mut SkeletonEnv) {
        if !self.module {
            return;
        }
        let end = self.source_len;
        let fallback_span = CoreSpan::new(0, end);
        for decl in &senv.program.declarations {
            let (name, kind, span) = match decl {
                HirDecl::Function(f) => {
                    let span = self
                        .top_level_function_spans
                        .get(&f.name)
                        .copied()
                        .unwrap_or(fallback_span);
                    (f.name.as_str(), "function", span)
                }
                HirDecl::Global { name, .. } => (name.as_str(), "global", fallback_span),
                _ => continue,
            };
            if let Some((code, reason)) = reserved_name_reason(name) {
                senv.diagnostics.push(Diagnostic::error(
                    code,
                    format!(
                        "{kind} `{name}` is reserved in module mode (collides with {reason}); rename or remove"
                    ),
                    span,
                ));
            }
        }
    }

    fn finalize_tla_main(&mut self, senv: &mut SkeletonEnv) {
        if !self.module {
            return;
        }
        let items = std::mem::take(&mut self.tla_main_body);
        let unit_ty = senv.types.intern(&Type::Void);
        let mut body: Vec<HirStmt> = Vec::with_capacity(items.len());
        for item in items {
            match item {
                TlaMainItem::Let(stmt) => body.push(stmt),
                TlaMainItem::Expr(expr) => body.push(HirStmt::Expr { expr }),
            }
        }
        if body.is_empty() {
            return;
        }
        let is_async = body_contains_await(&body);
        let tla_main = HirDecl::Function(HirFunction {
            name: self.tla_main_name.clone(),
            params: Vec::<HirParam>::new(),
            ret: unit_ty,
            throws: None,
            body,
            is_async,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        });
        senv.program.declarations.push(tla_main);
        senv.program.tla_main_name = Some(self.tla_main_name.clone());
    }

    fn pre_resolve_all_aliases(&mut self, senv: &mut SkeletonEnv, program: &Program<'_>) {
        let names = Self::collect_alias_names(&program.body);
        let alias_set: HashSet<String> = names.iter().cloned().collect();
        let mut visiting: HashSet<String> = HashSet::new();
        for name in &names {
            self.resolve_alias_chain(senv, name, &alias_set, &mut visiting, program);
        }
    }

    fn collect_alias_names(stmts: &[Statement<'_>]) -> Vec<String> {
        let mut names = Vec::new();
        for stmt in stmts {
            if let Some(decl) = stmt.as_declaration()
                && let Declaration::TSTypeAliasDeclaration(a) = decl
            {
                names.push(a.id.name.to_string());
            } else if let Some(m) = stmt.as_module_declaration()
                && let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
            {
                names.push(a.id.name.to_string());
            }
        }
        names
    }

    fn resolve_alias_chain(
        &mut self,
        senv: &mut SkeletonEnv,
        name: &str,
        alias_set: &HashSet<String>,
        visiting: &mut HashSet<String>,
        program: &Program<'_>,
    ) {
        if self.resolved_aliases.contains_key(name) {
            return;
        }
        if !visiting.insert(name.to_string()) {
            self.record_alias_cycle(senv, name, program);
            let id = senv.types.intern(&Type::Error);
            self.resolved_aliases.insert(name.to_string(), id);
            return;
        }
        for stmt in &program.body {
            let annotation_opt: Option<&TSType<'_>> = if let Some(decl) = stmt.as_declaration() {
                if let Declaration::TSTypeAliasDeclaration(a) = decl
                    && a.id.name == name
                {
                    Some(&a.type_annotation)
                } else {
                    None
                }
            } else if let Some(m) = stmt.as_module_declaration() {
                if let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                    && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
                    && a.id.name == name
                {
                    Some(&a.type_annotation)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(rhs) = annotation_opt {
                self.pre_resolve_aliases_in_type(senv, rhs, alias_set, visiting, program);
                let target_id = self.resolve_ts_type(senv, Some(rhs));
                visiting.remove(name);
                self.resolved_aliases.insert(name.to_string(), target_id);
                return;
            }
        }
        visiting.remove(name);
        let id = senv.types.intern(&Type::Error);
        self.resolved_aliases.insert(name.to_string(), id);
    }

    fn pre_resolve_aliases_in_type(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: &TSType<'_>,
        alias_set: &HashSet<String>,
        visiting: &mut HashSet<String>,
        program: &Program<'_>,
    ) {
        match ty {
            TSType::TSTypeReference(r) => {
                if let TSTypeName::IdentifierReference(id) = &r.type_name {
                    let dep_name = id.name.as_str();
                    if alias_set.contains(dep_name) && !self.resolved_aliases.contains_key(dep_name)
                    {
                        self.resolve_alias_chain(senv, dep_name, alias_set, visiting, program);
                    }
                }
                if let Some(type_args) = &r.type_arguments {
                    for arg in &type_args.params {
                        self.pre_resolve_aliases_in_type(senv, arg, alias_set, visiting, program);
                    }
                }
            }
            TSType::TSUnionType(u) => {
                for variant in &u.types {
                    self.pre_resolve_aliases_in_type(senv, variant, alias_set, visiting, program);
                }
            }
            TSType::TSIntersectionType(i) => {
                for part in &i.types {
                    self.pre_resolve_aliases_in_type(senv, part, alias_set, visiting, program);
                }
            }
            TSType::TSTupleType(t) => {
                for element in &t.element_types {
                    if let Some(ty) = element.as_ts_type() {
                        self.pre_resolve_aliases_in_type(senv, ty, alias_set, visiting, program);
                    }
                }
            }
            TSType::TSArrayType(a) => {
                self.pre_resolve_aliases_in_type(
                    senv,
                    &a.element_type,
                    alias_set,
                    visiting,
                    program,
                );
            }
            TSType::TSFunctionType(f) => {
                for p in &f.params.items {
                    if let Some(ann) = p.type_annotation.as_deref() {
                        self.pre_resolve_aliases_in_type(
                            senv,
                            &ann.type_annotation,
                            alias_set,
                            visiting,
                            program,
                        );
                    }
                }
                if let Some(rest) = &f.params.rest
                    && let Some(ann) = rest.type_annotation.as_deref()
                {
                    self.pre_resolve_aliases_in_type(
                        senv,
                        &ann.type_annotation,
                        alias_set,
                        visiting,
                        program,
                    );
                }
                self.pre_resolve_aliases_in_type(
                    senv,
                    &f.return_type.type_annotation,
                    alias_set,
                    visiting,
                    program,
                );
            }
            _ => {}
        }
    }

    fn record_alias_cycle(&mut self, senv: &mut SkeletonEnv, name: &str, program: &Program<'_>) {
        let span = Self::find_alias_span(name, program).unwrap_or_else(|| {
            let end = self.source_len;
            CoreSpan::new(0, end)
        });
        senv.diagnostics.push(Diagnostic::warning(
            ALIAS_CYCLE_CODE,
            format!("type alias `{name}` participates in a recursive cycle"),
            span,
        ));
    }

    fn find_alias_span(name: &str, program: &Program<'_>) -> Option<CoreSpan> {
        for stmt in &program.body {
            let span = stmt.span();
            if let Some(decl) = stmt.as_declaration()
                && let Declaration::TSTypeAliasDeclaration(a) = decl
                && a.id.name == name
            {
                return Some(CoreSpan::new(span.start, span.end));
            }
            if let Some(m) = stmt.as_module_declaration()
                && let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
                && a.id.name == name
            {
                return Some(CoreSpan::new(span.start, span.end));
            }
        }
        None
    }

    pub(crate) fn walk_top_level(&mut self, senv: &mut SkeletonEnv, stmt: &Statement<'_>) {
        if let Some(decl) = stmt.as_declaration() {
            self.walk_declaration(senv, decl);
        } else if let Some(m) = stmt.as_module_declaration() {
            self.walk_module_declaration(senv, m);
        } else if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if self.module {
                self.walk_tla_expression_statement(senv, expr_stmt);
            }
        } else if !matches!(stmt, Statement::EmptyStatement(_)) {
            senv.report_unsupported(
                "E0300",
                "top-level statement is not supported by foundation pass",
                stmt.span(),
            );
        }
    }

    fn walk_tla_expression_statement(
        &mut self,
        senv: &mut SkeletonEnv,
        expr_stmt: &oxc_ast::ast::ExpressionStatement<'_>,
    ) {
        let expr = self.walk_top_level_expr(senv, &expr_stmt.expression);
        self.tla_main_body.push(TlaMainItem::Expr(expr));
    }

    fn walk_top_level_expr(&mut self, senv: &mut SkeletonEnv, e: &Expression<'_>) -> HirExpr {
        self.is_generator_stack.push(false);
        let mut scope = std::mem::replace(&mut self.tla_top_level_scope, BodyScope::new(0));
        let result = self.walk_expr(senv, e, &mut scope);
        self.tla_top_level_scope = scope;
        self.is_generator_stack.pop();
        result
    }

    pub(crate) fn walk_top_level_init(
        &mut self,
        senv: &mut SkeletonEnv,
        e: &Expression<'_>,
    ) -> HirExpr {
        self.walk_top_level_expr(senv, e)
    }

    pub(crate) fn merged_type_params(&self) -> Option<TypeParamMap> {
        if self.type_param_stack.is_empty() {
            return None;
        }
        let mut merged = TypeParamMap::new();
        for map in &self.type_param_stack {
            for (k, v) in map.iter_bindings() {
                merged.bind(k, v);
            }
        }
        Some(merged)
    }
}
