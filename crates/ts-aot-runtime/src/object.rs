use crate::string::JsString;

pub trait SameValue {
    fn same_value(&self, other: &Self) -> bool;
}

macro_rules! same_value_eq {
    ($($t:ty),*) => {
        $(
            impl SameValue for $t {
                fn same_value(&self, other: &Self) -> bool {
                    self == other
                }
            }
        )*
    };
}

same_value_eq!(i8, i16, i32, i64, u8, u16, u32, u64, bool);

impl SameValue for f32 {
    fn same_value(&self, other: &Self) -> bool {
        (self.is_nan() && other.is_nan()) || self.to_bits() == other.to_bits()
    }
}

impl SameValue for f64 {
    fn same_value(&self, other: &Self) -> bool {
        (self.is_nan() && other.is_nan()) || self.to_bits() == other.to_bits()
    }
}

impl SameValue for JsString {
    fn same_value(&self, other: &Self) -> bool {
        self == other
    }
}

impl<T: SameValue> SameValue for Option<T> {
    fn same_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(a), Some(b)) => a.same_value(b),
            (None, None) => true,
            _ => false,
        }
    }
}

#[must_use]
pub fn __ts_aot_object_is<T: SameValue>(a: &T, b: &T) -> bool {
    a.same_value(b)
}

#[cfg(test)]
mod tests {
    use super::__ts_aot_object_is;
    use crate::string::JsString;

    #[test]
    fn object_is_uses_same_value_semantics() {
        assert!(__ts_aot_object_is(&1_i64, &1_i64));
        assert!(!__ts_aot_object_is(&1_i64, &2_i64));
        assert!(__ts_aot_object_is(&1.5_f64, &1.5_f64));
        assert!(
            __ts_aot_object_is(&f64::NAN, &f64::NAN),
            "Object.is(NaN, NaN) must be true (SameValue, unlike ==)"
        );
        assert!(
            !__ts_aot_object_is(&0.0_f64, &-0.0_f64),
            "Object.is(0, -0) must be false (SameValue distinguishes signed zero)"
        );
        assert!(__ts_aot_object_is(&true, &true));
        assert!(!__ts_aot_object_is(&true, &false));
        assert!(__ts_aot_object_is(
            &JsString::from("a"),
            &JsString::from("a")
        ));
        assert!(!__ts_aot_object_is(
            &JsString::from("a"),
            &JsString::from("b")
        ));
    }
}
