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
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(BigInt::zero());
    }
    let lower = trimmed.to_lowercase();
    let had_explicit_sign = matches!(lower.as_bytes().first(), Some(b'+' | b'-'));
    let (sign, body) = parse_sign(&lower);
    if let Some(value) = parse_with_radix(body, sign, had_explicit_sign) {
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

fn parse_sign(s: &str) -> (Sign, &str) {
    let bytes = s.as_bytes();
    if bytes.first() == Some(&b'+') {
        (Sign::Positive, &s[1..])
    } else if bytes.first() == Some(&b'-') {
        (Sign::Negative, &s[1..])
    } else {
        (Sign::Positive, s)
    }
}

fn parse_with_radix(s: &str, sign: Sign, had_explicit_sign: bool) -> Option<BigInt> {
    let bytes = s.as_bytes();
    let (digits, radix) = if bytes.starts_with(b"0x") {
        (&s[2..], 16u32)
    } else if bytes.starts_with(b"0o") {
        (&s[2..], 8u32)
    } else if bytes.starts_with(b"0b") {
        (&s[2..], 2u32)
    } else {
        if matches!(bytes.first(), Some(b'+' | b'-')) {
            return None;
        }
        let parsed = BigInt::from_str_radix(s, 10).ok()?;
        return Some(match sign {
            Sign::Positive => parsed,
            Sign::Negative => -parsed,
        });
    };
    if had_explicit_sign {
        return None;
    }
    BigInt::from_str_radix(digits, radix).ok()
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

fn bigint_shift_count(rhs: &BigInt) -> i64 {
    if let Some(v) = rhs.to_i64() {
        return v;
    }
    let masked = rhs & BigInt::from(u64::MAX);
    masked.to_i64().unwrap_or(0)
}

#[must_use]
pub fn __ts_aot_bigint_shl(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    let shift = bigint_shift_count(rhs.value());
    if shift < 0 {
        __ts_aot_throw("RangeError: BigInt shift count cannot be negative".to_string());
    }
    if (shift as u64) > BIGINT_MAX_SHIFT {
        __ts_aot_throw("RangeError: BigInt shift count exceeds maximum".to_string());
    }
    BigIntHandle::from_bigint(lhs.value() << shift)
}

#[must_use]
pub fn __ts_aot_bigint_shr(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    let shift = bigint_shift_count(rhs.value());
    if shift < 0 {
        __ts_aot_throw("RangeError: BigInt shift count cannot be negative".to_string());
    }
    if (shift as u64) > BIGINT_MAX_SHIFT {
        __ts_aot_throw("RangeError: BigInt shift count exceeds maximum".to_string());
    }
    BigIntHandle::from_bigint(lhs.value() >> shift)
}

#[must_use]
pub fn __ts_aot_bigint_to_string(value: &BigIntHandle, radix: Option<i32>) -> String {
    let radix = radix.unwrap_or(10);
    if !(2..=36).contains(&radix) {
        __ts_aot_throw(format!(
            "RangeError: BigInt.toString() radix must be between 2 and 36, got {radix}"
        ));
    }
    value.value().to_str_radix(radix as u32)
}

#[must_use]
pub fn __ts_aot_bigint_to_number(value: &BigIntHandle) -> f64 {
    value.to_f64()
}
