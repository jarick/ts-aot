use std::any::Any;
use std::fmt;

use crate::arena::Ctx;
use crate::promise::__ts_aot_panic_message;
use crate::string::JsString;

#[derive(Clone, Debug, PartialEq)]
pub enum CaughtValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
}

impl CaughtValue {
    #[must_use]
    pub fn typeof_str(&self) -> &'static str {
        match self {
            CaughtValue::Undefined => "undefined",
            CaughtValue::Null => "object",
            CaughtValue::Bool(_) => "boolean",
            CaughtValue::Number(_) => "number",
            CaughtValue::Str(_) => "string",
        }
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            CaughtValue::Undefined | CaughtValue::Null => false,
            CaughtValue::Bool(b) => *b,
            CaughtValue::Number(n) => *n != 0.0 && !n.is_nan(),
            CaughtValue::Str(s) => !s.is_empty(),
        }
    }

    #[must_use]
    pub fn to_js_string<'a>(&self, ctx: &Ctx<'a>) -> JsString<'a> {
        JsString::Valid(ctx.arena.alloc_str(&self.to_string()))
    }
}

impl fmt::Display for CaughtValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaughtValue::Undefined => f.write_str("undefined"),
            CaughtValue::Null => f.write_str("null"),
            CaughtValue::Bool(b) => write!(f, "{b}"),
            CaughtValue::Number(n) => {
                if n.is_nan() {
                    f.write_str("NaN")
                } else if n.is_infinite() {
                    f.write_str(if n.is_sign_negative() {
                        "-Infinity"
                    } else {
                        "Infinity"
                    })
                } else {
                    write!(f, "{n}")
                }
            }
            CaughtValue::Str(s) => f.write_str(s),
        }
    }
}

macro_rules! impl_from_int {
    ($($t:ty),*) => {
        $(
            impl From<$t> for CaughtValue {
                fn from(value: $t) -> Self {
                    use num_traits::ToPrimitive;
                    CaughtValue::Number(value.to_f64().unwrap_or(f64::NAN))
                }
            }
        )*
    };
}

impl_from_int!(i8, i16, i32, i64, u8, u16, u32, u64);

impl From<f32> for CaughtValue {
    fn from(value: f32) -> Self {
        CaughtValue::Number(f64::from(value))
    }
}

impl From<f64> for CaughtValue {
    fn from(value: f64) -> Self {
        CaughtValue::Number(value)
    }
}

impl From<bool> for CaughtValue {
    fn from(value: bool) -> Self {
        CaughtValue::Bool(value)
    }
}

impl From<JsString<'_>> for CaughtValue {
    fn from(value: JsString<'_>) -> Self {
        CaughtValue::Str(value.to_string_lossy())
    }
}

impl From<&str> for CaughtValue {
    fn from(value: &str) -> Self {
        CaughtValue::Str(value.to_owned())
    }
}

impl From<String> for CaughtValue {
    fn from(value: String) -> Self {
        CaughtValue::Str(value)
    }
}

#[must_use]
pub fn __ts_aot_caught_eq<T: Into<CaughtValue>>(caught: &CaughtValue, other: T) -> bool {
    *caught == other.into()
}

#[must_use]
pub fn __ts_aot_caught_eq_dyn<T: 'static>(caught: &CaughtValue, other: &T) -> bool {
    use num_traits::ToPrimitive;
    let any = other as &dyn Any;
    let other_value = if let Some(v) = any.downcast_ref::<bool>() {
        CaughtValue::Bool(*v)
    } else if let Some(v) = any.downcast_ref::<f64>() {
        CaughtValue::Number(*v)
    } else if let Some(v) = any.downcast_ref::<f32>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<i64>() {
        CaughtValue::Number(v.to_f64().unwrap_or(f64::NAN))
    } else if let Some(v) = any.downcast_ref::<u64>() {
        CaughtValue::Number(v.to_f64().unwrap_or(f64::NAN))
    } else if let Some(v) = any.downcast_ref::<i32>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<u32>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<i16>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<u16>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<i8>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<u8>() {
        CaughtValue::Number(f64::from(*v))
    } else if let Some(v) = any.downcast_ref::<String>() {
        CaughtValue::Str(v.clone())
    } else if let Some(v) = any.downcast_ref::<&str>() {
        CaughtValue::Str((*v).to_owned())
    } else {
        return false;
    };
    *caught == other_value
}

