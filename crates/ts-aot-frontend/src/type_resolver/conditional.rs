use oxc_span::GetSpan;
use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::reference::TypeLookup;
use super::resolve_simple_type;

pub(super) fn resolve_conditional(
    c: &oxc_ast::ast::TSConditionalType<'_>,
    types: &mut TypeTable,
    lookup: &TypeLookup<'_>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> TypeId {
    report_unsupported_conditional_types(c, diagnostics);
    resolve_branch(&c.check_type, "check", types, lookup, diagnostics);
    resolve_branch(&c.extends_type, "extends", types, lookup, diagnostics);
    resolve_branch(&c.true_type, "true", types, lookup, diagnostics);
    resolve_branch(&c.false_type, "false", types, lookup, diagnostics);
    // The conditional type is not resolved; `Error` is the same "unknown type"
    // marker the rest of the frontend uses.
    types.intern(&Type::Error)
}

fn resolve_branch(
    branch: &oxc_ast::ast::TSType<'_>,
    label: &str,
    types: &mut TypeTable,
    lookup: &TypeLookup<'_>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) {
    let resolved = resolve_simple_type(Some(branch), types, lookup, diagnostics.as_deref_mut());
    let unresolved = match resolved {
        None => true,
        Some(id) => matches!(types.resolve(id), Some(Type::Error)),
    };
    if unresolved {
        let span = core_span_from_oxc(branch.span());
        if let Some(diag) = diagnostics.as_deref_mut() {
            diag.push(Diagnostic::warning(
                "E0400",
                format!(
                    "conditional type `{label}` branch did not resolve to a concrete type; \
                     treating the whole conditional as an unresolved type"
                ),
                span,
            ));
        }
    }
}

fn report_unsupported_conditional_types(
    c: &oxc_ast::ast::TSConditionalType<'_>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) {
    if let Some(diag) = diagnostics.as_deref_mut() {
        diag.push(Diagnostic::warning(
            "E0407",
            "conditional type `T extends U ? A : B` is not supported in Phase 4 — \
             requires distributive type checker; the whole conditional resolves to the \
             unresolved (error) type",
            core_span_from_oxc(c.span),
        ));
    }
}
