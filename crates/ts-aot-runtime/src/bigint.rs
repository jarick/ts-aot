use std::fmt;
use std::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Rem, Sub};

use num_bigint::BigInt;
use num_traits::{Num, ToPrimitive, Zero};

use crate::host::__ts_aot_throw;

#[derive(Debug, Clone)]
pub struct BigIntHandle {
    value: BigInt,
}

#[derive(Debug)]
pub struct BigIntSyntaxError {
    pub message: String,
}

impl BigIntSyntaxError {
    #[must_use]
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
}

impl fmt::Display for BigIntSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BigIntSyntaxError: {}", self.message)
    }
}

pub fn parse_string_integer_literal(s: &str) -> Result<BigInt, BigIntSyntaxError> {
    let trimmed = ecma_trim_whitespace(s);
    if trimmed.is_empty() {
        return Ok(BigInt::zero());
    }
    if trimmed.contains('_') {
        return Err(BigIntSyntaxError::new(&format!(
            "Cannot convert {s:?} to a BigInt: not a valid string integer literal"
        )));
    }
    let lower = trimmed.to_lowercase();
    let (sign, body, had_explicit_sign) = parse_sign(&lower);
    if had_explicit_sign {
        let parsed = parse_decimal_literal(body).ok_or_else(|| {
            BigIntSyntaxError::new(&format!(
                "Cannot convert {s:?} to a BigInt: not a valid string integer literal"
            ))
        })?;
        return Ok(match sign {
            Sign::Positive => parsed,
            Sign::Negative => -parsed,
        });
    }
    if let Some(value) = parse_unsigned_non_decimal_literal(body) {
        return Ok(value);
    }
    if let Some(value) = parse_decimal_literal(body) {
        return Ok(value);
    }
    Err(BigIntSyntaxError::new(&format!(
        "Cannot convert {s:?} to a BigInt: not a valid string integer literal"
    )))
}

enum Sign {
    Positive,
    Negative,
}

fn parse_sign(s: &str) -> (Sign, &str, bool) {
    let bytes = s.as_bytes();
    if bytes.first() == Some(&b'+') {
        (Sign::Positive, &s[1..], true)
    } else if bytes.first() == Some(&b'-') {
        (Sign::Negative, &s[1..], true)
    } else {
        (Sign::Positive, s, false)
    }
}

fn parse_decimal_literal(s: &str) -> Option<BigInt> {
    if s.is_empty() {
        return None;
    }
    if matches!(s.as_bytes().first(), Some(b'+' | b'-')) {
        return None;
    }
    BigInt::from_str_radix(s, 10).ok()
}

fn parse_unsigned_non_decimal_literal(s: &str) -> Option<BigInt> {
    let bytes = s.as_bytes();
    let (digits, radix) = if bytes.starts_with(b"0x") {
        (&s[2..], 16u32)
    } else if bytes.starts_with(b"0o") {
        (&s[2..], 8u32)
    } else if bytes.starts_with(b"0b") {
        (&s[2..], 2u32)
    } else {
        return None;
    };
    BigInt::from_str_radix(digits, radix).ok()
}

fn is_ecma_whitespace(c: char) -> bool {
    match c {
        '\u{FEFF}' => true,
        '\u{0085}' => false,
        _ => c.is_whitespace(),
    }
}

fn first_non_ws<I>(mut iter: I) -> Option<(usize, char)>
where
    I: Iterator<Item = (usize, char)>,
{
    iter.find(|(_, c)| !is_ecma_whitespace(*c))
}

fn ecma_trim_whitespace(s: &str) -> &str {
    let leading_end = first_non_ws(s.char_indices()).map_or(s.len(), |(i, _)| i);
    let trailing_start = first_non_ws(s.char_indices().rev()).map_or(0, |(i, c)| i + c.len_utf8());
    if leading_end > trailing_start {
        ""
    } else {
        &s[leading_end..trailing_start]
    }
}

impl BigIntHandle {
    #[must_use]
    pub fn new(value: &str) -> Self {
        match parse_string_integer_literal(value) {
            Ok(parsed) => Self { value: parsed },
            Err(e) => __ts_aot_throw(e),
        }
    }

    #[must_use]
    pub fn from_bigint(value: BigInt) -> Self {
        Self { value }
    }

    #[must_use]
    pub fn value(&self) -> &BigInt {
        &self.value
    }

    #[must_use]
    pub fn to_f64(&self) -> f64 {
        self.value.to_f64().unwrap_or(f64::NAN)
    }

    #[must_use]
    pub fn to_i64(&self) -> Option<i64> {
        self.value.to_i64()
    }
}

impl PartialEq for BigIntHandle {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for BigIntHandle {}

impl Add for BigIntHandle {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            value: self.value + rhs.value,
        }
    }
}

impl Sub for BigIntHandle {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            value: self.value - rhs.value,
        }
    }
}

impl Mul for BigIntHandle {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            value: self.value * rhs.value,
        }
    }
}

impl Div for BigIntHandle {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        if rhs.value.is_zero() {
            __ts_aot_throw("RangeError: Division by zero".to_string())
        } else {
            Self {
                value: self.value / rhs.value,
            }
        }
    }
}