fn caught_from_payload(payload: &(dyn Any + Send)) -> Option<CaughtValue> {
    use num_traits::ToPrimitive;
    if let Some(c) = payload.downcast_ref::<CaughtValue>() {
        return Some(c.clone());
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return Some(CaughtValue::Str(s.clone()));
    }
    if let Some(s) = payload.downcast_ref::<&str>() {
        return Some(CaughtValue::Str((*s).to_owned()));
    }
    if let Some(s) = payload.downcast_ref::<JsString<'static>>() {
        return Some(CaughtValue::Str(s.to_string_lossy()));
    }
    if let Some(n) = payload.downcast_ref::<i64>() {
        return Some(CaughtValue::Number(n.to_f64().unwrap_or(f64::NAN)));
    }
    if let Some(n) = payload.downcast_ref::<u64>() {
        return Some(CaughtValue::Number(n.to_f64().unwrap_or(f64::NAN)));
    }
    if let Some(n) = payload.downcast_ref::<i32>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<u32>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<i16>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<u16>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<i8>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<u8>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(n) = payload.downcast_ref::<f64>() {
        return Some(CaughtValue::Number(*n));
    }
    if let Some(n) = payload.downcast_ref::<f32>() {
        return Some(CaughtValue::Number(f64::from(*n)));
    }
    if let Some(b) = payload.downcast_ref::<bool>() {
        return Some(CaughtValue::Bool(*b));
    }
    None
}

#[must_use]
pub fn __ts_aot_catch_bind(payload: &(dyn Any + Send)) -> CaughtValue {
    caught_from_payload(payload)
        .unwrap_or_else(|| CaughtValue::Str(__ts_aot_panic_message(payload)))
}

pub trait FromCaught: Sized {
    fn from_caught(caught: &CaughtValue) -> Option<Self>;
}

macro_rules! impl_from_caught_int {
    ($(($t:ty, $method:ident)),* $(,)?) => {
        $(
            impl FromCaught for $t {
                fn from_caught(caught: &CaughtValue) -> Option<Self> {
                    use num_traits::ToPrimitive;
                    match caught {
                        CaughtValue::Number(n) => n.$method(),
                        _ => None,
                    }
                }
            }
        )*
    };
}

impl_from_caught_int!(
    (i8, to_i8),
    (i16, to_i16),
    (i32, to_i32),
    (i64, to_i64),
    (u8, to_u8),
    (u16, to_u16),
    (u32, to_u32),
    (u64, to_u64),
    (f32, to_f32),
    (f64, to_f64),
);

