use std::hash::{Hash, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

impl Serialize for JsString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            JsString::Valid(s) => serializer.serialize_str(s),
            JsString::Raw(units) => {
                let s = String::from_utf16_lossy(units);
                serializer.serialize_str(&s)
            }
        }
    }
}

impl<'de> Deserialize<'de> for JsString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let lossy = bytes_to_utf16_lossy(s.as_bytes());
        let lossless: Vec<u16> = s.encode_utf16().collect();
        if lossy == lossless {
            Ok(JsString::Valid(s))
        } else {
            Ok(JsString::Raw(lossy))
        }
    }
}

pub(crate) fn bytes_to_utf16_lossy(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 < 0x80 {
            units.push(u16::from(b0));
            i += 1;
        } else if (0xC2..=0xDF).contains(&b0) && i + 1 < bytes.len() {
            let b1 = bytes[i + 1];
            if (b1 & 0xC0) == 0x80 {
                let cp = (u32::from(b0 & 0x1F) << 6) | u32::from(b1 & 0x3F);
                let unit = u16::try_from(cp).unwrap_or(0xFFFD);
                units.push(unit);
                i += 2;
            } else {
                units.push(0xFFFD);
                i += 1;
            }
        } else if (0xE0..=0xEF).contains(&b0) && i + 2 < bytes.len() {
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            if (b1 & 0xC0) == 0x80 && (b2 & 0xC0) == 0x80 {
                let cp = (u32::from(b0 & 0x0F) << 12)
                    | (u32::from(b1 & 0x3F) << 6)
                    | u32::from(b2 & 0x3F);
                let unit = u16::try_from(cp).unwrap_or(0xFFFD);
                units.push(unit);
                i += 3;
            } else {
                units.push(0xFFFD);
                i += 1;
            }
        } else if (0xF0..=0xF7).contains(&b0) && i + 3 < bytes.len() {
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            let b3 = bytes[i + 3];
            if (b1 & 0xC0) == 0x80 && (b2 & 0xC0) == 0x80 && (b3 & 0xC0) == 0x80 {
                let cp = (u32::from(b0 & 0x07) << 18)
                    | (u32::from(b1 & 0x3F) << 12)
                    | (u32::from(b2 & 0x3F) << 6)
                    | u32::from(b3 & 0x3F);
                if (0x0001_0000..=0x0010_FFFF).contains(&cp) {
                    let adjusted = cp - 0x10000;
                    let high = 0xD800 + u16::try_from(adjusted >> 10).unwrap_or(0);
                    let low = 0xDC00 + u16::try_from(adjusted & 0x3FF).unwrap_or(0);
                    units.push(high);
                    units.push(low);
                    i += 4;
                } else {
                    units.push(0xFFFD);
                    i += 1;
                }
            } else {
                units.push(0xFFFD);
                i += 1;
            }
        } else {
            units.push(0xFFFD);
            i += 1;
        }
    }
    units
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

fn i64_to_char_code_u16(c: i64) -> u16 {
    u16::try_from(c & 0xFFFF_i64).expect("0..=0xFFFF fits in u16")
}

#[cfg(test)]
mod tests {
    use super::bytes_to_utf16_lossy;

    #[test]
    fn bytes_to_utf16_lossy_valid_4byte_min_codepoint() {
        let units = bytes_to_utf16_lossy(&[0xF0, 0x90, 0x80, 0x80]);
        assert_eq!(
            units,
            vec![0xD800, 0xDC00],
            "U+10000 must encode as surrogate pair high=0xD800 low=0xDC00"
        );
    }

    #[test]
    fn bytes_to_utf16_lossy_valid_4byte_max_codepoint() {
        let units = bytes_to_utf16_lossy(&[0xF4, 0x8F, 0xBF, 0xBF]);
        assert_eq!(
            units,
            vec![0xDBFF, 0xDFFF],
            "U+10FFFF must encode as surrogate pair high=0xDBFF low=0xDFFF"
        );
    }

    #[test]
    fn bytes_to_utf16_lossy_rejects_4byte_overlong() {
        let units = bytes_to_utf16_lossy(&[0xF0, 0x80, 0x80, 0x80]);
        assert_eq!(
            units,
            vec![0xFFFD; 4],
            "overlong 4-byte (cp < 0x10000) must yield 0xFFFD per byte (existing maximal-bad-subpart policy), not underflow at cp - 0x10000"
        );
    }

    #[test]
    fn bytes_to_utf16_lossy_rejects_4byte_out_of_range() {
        let units = bytes_to_utf16_lossy(&[0xF5, 0x80, 0x80, 0x80]);
        assert_eq!(
            units,
            vec![0xFFFD; 4],
            "cp > 0x10FFFF (0xF5+ prefix) must yield 0xFFFD per byte, not invalid surrogate units from cp - 0x10000"
        );
    }
}
