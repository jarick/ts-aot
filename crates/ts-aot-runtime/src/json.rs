use serde::de::DeserializeOwned;
use serde::ser::Serialize;

use crate::arena::Ctx;
use crate::host::__ts_aot_throw;
use crate::string::JsString;

#[must_use]
pub fn __ts_aot_json_parse_string<'a>(ctx: &Ctx<'a>, text: &JsString<'_>) -> JsString<'a> {
    let json_source: String = match text {
        JsString::Valid(s) => (*s).to_owned(),
        JsString::Raw(units) => String::from_utf16_lossy(units),
    };
    match serde_json::from_str::<String>(&json_source) {
        Ok(s) => JsString::Valid(ctx.arena.alloc_str(&s)),
        Err(err) => __ts_aot_throw(format!("JSON.parse failed: {err}")),
    }
}

#[must_use]
pub fn __ts_aot_json_parse<T>(text: &JsString<'_>) -> T
where
    T: DeserializeOwned,
{
    let json_source: String = match text {
        JsString::Valid(s) => (*s).to_owned(),
        JsString::Raw(units) => String::from_utf16_lossy(units),
    };
    match serde_json::from_str::<T>(&json_source) {
        Ok(value) => value,
        Err(err) => __ts_aot_throw(format!("JSON.parse failed: {err}")),
    }
}

#[must_use]
pub fn __ts_aot_json_stringify_string<'a>(ctx: &Ctx<'a>, value: &JsString<'_>) -> JsString<'a> {
    let source = serde_json::to_string(value).expect("JsString::Serialize is infallible");
    let units: Vec<u16> = source.encode_utf16().collect();
    JsString::Raw(ctx.arena.alloc_slice_copy(&units))
}

#[must_use]
pub fn __ts_aot_json_stringify<'a, T>(ctx: &Ctx<'a>, value: &T) -> JsString<'a>
where
    T: Serialize,
{
    match serde_json::to_string(value) {
        Ok(s) => JsString::Valid(ctx.arena.alloc_str(&s)),
        Err(err) => __ts_aot_throw(format!("JSON.stringify failed: {err}")),
    }
}
