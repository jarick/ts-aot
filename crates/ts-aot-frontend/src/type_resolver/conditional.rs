use std::collections::HashMap;

use oxc_span::GetSpan;
use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::{TypeParamMap, resolve_simple_type};

pub(super) fn resolve_conditional(
    c: &oxc_ast::ast::TSConditionalType<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> TypeId {
    report_unsupported_conditional_types(c, diagnostics);
    resolve_branch(
        &c.check_type,
        "check",
        types,
        aliases,
        type_params,
        diagnostics,
    );
    resolve_branch(
        &c.extends_type,
        "extends",
        types,
        aliases,
        type_params,
        diagnostics,
    );
    resolve_branch(
        &c.true_type,
        "true",
        types,
        aliases,
        type_params,
        diagnostics,
    );
    resolve_branch(
        &c.false_type,
        "false",
        types,
        aliases,
        type_params,
        diagnostics,
    );
    types.intern(&Type::Never)
}

fn resolve_branch(
    branch: &oxc_ast::ast::TSType<'_>,
    label: &str,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) {
    let resolved = resolve_simple_type(
        Some(branch),
        types,
        aliases,
        type_params,
        diagnostics.as_deref_mut(),
    );
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
                     treating whole conditional as `never`"
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
             requires distributive type checker; falling back to `never`",
            core_span_from_oxc(c.span),
        ));
    }
}
