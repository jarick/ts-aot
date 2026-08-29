use oxc_ast::ast::BindingPattern;
use oxc_span::{SourceType, Span as OxcSpan};
use ts_aot_core::Span as CoreSpan;

pub(crate) fn binding_pattern_name(pattern: &BindingPattern<'_>) -> Option<oxc_str::CompactStr> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_compact_str()),
        BindingPattern::AssignmentPattern(ap) => binding_pattern_name(&ap.left),
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => None,
    }
}

pub(crate) fn source_type_for(name: &str, module: bool) -> SourceType {
    SourceType::from_path(name).map_or_else(
        |_| {
            let mut s = SourceType::default().with_typescript(true);
            if module {
                s = s.with_module(true);
            }
            s
        },
        |s| if module { s.with_module(true) } else { s },
    )
}

pub(crate) fn core_span_from_oxc(span: OxcSpan) -> CoreSpan {
    CoreSpan::new(span.start, span.end)
}
