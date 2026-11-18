use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    NotImplemented(&'static str),
    Internal(String),
    UndecidableNullishComparison {
        code: &'static str,
        message: String,
    },
    EscapingThisCapture {
        code: &'static str,
        message: &'static str,
    },
}

impl BackendError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UndecidableNullishComparison { code, .. }
            | Self::EscapingThisCapture { code, .. } => code,
            Self::NotImplemented(_) | Self::Internal(_) => "E0300",
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotImplemented(reason) => format!(
                "ts-aot-backend emission is not yet implemented for non-empty MIR: {reason}"
            ),
            Self::Internal(msg) => format!("ts-aot-backend internal error: {msg}"),
            Self::UndecidableNullishComparison { message, .. } => message.clone(),
            Self::EscapingThisCapture { message, .. } => (*message).to_string(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndecidableNullishComparison { .. } | Self::EscapingThisCapture { .. } => {
                write!(f, "{}: {}", self.code(), self.message())
            }
            _ => write!(f, "{}", self.message()),
        }
    }
}

impl std::error::Error for BackendError {}
