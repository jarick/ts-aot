use oxc_ast::ast::ImportDeclarationSpecifier;
use oxc_ast::ast::ModuleDeclaration;
use oxc_span::GetSpan;
use ts_aot_core::Atom;
use ts_aot_ir_hir::{HirExport, HirImport};

use super::skeleton::SkeletonBuilder;

const UNSUPPORTED_MODULE_CODE: &str = "E0300";

impl SkeletonBuilder<'_, '_> {
    pub(super) fn walk_module_declaration(&mut self, m: &ModuleDeclaration<'_>) {
        match m {
            ModuleDeclaration::ExportNamedDeclaration(e) => {
                self.handle_export_named_declaration_full(m.span(), e);
            }
            ModuleDeclaration::ImportDeclaration(i) => {
                self.handle_import_declaration(m.span(), i);
            }
            ModuleDeclaration::ExportDefaultDeclaration(_) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    "default export is not supported in foundation",
                    m.span(),
                );
            }
            ModuleDeclaration::ExportAllDeclaration(_) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    "`export * from \"...\"` is not supported in foundation",
                    m.span(),
                );
            }
            ModuleDeclaration::TSExportAssignment(_) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    "TS export assignment is not supported in foundation",
                    m.span(),
                );
            }
            ModuleDeclaration::TSNamespaceExportDeclaration(_) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    "TS namespace export is not supported in foundation",
                    m.span(),
                );
            }
        }
    }

    fn handle_export_named_declaration_full(
        &mut self,
        span: oxc_span::Span,
        e: &oxc_ast::ast::ExportNamedDeclaration<'_>,
    ) {
        if e.source.is_some() {
            self.report_unsupported(
                UNSUPPORTED_MODULE_CODE,
                "re-export `export { ... } from \"...\"` is not supported in foundation",
                span,
            );
            return;
        }
        if let Some(decl) = e.declaration.as_ref() {
            self.handle_export_named_declaration(decl);
        }
        for specifier in &e.specifiers {
            self.record_export_specifier(specifier);
        }
    }

    fn record_export_specifier(&mut self, specifier: &oxc_ast::ast::ExportSpecifier) {
        let local_str = specifier.local.to_string();
        let exported_str = specifier.exported.to_string();
        let alias = (exported_str != local_str).then(|| Atom::from(exported_str));
        self.program.exports.push(HirExport {
            name: Atom::from(local_str),
            alias,
        });
    }

    fn handle_import_declaration(
        &mut self,
        span: oxc_span::Span,
        i: &oxc_ast::ast::ImportDeclaration<'_>,
    ) {
        let module_atom = Atom::from(i.source.value.as_str());
        let Some(specifiers) = &i.specifiers else {
            self.report_unsupported(
                UNSUPPORTED_MODULE_CODE,
                &format!(
                    "side-effect import `import \"{}\"` is not supported in foundation",
                    i.source.value.as_str(),
                ),
                span,
            );
            return;
        };
        for specifier in specifiers {
            self.record_import_specifier(span, &module_atom, i.source.value.as_str(), specifier);
        }
    }

    fn record_import_specifier(
        &mut self,
        span: oxc_span::Span,
        module: &Atom,
        source: &str,
        specifier: &ImportDeclarationSpecifier,
    ) {
        match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                let name = Atom::from(s.imported.to_string());
                let local = Atom::from(s.local.name.as_str());
                let alias = if local == name { None } else { Some(local) };
                self.program.imports.push(HirImport {
                    module: module.clone(),
                    name,
                    alias,
                });
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    &format!(
                        "default import `import {} from \"{}\"` is not supported in foundation",
                        s.local.name.as_str(),
                        source,
                    ),
                    span,
                );
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                self.report_unsupported(
                    UNSUPPORTED_MODULE_CODE,
                    &format!(
                        "namespace import `import * as {} from \"{}\"` is not supported in foundation",
                        s.local.name.as_str(),
                        source,
                    ),
                    span,
                );
            }
        }
    }
}
