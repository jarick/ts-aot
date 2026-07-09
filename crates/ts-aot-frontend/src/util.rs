use oxc_ast::ast::BindingPatternKind;
use oxc_span::{Atom as OxcAtom, SourceType, Span as OxcSpan};
use ts_aot_core::Span as CoreSpan;

pub(crate) fn binding_pattern_name(pattern: &oxc_ast::ast::BindingPattern<'_>) -> Option<OxcAtom> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(id) => Some(id.name.clone()),
        BindingPatternKind::AssignmentPattern(ap) => binding_pattern_name(&ap.left),
        BindingPatternKind::ObjectPattern(_) | BindingPatternKind::ArrayPattern(_) => None,
    }
}

pub(crate) fn source_type_for(name: &str) -> SourceType {
    SourceType::from_path(name).unwrap_or_else(|_| SourceType::default().with_typescript(true))
}

pub(crate) fn core_span_from_oxc(span: OxcSpan) -> CoreSpan {
    CoreSpan::new(span.start, span.end)
}
