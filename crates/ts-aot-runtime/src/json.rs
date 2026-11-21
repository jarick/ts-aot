use std::borrow::Cow;

use serde::de::DeserializeOwned;
use serde::ser::Serialize;

use bumpalo::collections::Vec as BumpVec;

use crate::arena::Ctx;
use crate::host::__ts_aot_throw;
use crate::string::JsString;

fn json_source_str<'a>(text: &JsString<'a>) -> Cow<'a, str> {
    match *text {
        JsString::Valid(s) => Cow::Borrowed(s),
        JsString::Raw(units) => Cow::Owned(String::from_utf16_lossy(units)),
    }
}

#[must_use]
pub fn __ts_aot_json_parse_string<'a>(ctx: &Ctx<'a>, text: &JsString<'_>) -> JsString<'a> {
    let json_source = json_source_str(text);
    match serde_json::from_str::<String>(json_source.as_ref()) {
        Ok(s) => JsString::Valid(ctx.arena.alloc_str(&s)),
        Err(err) => __ts_aot_throw(format!("JSON.parse failed: {err}")),
    }
}

#[must_use]
pub fn __ts_aot_json_parse_vec_string<'a>(
    ctx: &Ctx<'a>,
    text: &JsString<'_>,
) -> BumpVec<'a, JsString<'a>> {
    let json_source = json_source_str(text);
    match serde_json::from_str::<Vec<String>>(json_source.as_ref()) {
        Ok(values) => BumpVec::from_iter_in(
            values
                .iter()
                .map(|s| JsString::Valid(ctx.arena.alloc_str(s))),
            ctx.arena,
        ),
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
    let mut units = BumpVec::with_capacity_in(source.encode_utf16().count(), ctx.arena);
    units.extend(source.encode_utf16());
    JsString::Raw(units.into_bump_slice())
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
