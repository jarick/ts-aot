use std::collections::HashMap;

use oxc_ast::ast::TSTupleElement;
use oxc_span::GetSpan;
use ts_aot_core::{Diagnostic, DiagnosticBag, Type, TypeId, TypeTable};

use crate::util::core_span_from_oxc;

use super::{TypeParamMap, resolve_simple_type};

pub(super) fn resolve_union(
    types_arena: &[oxc_ast::ast::TSType<'_>],
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    let mut variants: Vec<TypeId> = Vec::with_capacity(types_arena.len());
    for variant in types_arena {
        let id = resolve_simple_type(
            Some(variant),
            types,
            aliases,
            type_params,
            diagnostics.as_deref_mut(),
        )?;
        variants.push(id);
    }
    Some(types.intern(&Type::Union { variants }))
}

pub(super) fn resolve_intersection(
    types_arena: &[oxc_ast::ast::TSType<'_>],
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    let mut parts: Vec<TypeId> = Vec::with_capacity(types_arena.len());
    for part in types_arena {
        let id = resolve_simple_type(
            Some(part),
            types,
            aliases,
            type_params,
            diagnostics.as_deref_mut(),
        )?;
        parts.push(id);
    }
    parts.sort_unstable_by_key(|id| id.raw());
    parts.dedup();
    Some(types.intern(&Type::Intersection { parts }))
}

pub(super) fn resolve_tuple(
    element_types: &[oxc_ast::ast::TSTupleElement<'_>],
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    let mut elements: Vec<TypeId> = Vec::with_capacity(element_types.len());
    for element in element_types {
        if let Some(ty) = element.as_ts_type() {
            let id = resolve_simple_type(
                Some(ty),
                types,
                aliases,
                type_params,
                diagnostics.as_deref_mut(),
            )?;
            elements.push(id);
        } else {
            report_unsupported_tuple_element(element, diagnostics.as_deref_mut());
            elements.push(types.intern(&Type::Error));
        }
    }
    Some(types.intern(&Type::Tuple { elements }))
}

pub(super) fn resolve_array(
    element_type: &oxc_ast::ast::TSType<'_>,
    types: &mut TypeTable,
    aliases: Option<&HashMap<String, TypeId>>,
    type_params: Option<&TypeParamMap>,
    diagnostics: &mut Option<&mut DiagnosticBag>,
) -> Option<TypeId> {
    let id = resolve_simple_type(
        Some(element_type),
        types,
        aliases,
        type_params,
        diagnostics.as_deref_mut(),
    )?;
    Some(types.intern(&Type::Array { element: id }))
}

fn report_unsupported_tuple_element(
    element: &TSTupleElement<'_>,
    diagnostics: Option<&mut DiagnosticBag>,
) {
    if let Some(diag) = diagnostics {
        let (code, message) = match element {
            TSTupleElement::TSRestType(_) => (
                "E0402",
                "rest tuple element `...T` is not supported in Phase 4".to_owned(),
            ),
            TSTupleElement::TSOptionalType(_) => (
                "E0402",
                "optional tuple element `T?` is not supported in Phase 4".to_owned(),
            ),
            _ => (
                "E0402",
                "extended tuple element is not supported in Phase 4".to_owned(),
            ),
        };
        diag.push(Diagnostic::warning(
            code,
            message,
            core_span_from_oxc(element.span()),
        ));
    }
}
