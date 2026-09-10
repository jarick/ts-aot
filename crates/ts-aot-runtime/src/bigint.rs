use std::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Rem, Sub};

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

#[derive(Debug, Clone)]
pub struct BigIntHandle {
    value: BigInt,
}

impl BigIntHandle {
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self {
            value: value
                .parse::<BigInt>()
                .expect("BigIntHandle::new: invalid BigInt string literal"),
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
            Self::from_bigint(BigInt::zero())
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
            Self::from_bigint(BigInt::zero())
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
        BigIntHandle::from_bigint(BigInt::zero())
    } else {
        BigIntHandle::from_bigint(lhs.value() / rhs.value())
    }
}

#[must_use]
pub fn __ts_aot_bigint_rem(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    if rhs.value().is_zero() {
        BigIntHandle::from_bigint(BigInt::zero())
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

#[must_use]
pub fn __ts_aot_bigint_shl(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    let shift = rhs.to_i64().unwrap_or(0);
    BigIntHandle::from_bigint(lhs.value() << shift)
}

#[must_use]
pub fn __ts_aot_bigint_shr(lhs: &BigIntHandle, rhs: &BigIntHandle) -> BigIntHandle {
    let shift = rhs.to_i64().unwrap_or(0);
    BigIntHandle::from_bigint(lhs.value() >> shift)
}

#[must_use]
pub fn __ts_aot_bigint_to_string(value: &BigIntHandle) -> String {
    value.value().to_string()
}

#[must_use]
pub fn __ts_aot_bigint_to_number(value: &BigIntHandle) -> f64 {
    value.to_f64()
}
