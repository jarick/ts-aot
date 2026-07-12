use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{Declaration, Program, Statement, TSType, TSTypeName};
use oxc_span::GetSpan;
use ts_aot_core::{Diagnostic, DiagnosticBag, Span as CoreSpan, Type, TypeId};
use ts_aot_ir_hir::HirProgram;

const ALIAS_CYCLE_CODE: &str = "E0401";

pub(crate) struct SkeletonBuilder<'a, 'b> {
    pub(crate) source: &'a str,
    pub(crate) types: &'b mut ts_aot_core::TypeTable,
    pub(crate) diagnostics: &'b mut DiagnosticBag,
    pub(crate) program: &'b mut HirProgram,
    pub(crate) next_generic_param: u32,
    pub(crate) resolved_aliases: HashMap<String, TypeId>,
}

impl SkeletonBuilder<'_, '_> {
    pub(crate) fn build(mut self, program: &Program<'_>) {
        self.pre_resolve_all_aliases(program);
        for stmt in &program.body {
            self.walk_top_level(stmt);
        }
    }

    fn pre_resolve_all_aliases(&mut self, program: &Program<'_>) {
        let names = Self::collect_alias_names(&program.body);
        let alias_set: HashSet<String> = names.iter().cloned().collect();
        let mut visiting: HashSet<String> = HashSet::new();
        for name in &names {
            self.resolve_alias_chain(name, &alias_set, &mut visiting, program);
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
        name: &str,
        alias_set: &HashSet<String>,
        visiting: &mut HashSet<String>,
        program: &Program<'_>,
    ) {
        if self.resolved_aliases.contains_key(name) {
            return;
        }
        if !visiting.insert(name.to_string()) {
            self.record_alias_cycle(name, program);
            let id = self.types.intern(&Type::Error);
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
                if let TSType::TSTypeReference(r) = rhs
                    && let TSTypeName::IdentifierReference(id) = &r.type_name
                    && alias_set.contains(id.name.as_str())
                    && !self.resolved_aliases.contains_key(id.name.as_str())
                {
                    self.resolve_alias_chain(id.name.as_str(), alias_set, visiting, program);
                }
                let target_id = self.resolve_ts_type(Some(rhs));
                visiting.remove(name);
                self.resolved_aliases.insert(name.to_string(), target_id);
                return;
            }
        }
        visiting.remove(name);
        let id = self.types.intern(&Type::Error);
        self.resolved_aliases.insert(name.to_string(), id);
    }

    fn record_alias_cycle(&mut self, name: &str, program: &Program<'_>) {
        let span = Self::find_alias_span(name, program).unwrap_or_else(|| {
            let end = u32::try_from(self.source.len()).unwrap_or(u32::MAX);
            CoreSpan::new(0, end)
        });
        self.diagnostics.push(Diagnostic::warning(
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

    pub(crate) fn walk_top_level(&mut self, stmt: &Statement<'_>) {
        if let Some(decl) = stmt.as_declaration() {
            self.walk_declaration(decl);
        } else if let Some(m) = stmt.as_module_declaration() {
            self.walk_module_declaration(m);
        } else if !matches!(
            stmt,
            Statement::ExpressionStatement(_) | Statement::EmptyStatement(_)
        ) {
            self.report_unsupported(
                "E0300",
                "top-level statement is not supported by foundation pass",
                stmt.span(),
            );
        }
    }
}
