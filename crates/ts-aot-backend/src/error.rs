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
    UnrepresentableStructGlobalDefault {
        name: String,
        initializer: &'static str,
    },
}

impl BackendError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UndecidableNullishComparison { code, .. }
            | Self::EscapingThisCapture { code, .. } => code,
            Self::NotImplemented(_)
            | Self::Internal(_)
            | Self::UnrepresentableStructGlobalDefault { .. } => "E0300",
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
            Self::UnrepresentableStructGlobalDefault { name, initializer } => format!(
                "the initializer of struct-typed global `{name}` is {initializer}, which cannot \
                 be represented as the lazy default of that struct; reading the global would \
                 otherwise yield a zero-initialized struct. Change the initializer to a \
                 non-constant expression that produces the struct (for example `new C()`), or \
                 declare the global as `<struct> | null`; a constant initializer is rejected in \
                 module mode as well, so it does not help to move it into the module body that \
                 runs through `__tla_main`"
            ),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndecidableNullishComparison { .. }
            | Self::EscapingThisCapture { .. }
            | Self::UnrepresentableStructGlobalDefault { .. } => {
                write!(f, "{}: {}", self.code(), self.message())
            }
            _ => write!(f, "{}", self.message()),
        }
    }
}

impl std::error::Error for BackendError {}