impl Rem for BigIntHandle {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self {
        if rhs.value.is_zero() {
            __ts_aot_throw("RangeError: Division by zero".to_string())
        } else {
            Self {
                value: self.value % rhs.value,
            }
        }
    }
}

impl Neg for BigIntHandle {
    type Output = Self;
    fn neg(self) -> Self {
        Self { value: -self.value }
    }
}

impl BitAnd for BigIntHandle {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self {
            value: self.value & rhs.value,
        }
    }
}

impl BitOr for BigIntHandle {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self {
            value: self.value | rhs.value,
        }
    }
}

impl BitXor for BigIntHandle {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self {
            value: self.value ^ rhs.value,
        }
    }
}

impl Not for BigIntHandle {
    type Output = Self;
    fn not(self) -> Self {
        Self { value: !self.value }
    }
}

#[must_use]
pub fn __ts_aot_bigint_new(value: &str) -> BigIntHandle {
    BigIntHandle::new(value)
}

#[must_use]
pub fn __ts_aot_bigint_add(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() + rhs.value())
}

#[must_use]
pub fn __ts_aot_bigint_sub(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() - rhs.value())
}

#[must_use]
pub fn __ts_aot_bigint_mul(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() * rhs.value())
}

#[must_use]
pub fn __ts_aot_bigint_div(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    if rhs.value().is_zero() {
        __ts_aot_throw("RangeError: Division by zero".to_string())
    } else {
        BigIntHandle::from_bigint(lhs.value() / rhs.value())
    }
}

#[must_use]
pub fn __ts_aot_bigint_rem(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    if rhs.value().is_zero() {
        __ts_aot_throw("RangeError: Division by zero".to_string())
    } else {
        BigIntHandle::from_bigint(lhs.value() % rhs.value())
    }
}

#[must_use]
pub fn __ts_aot_bigint_neg(value: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(-value.value())
}

#[must_use]
pub fn __ts_aot_bigint_not(value: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(!value.value())
}

#[must_use]
pub fn __ts_aot_bigint_lt(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() < rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_le(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() <= rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_gt(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() > rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_ge(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() >= rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_eq(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() == rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_neq(lhs: &BigIntHandle, rhs: &BigIntHandle) -> bool {
    lhs.value() != rhs.value()
}

#[must_use]
pub fn __ts_aot_bigint_and(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() & rhs.value())
}

#[must_use]
pub fn __ts_aot_bigint_or(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() | rhs.value())
}

#[must_use]
pub fn __ts_aot_bigint_xor(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    BigIntHandle::from_bigint(lhs.value() ^ rhs.value())
}

const BIGINT_MAX_SHIFT: u64 = u32::MAX as u64;

#[derive(Clone, Copy)]
enum ShiftDir {
    Left,
    Right,
}

fn bigint_shift_count(rhs: &BigInt) -> i64 {
    if let Some(v) = rhs.to_i64() {
        return v;
    }
    let masked = rhs & BigInt::from(u64::MAX);
    masked.to_i64().unwrap_or(0)
}

fn bigint_resolved_shift(rhs: &BigIntHandle, original: ShiftDir) -> (ShiftDir, u64) {
    let raw = bigint_shift_count(rhs.value());
    if raw < 0 {
        let magnitude = raw.unsigned_abs();
        let reversed = match original {
            ShiftDir::Left => ShiftDir::Right,
            ShiftDir::Right => ShiftDir::Left,
        };
        if matches!(reversed, ShiftDir::Left) && magnitude > BIGINT_MAX_SHIFT {
            __ts_aot_throw("RangeError: BigInt shift count exceeds maximum".to_string());
        }
        (reversed, magnitude)
    } else {
        if raw.cast_unsigned() > BIGINT_MAX_SHIFT {
            __ts_aot_throw("RangeError: BigInt shift count exceeds maximum".to_string());
        }
        (original, raw.cast_unsigned())
    }
}

fn bigint_apply_resolved_shift(
    lhs: &BigIntHandle,
    rhs: &BigIntHandle,
    original: ShiftDir,
) -> BigIntHandle {
    let (dir, count) = bigint_resolved_shift(rhs, original);
    match dir {
        ShiftDir::Left => BigIntHandle::from_bigint(lhs.value() << count),
        ShiftDir::Right => BigIntHandle::from_bigint(lhs.value() >> count),
    }
}

#[must_use]
pub fn __ts_aot_bigint_shl(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    bigint_apply_resolved_shift(lhs, rhs, ShiftDir::Left)
}

#[must_use]
pub fn __ts_aot_bigint_shr(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    bigint_apply_resolved_shift(lhs, rhs, ShiftDir::Right)
}

#[must_use]
pub fn __ts_aot_bigint_to_string(value: &BigIntHandle, radix: Option<i32>) -> String {
    let radix = bigint_validated_radix(radix);
    value.value().to_str_radix(radix)
}

fn bigint_validated_radix(radix: Option<i32>) -> u32 {
    let radix = radix.unwrap_or(10);
    if !(2..=36).contains(&radix) {
        __ts_aot_throw(format!(
            "RangeError: BigInt.toString() radix must be between 2 and 36, got {radix}"
        ));
    }
    radix.cast_unsigned()
}

#[must_use]
pub fn __ts_aot_bigint_to_number(value: &BigIntHandle) -> f64 {
    value.to_f64()
}
