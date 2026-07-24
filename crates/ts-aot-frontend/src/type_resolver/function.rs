use std::collections::HashMap;

use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::{TypeParamMap, resolve_simple_type};

pub(super) fn resolve_function(
    f: &oxc_ast::ast::TSFunctionType<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    report_unsupported_function_features(f, diagnostics);
    if f.params.rest.is_some() {
        return Some(types.intern(&Type::Error));
    }
    let params =
        resolve_function_params(&f.params.items, types, aliases, type_params, diagnostics)?;
    let ret = resolve_function_return(&f.return_type, types, aliases, type_params, diagnostics)?;
    Some(types.intern(&Type::Fn {
        params,
        ret,
        err: None,
    }))
}

fn report_unsupported_function_features(
    f: &oxc_ast::ast::TSFunctionType<'_>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) {
    if let Some(diag) = diagnostics.as_deref_mut() {
        if f.type_parameters.is_some() {
            diag.push(Diagnostic::warning(
                "E0404",
                "function type generics `<T>` are not supported in Phase 4",
                core_span_from_oxc(f.type_parameters.as_ref().map_or(f.span, |tp| tp.span)),
            ));
        }
        if f.this_param.is_some() {
            diag.push(Diagnostic::warning(
                "E0404",
                "function type `this:` parameter is not supported in Phase 4",
                core_span_from_oxc(f.this_param.as_ref().map_or(f.span, |tp| tp.span)),
            ));
        }
        if f.params.rest.is_some() {
            diag.push(Diagnostic::warning(
                "E0404",
                "function type rest parameter `...args: T` is not supported in Phase 4",
                core_span_from_oxc(f.params.rest.as_ref().map_or(f.span, |r| r.span)),
            ));
        }
    }
}

fn resolve_function_params(
    items: &[oxc_ast::ast::FormalParameter<'_>],
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<Vec<TypeId>> {
    let mut out: Vec<TypeId> = Vec::with_capacity(items.len());
    for p in items {
        if let Some(ann) = p.type_annotation.as_deref() {
            let id = resolve_simple_type(
                Some(&ann.type_annotation),
                types,
                aliases,
                type_params,
                diagnostics.as_deref_mut(),
            )?;
            out.push(id);
        } else {
            if let Some(diag) = diagnostics.as_deref_mut() {
                diag.push(Diagnostic::warning(
                    "E0404",
                    "function type parameter without type annotation is not supported in Phase 4 — use `(a: T)` instead",
                    core_span_from_oxc(p.span),
                ));
            }
            out.push(types.intern(&Type::Error));
        }
    }
    Some(out)
}

fn resolve_function_return(
    return_type: &oxc_ast::ast::TSTypeAnnotation<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    resolve_simple_type(
        Some(&return_type.type_annotation),
        types,
        aliases,
        type_params,
        diagnostics.as_deref_mut(),
    )
}