impl FromCaught for bool {
    fn from_caught(caught: &CaughtValue) -> Option<Self> {
        match caught {
            CaughtValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

pub fn __ts_aot_catch_string<'a>(
    ctx: &Ctx<'a>,
    payload: Box<dyn Any + Send>,
) -> Result<JsString<'a>, Box<dyn Any + Send>> {
    match payload.downcast::<CaughtValue>() {
        Ok(caught) => match &*caught {
            CaughtValue::Str(s) => Ok(JsString::Valid(ctx.arena.alloc_str(s))),
            _ => Err(caught),
        },
        Err(payload) => match caught_from_payload(&*payload) {
            Some(CaughtValue::Str(s)) => Ok(JsString::Valid(ctx.arena.alloc_str(&s))),
            _ => Err(payload),
        },
    }
}

pub fn __ts_aot_catch_typed<T: FromCaught + 'static>(
    payload: Box<dyn Any + Send>,
) -> Result<T, Box<dyn Any + Send>> {
    match payload.downcast::<T>() {
        Ok(value) => Ok(*value),
        Err(payload) => match payload.downcast::<CaughtValue>() {
            Ok(caught) => match T::from_caught(&caught) {
                Some(value) => Ok(value),
                None => Err(Box::new(*caught)),
            },
            Err(payload) => Err(payload),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_bind_preserves_primitive_payload_kind() {
        assert_eq!(
            __ts_aot_catch_bind(&7_i64),
            CaughtValue::Number(7.0),
            "a thrown number must stay a number"
        );
        assert_eq!(
            __ts_aot_catch_bind(&"boom".to_owned()),
            CaughtValue::Str("boom".to_owned()),
            "a thrown string must stay a string"
        );
        assert_eq!(
            __ts_aot_catch_bind(&true),
            CaughtValue::Bool(true),
            "a thrown boolean must stay a boolean"
        );
    }

    #[test]
    fn catch_bind_recovers_arena_string_payload() {
        let payload: Box<dyn Any + Send> = Box::new(JsString::Valid("boom"));
        assert_eq!(
            __ts_aot_catch_bind(&*payload),
            CaughtValue::Str("boom".to_owned()),
            "a thrown arena string (`JsString<'static>`) must recover its text, not fall back to \
             `__ts_aot_panic_message`"
        );
    }

    #[test]
    fn caught_equality_is_tag_aware() {
        let number = CaughtValue::from(1_i64);
        let string = CaughtValue::from("1");
        assert_eq!(number, number);
        assert_ne!(
            number, string,
            "`1 === 1` must not hold against the string \"1\" (no stringification)"
        );
        assert!(__ts_aot_caught_eq(&number, 1_i64));
        assert!(__ts_aot_caught_eq(&number, 1.0_f64));
        assert!(!__ts_aot_caught_eq(&number, "1"));
        assert!(__ts_aot_caught_eq(&string, "1"));
        assert!(!__ts_aot_caught_eq(&string, 1_i64));
    }

    #[test]
    fn caught_typeof_and_stringification() {
        assert_eq!(CaughtValue::from(1_i64).typeof_str(), "number");
        assert_eq!(CaughtValue::from("x").typeof_str(), "string");
        assert_eq!(CaughtValue::from(true).typeof_str(), "boolean");
        assert_eq!(CaughtValue::from(1_i64).to_string(), "1");
        assert_eq!(CaughtValue::from("x").to_string(), "x");
    }

    #[test]
    fn caught_dyn_equality_matches_primitives() {
        assert!(__ts_aot_caught_eq_dyn(&CaughtValue::from(1_i64), &1_i64));
        assert!(__ts_aot_caught_eq_dyn(&CaughtValue::from(1_i64), &1.0_f64));
        assert!(!__ts_aot_caught_eq_dyn(&CaughtValue::from(1_i64), &"1"));
        assert!(__ts_aot_caught_eq_dyn(&CaughtValue::from("x"), &"x"));
        assert!(__ts_aot_caught_eq_dyn(&CaughtValue::from(true), &true));
        assert!(
            !__ts_aot_caught_eq_dyn(&CaughtValue::from(1_i64), &()),
            "an unsupported payload type must not spuriously equal a caught number"
        );
    }

    #[test]
    fn catch_bind_preserves_rethrown_caught_value() {
        let rethrown = CaughtValue::from(7_i64);
        assert_eq!(
            __ts_aot_catch_bind(&rethrown),
            rethrown,
            "a rethrown `CaughtValue` payload must be preserved (cloned), not stringified"
        );
        let rethrown_str = CaughtValue::from("boom");
        assert_eq!(__ts_aot_catch_bind(&rethrown_str), rethrown_str);
    }

    #[test]
    fn catch_bind_handles_extended_integer_payloads() {
        assert_eq!(__ts_aot_catch_bind(&5_u32), CaughtValue::Number(5.0));
        assert_eq!(__ts_aot_catch_bind(&5_u16), CaughtValue::Number(5.0));
        assert_eq!(__ts_aot_catch_bind(&5_u8), CaughtValue::Number(5.0));
        assert_eq!(__ts_aot_catch_bind(&5_i16), CaughtValue::Number(5.0));
        assert_eq!(__ts_aot_catch_bind(&5_i8), CaughtValue::Number(5.0));
        assert_eq!(__ts_aot_catch_bind(&5_u64), CaughtValue::Number(5.0));
    }

    struct NoHost;
    impl crate::arena::Host for NoHost {}

    #[test]
    fn catch_typed_recovers_from_caught_value() {
        let payload: Box<dyn Any + Send> = Box::new(CaughtValue::from(7_i64));
        assert_eq!(
            __ts_aot_catch_typed::<i64>(payload).expect("number recovers as i64"),
            7
        );
        let payload: Box<dyn Any + Send> = Box::new(42_i64);
        assert_eq!(
            __ts_aot_catch_typed::<i64>(payload).expect("a direct i64 payload still downcasts"),
            42
        );
        let host = NoHost;
        crate::arena::run_in_arena(&host, |ctx| {
            let payload: Box<dyn Any + Send> = Box::new(CaughtValue::from(7_i64));
            assert!(
                __ts_aot_catch_string(ctx, payload).is_err(),
                "an incompatible typed catch must keep the mismatch (Err) behavior"
            );
            let payload: Box<dyn Any + Send> = Box::new(CaughtValue::from("x"));
            assert_eq!(
                __ts_aot_catch_string(ctx, payload)
                    .expect("string recovers as JsString")
                    .to_string_lossy(),
                "x"
            );
        });
    }

    #[test]
    fn catch_typed_string_recovers_raw_arena_string_payload() {
        let host = NoHost;
        crate::arena::run_in_arena(&host, |ctx| {
            let payload: Box<dyn Any + Send> = Box::new(JsString::Valid("boom"));
            assert_eq!(
                __ts_aot_catch_string(ctx, payload)
                    .expect("a typed string catch must recover a raw arena string payload")
                    .to_string_lossy(),
                "boom"
            );
        });
    }

    #[test]
    fn catch_typed_numeric_conversion_semantics() {
        let payload: Box<dyn Any + Send> = Box::new(CaughtValue::Number(7.9_f64));
        assert_eq!(
            __ts_aot_catch_typed::<i64>(payload).expect("an in-range number recovers as i64"),
            7,
            "integer recovery truncates toward zero"
        );
        let payload: Box<dyn Any + Send> = Box::new(CaughtValue::Number(300.0_f64));
        assert!(
            __ts_aot_catch_typed::<u8>(payload).is_err(),
            "an out-of-range integer recovery is a mismatch (Err), not a wraparound"
        );
        let payload: Box<dyn Any + Send> = Box::new(CaughtValue::Number(1.5_f64));
        assert_eq!(
            __ts_aot_catch_typed::<f32>(payload)
                .expect("number narrows to f32")
                .to_bits(),
            1.5_f32.to_bits()
        );
        let payload: Box<dyn Any + Send> = Box::new(CaughtValue::Number(1.5_f64));
        assert_eq!(
            __ts_aot_catch_typed::<f64>(payload)
                .expect("number stays f64")
                .to_bits(),
            1.5_f64.to_bits()
        );
    }
}
