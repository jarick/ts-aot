use ts_aot_core::{Atom, Span, TypeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassError {
    UnresolvedSymbol {
        name: Atom,
        span: Span,
    },
    UnresolvedType {
        ty: TypeId,
        span: Span,
    },
    DuplicateDefinition {
        name: Atom,
        first: Span,
        second: Span,
    },
    ArityMismatch {
        expected: usize,
        got: usize,
        span: Span,
    },
    NotYetImplemented {
        feature: &'static str,
        span: Span,
    },
    UnsupportedSyntax {
        what: &'static str,
        span: Span,
    },
    Internal {
        message: String,
    },
}

impl PassError {
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::UnresolvedSymbol { span, .. }
            | Self::UnresolvedType { span, .. }
            | Self::ArityMismatch { span, .. }
            | Self::NotYetImplemented { span, .. }
            | Self::UnsupportedSyntax { span, .. } => Some(*span),
            Self::DuplicateDefinition { second, .. } => Some(*second),
            Self::Internal { .. } => None,
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnresolvedSymbol { .. } => "P0001",
            Self::UnresolvedType { .. } => "P0002",
            Self::DuplicateDefinition { .. } => "P0003",
            Self::ArityMismatch { .. } => "P0004",
            Self::NotYetImplemented { .. } => "P0005",
            Self::UnsupportedSyntax { .. } => "P0006",
            Self::Internal { .. } => "P0099",
        }
    }
}

impl std::fmt::Display for PassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedSymbol { name, .. } => {
                write!(f, "unresolved symbol {name}")
            }
            Self::UnresolvedType { ty, .. } => write!(f, "unresolved type #{}", ty.raw()),
            Self::DuplicateDefinition { name, .. } => {
                write!(f, "duplicate definition of symbol {name}")
            }
            Self::ArityMismatch { expected, got, .. } => {
                write!(f, "expected {expected} arguments, got {got}")
            }
            Self::NotYetImplemented { feature, .. } => {
                write!(f, "feature not yet implemented: {feature}")
            }
            Self::UnsupportedSyntax { what, .. } => write!(f, "unsupported syntax: {what}"),
            Self::Internal { message } => write!(f, "internal pass error: {message}"),
        }
    }
}

impl std::error::Error for PassError {}

pub type PassResult<T> = Result<T, PassError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_symbol_carries_span_and_code() {
        let err = PassError::UnresolvedSymbol {
            name: Atom::new_inline("7"),
            span: Span::new(10, 12),
        };
        assert_eq!(err.span(), Some(Span::new(10, 12)));
        assert_eq!(err.code(), "P0001");
    }

    #[test]
    fn unresolved_type_carries_span_and_code() {
        let err = PassError::UnresolvedType {
            ty: TypeId::from_raw(3),
            span: Span::new(0, 4),
        };
        assert_eq!(err.span(), Some(Span::new(0, 4)));
        assert_eq!(err.code(), "P0002");
    }

    #[test]
    fn duplicate_definition_uses_second_span() {
        let err = PassError::DuplicateDefinition {
            name: Atom::new_inline("1"),
            first: Span::new(0, 1),
            second: Span::new(20, 25),
        };
        assert_eq!(err.span(), Some(Span::new(20, 25)));
        assert_eq!(err.code(), "P0003");
    }

    #[test]
    fn arity_mismatch_carries_counts() {
        let err = PassError::ArityMismatch {
            expected: 3,
            got: 5,
            span: Span::new(0, 8),
        };
        assert_eq!(err.span(), Some(Span::new(0, 8)));
        assert_eq!(err.code(), "P0004");
        assert_eq!(format!("{err}"), "expected 3 arguments, got 5");
    }

    #[test]
    fn not_yet_implemented_includes_feature() {
        let err = PassError::NotYetImplemented {
            feature: "for-in",
            span: Span::new(5, 10),
        };
        assert_eq!(err.code(), "P0005");
        assert_eq!(format!("{err}"), "feature not yet implemented: for-in");
    }

    #[test]
    fn unsupported_syntax_includes_what() {
        let err = PassError::UnsupportedSyntax {
            what: "with statement",
            span: Span::new(0, 4),
        };
        assert_eq!(err.code(), "P0006");
        assert_eq!(format!("{err}"), "unsupported syntax: with statement");
    }

    #[test]
    fn internal_has_no_span() {
        let err = PassError::Internal {
            message: "boom".to_owned(),
        };
        assert_eq!(err.span(), None);
        assert_eq!(err.code(), "P0099");
        assert_eq!(format!("{err}"), "internal pass error: boom");
    }

    #[test]
    fn error_codes_are_distinct() {
        let errs = [
            PassError::UnresolvedSymbol {
                name: Atom::new_inline("0"),
                span: Span::new(0, 1),
            },
            PassError::UnresolvedType {
                ty: TypeId::from_raw(0),
                span: Span::new(0, 1),
            },
            PassError::DuplicateDefinition {
                name: Atom::new_inline("0"),
                first: Span::new(0, 1),
                second: Span::new(2, 3),
            },
            PassError::ArityMismatch {
                expected: 0,
                got: 1,
                span: Span::new(0, 1),
            },
            PassError::NotYetImplemented {
                feature: "x",
                span: Span::new(0, 1),
            },
            PassError::UnsupportedSyntax {
                what: "x",
                span: Span::new(0, 1),
            },
            PassError::Internal {
                message: "x".to_owned(),
            },
        ];
        let codes: Vec<&str> = errs.iter().map(PassError::code).collect();
        let unique: std::collections::HashSet<&str> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn pass_result_ok_and_err() {
        let ok: PassResult<u32> = Ok(7);
        let err: PassResult<u32> = Err(PassError::Internal {
            message: "x".to_owned(),
        });
        assert!(matches!(ok, Ok(7)));
        assert!(err.is_err());
    }
}
