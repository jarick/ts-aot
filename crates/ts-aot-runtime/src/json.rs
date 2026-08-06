use serde::de::DeserializeOwned;
use serde::ser::Serialize;

use crate::host::__ts_aot_throw;
use crate::string::JsString;

#[must_use]
pub fn __ts_aot_json_parse_string(text: &JsString) -> JsString {
    let json_source: String = match text {
        JsString::Valid(s) => s.clone(),
        JsString::Raw(units) => String::from_utf16_lossy(units),
    };
    match serde_json::from_str::<String>(&json_source) {
        Ok(s) => JsString::Valid(s),
        Err(err) => __ts_aot_throw(format!("JSON.parse failed: {err}")),
    }
}

#[must_use]
pub fn __ts_aot_json_parse<T>(text: &JsString) -> T
where
    T: DeserializeOwned,
{
    let json_source: String = match text {
        JsString::Valid(s) => s.clone(),
        JsString::Raw(units) => String::from_utf16_lossy(units),
    };
    match serde_json::from_str::<T>(&json_source) {
        Ok(value) => value,
        Err(err) => __ts_aot_throw(format!("JSON.parse failed: {err}")),
    }
}

#[must_use]
pub fn __ts_aot_json_stringify_string(value: &JsString) -> JsString {
    let source = serde_json::to_string(value).expect("JsString::Serialize is infallible");
    JsString::Raw(source.encode_utf16().collect())
}

#[must_use]
pub fn __ts_aot_json_stringify<T>(value: &T) -> JsString
where
    T: Serialize,
{
    match serde_json::to_string(value) {
        Ok(s) => JsString::Valid(s),
        Err(err) => __ts_aot_throw(format!("JSON.stringify failed: {err}")),
    }
}
