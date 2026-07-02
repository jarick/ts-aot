use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    NotImplemented,
    Internal(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented => {
                f.write_str("ts-aot-backend emission is not yet implemented for non-empty MIR")
            }
            Self::Internal(msg) => write!(f, "ts-aot-backend internal error: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {}
