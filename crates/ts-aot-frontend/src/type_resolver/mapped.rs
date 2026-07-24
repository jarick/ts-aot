use std::collections::HashMap;

use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::{TypeParamMap, resolve_simple_type};

pub(super) fn resolve_mapped(
    m: &oxc_ast::ast::TSMappedType<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    report_unsupported_mapped_features(m, diagnostics);
    if let Some(name_ty) = &m.name_type {
        resolve_simple_type(
            Some(name_ty),
            types,
            aliases,
            type_params,
            diagnostics.as_deref_mut(),
        )
        .unwrap_or_else(|| types.intern(&Type::Error));
    }
    resolve_simple_type(
        Some(&m.constraint),
        types,
        aliases,
        type_params,
        diagnostics.as_deref_mut(),
    )
    .unwrap_or_else(|| types.intern(&Type::Error));
    if let Some(value_ty) = &m.type_annotation {
        resolve_simple_type(
            Some(value_ty),
            types,
            aliases,
            type_params,
            diagnostics.as_deref_mut(),
        )
        .unwrap_or_else(|| types.intern(&Type::Error));
    }
    Some(types.intern(&Type::Error))
}

fn report_unsupported_mapped_features(
    m: &oxc_ast::ast::TSMappedType<'_>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) {
    if let Some(diag) = diagnostics.as_deref_mut() {
        diag.push(Diagnostic::warning(
            "E0406",
            "mapped type `{[K in keyof T]: V}` is not supported in Phase 4 — \
             requires monomorphization per key type, deferred to a later phase",
            core_span_from_oxc(m.span),
        ));
    }
}
