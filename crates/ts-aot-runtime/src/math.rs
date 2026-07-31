#[must_use]
pub fn __ts_aot_math_abs(x: f64) -> f64 {
    x.abs()
}

#[must_use]
pub fn __ts_aot_math_floor(x: f64) -> f64 {
    f64::floor(x)
}

#[must_use]
pub fn __ts_aot_math_ceil(x: f64) -> f64 {
    f64::ceil(x)
}

#[must_use]
pub fn __ts_aot_math_round(x: f64) -> f64 {
    if x.abs() >= 2.0_f64.powi(52) {
        return x;
    }
    if (-0.5..0.5).contains(&x) {
        if x.is_sign_negative() { -0.0 } else { 0.0 }
    } else {
        f64::floor(x + 0.5)
    }
}

#[must_use]
pub fn __ts_aot_math_trunc(x: f64) -> f64 {
    f64::trunc(x)
}

#[must_use]
pub fn __ts_aot_math_sign(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if is_f64_zero(x) {
        if x.is_sign_negative() { -0.0 } else { 0.0 }
    } else if x.is_sign_negative() {
        -1.0
    } else {
        1.0
    }
}

#[must_use]
pub fn __ts_aot_math_sqrt(x: f64) -> f64 {
    f64::sqrt(x)
}

#[must_use]
pub fn __ts_aot_math_pow(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 {
        return 1.0;
    }
    if exponent.is_infinite() && base.abs().to_bits() == 1.0_f64.to_bits() {
        return f64::NAN;
    }
    f64::powf(base, exponent)
}

#[must_use]
pub fn __ts_aot_math_log(x: f64) -> f64 {
    x.ln()
}

#[must_use]
pub fn __ts_aot_math_exp(x: f64) -> f64 {
    x.exp()
}

#[must_use]
pub fn __ts_aot_math_sin(x: f64) -> f64 {
    x.sin()
}

#[must_use]
pub fn __ts_aot_math_cos(x: f64) -> f64 {
    x.cos()
}

#[must_use]
pub fn __ts_aot_math_tan(x: f64) -> f64 {
    x.tan()
}

#[must_use]
pub fn __ts_aot_math_asin(x: f64) -> f64 {
    x.asin()
}

#[must_use]
pub fn __ts_aot_math_acos(x: f64) -> f64 {
    x.acos()
}

#[must_use]
pub fn __ts_aot_math_atan(x: f64) -> f64 {
    x.atan()
}

#[must_use]
pub fn __ts_aot_math_atan2(y: f64, x: f64) -> f64 {
    f64::atan2(y, x)
}

#[must_use]
pub fn __ts_aot_math_max(args: &[f64]) -> f64 {
    let mut iter = args.iter();
    let Some(&first) = iter.next() else {
        return f64::NEG_INFINITY;
    };
    if first.is_nan() {
        return f64::NAN;
    }
    let mut result = first;
    for &x in iter {
        if x.is_nan() {
            return f64::NAN;
        }
        let prev_positive = !result.is_sign_negative();
        let x_positive = !x.is_sign_negative();
        result = result.max(x);
        if result == 0.0 && (prev_positive || x_positive) {
            result = 0.0;
        }
    }
    result
}

#[must_use]
pub fn __ts_aot_math_min(args: &[f64]) -> f64 {
    let mut iter = args.iter();
    let Some(&first) = iter.next() else {
        return f64::INFINITY;
    };
    if first.is_nan() {
        return f64::NAN;
    }
    let mut result = first;
    for &x in iter {
        if x.is_nan() {
            return f64::NAN;
        }
        let prev_negative = result.is_sign_negative();
        let x_negative = x.is_sign_negative();
        result = result.min(x);
        if result == 0.0 && (prev_negative || x_negative) {
            result = -0.0;
        }
    }
    result
}

#[must_use]
pub fn __ts_aot_math_random() -> f64 {
    fastrand::f64()
}

fn is_f64_zero(x: f64) -> bool {
    let bits = x.to_bits();
    bits == 0 || bits == 0x8000_0000_0000_0000
}
