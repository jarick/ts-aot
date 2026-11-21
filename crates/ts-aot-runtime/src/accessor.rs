use crate::host::__ts_aot_throw;
use crate::string::JsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeErrorPayload {
    pub message: String,
}

impl std::fmt::Display for TypeErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TypeError: {}", self.message)
    }
}

pub fn __ts_aot_throw_type_error(message: JsString<'_>) -> ! {
    __ts_aot_throw(TypeErrorPayload {
        message: message.to_string_lossy(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeErrorPayload {
    pub message: String,
}

impl std::fmt::Display for RangeErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RangeError: {}", self.message)
    }
}

pub fn __ts_aot_throw_range_error(message: JsString<'_>) -> ! {
    __ts_aot_throw(RangeErrorPayload {
        message: message.to_string_lossy(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_error_payload_names_the_error() {
        let thrown = std::panic::catch_unwind(|| __ts_aot_throw_type_error(JsString::from("boom")));
        let payload = thrown.expect_err("__ts_aot_throw_type_error must panic");
        let error = payload
            .downcast_ref::<TypeErrorPayload>()
            .expect("the payload must keep the TypeError kind instead of a bare string");
        assert_eq!(error.message, "boom");
        assert_eq!(error.to_string(), "TypeError: boom");
    }

    #[test]
    fn range_error_payload_names_the_error() {
        let thrown =
            std::panic::catch_unwind(|| __ts_aot_throw_range_error(JsString::from("boom")));
        let payload = thrown.expect_err("__ts_aot_throw_range_error must panic");
        let error = payload
            .downcast_ref::<RangeErrorPayload>()
            .expect("the payload must keep the RangeError kind instead of a bare string");
        assert_eq!(error.message, "boom");
        assert_eq!(error.to_string(), "RangeError: boom");
    }
}
