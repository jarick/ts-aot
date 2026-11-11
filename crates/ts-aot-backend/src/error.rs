use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    NotImplemented(&'static str),
    Internal(String),
    /// A construct whose meaning the AOT representation cannot express, so the
    /// emission would silently miscompile it. `code` is the pass-level
    /// diagnostic code the same situation uses (`P0019` for a nullish-kind
    /// mismatch, `P0020` for a comparison that admits both kinds, `P0022` for a
    /// comparison whose optional inner types cannot be compared); the driver
    /// reports the backend error under that code (see [`BackendError::code`]).
    UndecidableNullishComparison {
        code: &'static str,
        message: String,
    },
}

impl BackendError {
    /// The diagnostic code the driver reports this error under.
    ///
    /// A construct that maps to an established pass-level diagnostic keeps that
    /// code (`P0019`/`P0020`/`P0022`), so tooling and tests can match on it; the
    /// generic emission failures stay `E0300`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UndecidableNullishComparison { code, .. } => code,
            Self::NotImplemented(_) | Self::Internal(_) => "E0300",
        }
    }

    /// The diagnostic message, without the code prefix [`fmt::Display`] adds.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotImplemented(reason) => format!(
                "ts-aot-backend emission is not yet implemented for non-empty MIR: {reason}"
            ),
            Self::Internal(msg) => format!("ts-aot-backend internal error: {msg}"),
            Self::UndecidableNullishComparison { message, .. } => message.clone(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndecidableNullishComparison { .. } => {
                write!(f, "{}: {}", self.code(), self.message())
            }
            _ => write!(f, "{}", self.message()),
        }
    }
}

impl std::error::Error for BackendError {}
