use std::hash::{Hash, Hasher};

use crate::host::__ts_aot_throw;

#[derive(Debug, Clone)]
pub enum JsString {
    Valid(String),
    Raw(Vec<u16>),
}

pub(crate) enum JsStringUnits<'a> {
    Valid(std::str::EncodeUtf16<'a>),
    Raw(std::iter::Copied<std::slice::Iter<'a, u16>>),
}

impl Iterator for JsStringUnits<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<u16> {
        match self {
            JsStringUnits::Valid(it) => it.next(),
            JsStringUnits::Raw(it) => it.next(),
        }
    }
}

impl Default for JsString {
    fn default() -> Self {
        JsString::Valid(String::new())
    }
}

impl JsString {
    pub(crate) fn units_iter(&self) -> JsStringUnits<'_> {
        match self {
            JsString::Valid(s) => JsStringUnits::Valid(s.encode_utf16()),
            JsString::Raw(units) => JsStringUnits::Raw(units.iter().copied()),
        }
    }

    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        match self {
            JsString::Valid(s) => s.clone(),
            JsString::Raw(units) => String::from_utf16_lossy(units),
        }
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        let a = self.units_iter();
        let mut b = other.units_iter();
        a.eq(b.by_ref())
    }
}

impl Eq for JsString {}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for cu in self.units_iter() {
            cu.hash(state);
        }
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        JsString::Valid(s.to_owned())
    }
}

impl JsString {
    #[must_use]
    pub fn from_units(units: Vec<u16>) -> Self {
        JsString::Raw(units)
    }

    #[must_use]
    pub fn as_valid(&self) -> Option<&str> {
        match self {
            JsString::Valid(s) => Some(s),
            JsString::Raw(_) => None,
        }
    }

    #[must_use]
    pub fn to_units(&self) -> Vec<u16> {
        match self {
            JsString::Valid(s) => s.encode_utf16().collect(),
            JsString::Raw(units) => units.clone(),
        }
    }

    #[must_use]
    pub fn len_code_units(&self) -> i64 {
        match self {
            JsString::Valid(s) => i64::try_from(s.encode_utf16().count()).unwrap_or(0),
            JsString::Raw(units) => i64::try_from(units.len()).unwrap_or(0),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len_code_units() == 0
    }
}

#[must_use]
pub fn __ts_aot_string_concat(a: &JsString, b: &JsString) -> JsString {
    let mut units = a.to_units();
    units.extend(b.to_units());
    JsString::Raw(units)
}

#[must_use]
pub fn __ts_aot_string_equals(a: &JsString, b: &JsString) -> bool {
    a == b
}

#[must_use]
pub fn __ts_aot_string_index_of(haystack: &JsString, needle: &JsString, from_index: i64) -> i64 {
    let haystack_units = haystack.to_units();
    let len_utf16 = haystack_units.len();
    if from_index < 0 {
        return __ts_aot_string_index_of(haystack, needle, 0);
    }
    let start = usize::try_from(from_index)
        .unwrap_or(len_utf16)
        .min(len_utf16);
    let needle_units = needle.to_units();
    if needle_units.is_empty() {
        return i64::try_from(start).unwrap_or(-1);
    }
    if start >= len_utf16 {
        return -1;
    }
    haystack_units[start..]
        .windows(needle_units.len())
        .position(|w| w == needle_units.as_slice())
        .map_or(-1, |i| i64::try_from(start + i).unwrap_or(-1))
}

#[must_use]
pub fn __ts_aot_string_char_at(s: &JsString, idx: i64) -> JsString {
    if idx < 0 {
        return JsString::Valid(String::new());
    }
    let Ok(i) = usize::try_from(idx) else {
        return JsString::Valid(String::new());
    };
    let units = s.to_units();
    match units.get(i) {
        Some(&code) => JsString::Raw(vec![code]),
        None => JsString::Valid(String::new()),
    }
}

#[must_use]
pub fn __ts_aot_string_substring_utf16(s: &JsString, start: i64, end: i64) -> JsString {
    let units = s.to_units();
    let len = units.len();
    let clamp = |n: i64| -> usize { usize::try_from(n.max(0)).unwrap_or(len).min(len) };
    let lo = clamp(start);
    let hi = clamp(end);
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    JsString::Raw(units[lo..hi].to_vec())
}

#[must_use]
pub fn __ts_aot_string_from_char_code(codes: &[i64]) -> JsString {
    let units: Vec<u16> = codes.iter().map(|&c| i64_to_char_code_u16(c)).collect();
    JsString::Raw(units)
}

#[must_use]
pub fn __ts_aot_string_from_code_point(points: &[i64]) -> JsString {
    let mut has_surrogate = false;
    for &p in points {
        if !(0..=0x10_FFFF).contains(&p) {
            __ts_aot_throw(format!("RangeError: Invalid code point {p}"));
        }
        if (0xD800..=0xDFFF).contains(&p) {
            has_surrogate = true;
        }
    }
    if has_surrogate {
        let mut units: Vec<u16> = Vec::with_capacity(points.len());
        for &p in points {
            if (0xD800..=0xDFFF).contains(&p) {
                units.push(u16::try_from(p).expect("validated surrogate range above"));
            } else {
                let as_u32 = u32::try_from(p).expect("validated in-range above");
                let c = char::from_u32(as_u32).expect("validated non-surrogate above");
                let mut buf = [0u16; 2];
                let encoded = c.encode_utf16(&mut buf);
                units.extend_from_slice(encoded);
            }
        }
        JsString::Raw(units)
    } else {
        let mut s = String::new();
        for &p in points {
            let as_u32 = u32::try_from(p).expect("validated in-range above");
            let c = char::from_u32(as_u32).expect("validated non-surrogate above");
            s.push(c);
        }
        JsString::Valid(s)
    }
}

#[must_use]
pub fn __ts_aot_string_len(s: &JsString) -> i64 {
    s.len_code_units()
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn i64_to_char_code_u16(c: i64) -> u16 {
    (c as u32 & 0xFFFF) as u16
}
