use std::cmp::Ordering;

use bumpalo::collections::String as BumpString;
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

use crate::accessor::__ts_aot_throw_range_error;
use crate::arena::Ctx;
use crate::string::JsString;

const MAX_RADIX_FRACTION_DIGITS: usize = 1100;
const MAX_SIGNIFICANT_DIGITS: usize = 17;

fn js_string<'a>(ctx: &Ctx<'a>, text: String) -> JsString<'a> {
    let out = BumpString::from_str_in(&text, ctx.arena);
    JsString::Valid(out.into_bump_str())
}

fn special(value: f64) -> Option<&'static str> {
    if value.is_nan() {
        Some("NaN")
    } else if value == f64::INFINITY {
        Some("Infinity")
    } else if value == f64::NEG_INFINITY {
        Some("-Infinity")
    } else {
        None
    }
}

fn pow_small(base: u32, exponent: i32) -> BigUint {
    let mut out = BigUint::one();
    let mut factor = BigUint::from(base);
    let mut remaining = u32::try_from(exponent.max(0)).unwrap_or(0);
    while remaining > 0 {
        if remaining & 1 == 1 {
            out *= &factor;
        }
        remaining >>= 1;
        if remaining > 0 {
            factor = &factor * &factor;
        }
    }
    out
}

fn compare_pow10(mantissa: &BigUint, exponent: i32, power: i32) -> Ordering {
    let shift_two = -exponent.min(power).min(0);
    let shift_five = -power.min(0);
    let mut left = mantissa.clone() * pow_small(2, exponent + shift_two);
    left *= pow_small(5, shift_five);
    let right = pow_small(2, power + shift_two) * pow_small(5, power + shift_five);
    left.cmp(&right)
}

fn decimal_exponent(mantissa: &BigUint, exponent: i32) -> i32 {
    let estimate =
        mantissa.to_f64().unwrap_or(1.0).log10() + f64::from(exponent) * std::f64::consts::LOG10_2;
    let mut candidate = estimate.floor().clamp(-350.0, 350.0).to_i32().unwrap_or(0);
    while compare_pow10(mantissa, exponent, candidate) == Ordering::Less {
        candidate -= 1;
    }
    while compare_pow10(mantissa, exponent, candidate + 1) != Ordering::Less {
        candidate += 1;
    }
    candidate
}

fn power_scaled(mantissa: &BigUint, exponent: i32, decimal_shift: i32) -> (BigUint, BigUint) {
    let mut numerator = mantissa.clone();
    let mut denominator = BigUint::one();
    if exponent >= 0 {
        numerator *= pow_small(2, exponent);
    } else {
        denominator *= pow_small(2, -exponent);
    }
    if decimal_shift >= 0 {
        numerator *= pow_small(10, decimal_shift);
    } else {
        denominator *= pow_small(10, -decimal_shift);
    }
    (numerator, denominator)
}

fn rounded_quotient(numerator: &BigUint, denominator: &BigUint, ties_to_even: bool) -> BigUint {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let doubled = remainder << 1u32;
    let round_up = match doubled.cmp(denominator) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => !ties_to_even || quotient.bit(0),
    };
    if round_up { quotient + 1u32 } else { quotient }
}

fn significant_digits(
    mantissa: &BigUint,
    exponent: i32,
    decimal_exponent: i32,
    digits: usize,
    ties_to_even: bool,
) -> (String, i32) {
    let width = i32::try_from(digits).unwrap_or(1);
    let (numerator, denominator) = power_scaled(mantissa, exponent, width - 1 - decimal_exponent);
    let mut scaled = rounded_quotient(&numerator, &denominator, ties_to_even);
    let mut decimal_exponent = decimal_exponent;
    if scaled >= pow_small(10, width) {
        scaled /= 10u32;
        decimal_exponent += 1;
    }
    (integer_digits(&scaled, 10), decimal_exponent)
}

#[allow(clippy::float_cmp)]
fn decimal_candidate_matches(scaled: &BigUint, decimal_shift: i32, target: f64) -> bool {
    let (numerator, denominator) = if decimal_shift >= 0 {
        (scaled * pow_small(10, decimal_shift), BigUint::one())
    } else {
        (scaled.clone(), pow_small(10, -decimal_shift))
    };
    rational_to_f64(&numerator, &denominator) == target
}

fn midpoint_order(
    mantissa: &BigUint,
    exponent: i32,
    lower: &BigUint,
    decimal_shift: i32,
) -> Ordering {
    let mut left = mantissa.clone() * 2u32;
    let mut right = lower * 2u32 + 1u32;
    if exponent >= 0 {
        left *= pow_small(2, exponent);
    } else {
        right *= pow_small(2, -exponent);
    }
    if decimal_shift >= 0 {
        right *= pow_small(10, decimal_shift);
    } else {
        left *= pow_small(10, -decimal_shift);
    }
    left.cmp(&right)
}

fn render_significant(scaled: BigUint, width: usize, decimal_exponent: i32) -> (String, i32) {
    if scaled == pow_small(10, i32::try_from(width).unwrap_or(1)) {
        return (format!("1{}", "0".repeat(width - 1)), decimal_exponent + 1);
    }
    (integer_digits(&scaled, 10), decimal_exponent)
}

fn shortest_digits(value: f64) -> (String, i32) {
    let (mantissa, exponent) = decompose(value);
    let decimal_exponent = decimal_exponent(&mantissa, exponent);
    for width in 1..=MAX_SIGNIFICANT_DIGITS {
        let shift = decimal_exponent - i32::try_from(width).unwrap_or(1) + 1;
        let (numerator, denominator) = power_scaled(&mantissa, exponent, -shift);
        let lower = &numerator / &denominator;
        let upper = lower.clone() + 1u32;
        let lower_matches = decimal_candidate_matches(&lower, shift, value);
        let upper_matches = decimal_candidate_matches(&upper, shift, value);
        let chosen = match (lower_matches, upper_matches) {
            (false, false) => continue,
            (true, false) => lower,
            (false, true) => upper,
            (true, true) => match midpoint_order(&mantissa, exponent, &lower, shift) {
                Ordering::Greater => upper,
                Ordering::Less => lower,
                Ordering::Equal => {
                    if lower.bit(0) {
                        upper
                    } else {
                        lower
                    }
                }
            },
        };
        return render_significant(chosen, width, decimal_exponent);
    }
    significant_digits(
        &mantissa,
        exponent,
        decimal_exponent,
        MAX_SIGNIFICANT_DIGITS,
        true,
    )
}

#[allow(clippy::float_cmp)]
fn digits_and_exponent(value: f64) -> (String, i32) {
    if value == 0.0 {
        return ("0".to_owned(), 1);
    }
    let (digits, exponent) = shortest_digits(value);
    (digits, exponent + 1)
}

fn assemble(digits: &str, exponent: i32, negative: bool) -> String {
    let k = i32::try_from(digits.len()).expect("digit count fits in i32");
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if k <= exponent && exponent <= 21 {
        out.push_str(digits);
        for _ in 0..(exponent - k) {
            out.push('0');
        }
    } else if 0 < exponent && exponent <= 21 {
        let split = usize::try_from(exponent).expect("positive exponent");
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    } else if -6 < exponent && exponent <= 0 {
        out.push_str("0.");
        for _ in 0..(-exponent) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let printed = exponent - 1;
        out.push(if printed < 0 { '-' } else { '+' });
        out.push_str(&printed.unsigned_abs().to_string());
    }
    out
}

fn decimal_to_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let (digits, exponent) = digits_and_exponent(value.abs());
    assemble(&digits, exponent, value < 0.0)
}

fn radix_digit(digit: &BigUint) -> char {
    let value: u32 = digit.try_into().unwrap_or(0);
    char::from_digit(value, 36).unwrap_or('0')
}

fn integer_digits(value: &BigUint, radix: u32) -> String {
    if value.is_zero() {
        return "0".to_owned();
    }
    let radix = BigUint::from(radix);
    let mut out = Vec::new();
    let mut current = value.clone();
    while !current.is_zero() {
        let digit = &current % &radix;
        current /= &radix;
        out.push(radix_digit(&digit));
    }
    out.reverse();
    out.into_iter().collect()
}

fn decompose(value: f64) -> (BigUint, i32) {
    let bits = value.to_bits();
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    if raw_exponent == 0 {
        (BigUint::from(fraction), -1074)
    } else {
        (BigUint::from(fraction | (1 << 52)), raw_exponent - 1075)
    }
}

fn at_least_pow_two(numerator: &BigUint, denominator: &BigUint, exponent: i32) -> bool {
    if exponent >= 0 {
        numerator >= &(denominator * pow_small(2, exponent))
    } else {
        &(numerator * pow_small(2, -exponent)) >= denominator
    }
}

fn binary_exponent(numerator: &BigUint, denominator: &BigUint) -> i32 {
    let mut exponent = i32::try_from(numerator.bits()).unwrap_or(0)
        - i32::try_from(denominator.bits()).unwrap_or(0);
    while !at_least_pow_two(numerator, denominator, exponent) {
        exponent -= 1;
    }
    while at_least_pow_two(numerator, denominator, exponent + 1) {
        exponent += 1;
    }
    exponent
}

fn rational_to_f64(numerator: &BigUint, denominator: &BigUint) -> f64 {
    if numerator.is_zero() {
        return 0.0;
    }
    let exponent = binary_exponent(numerator, denominator);
    let value_unit = (exponent - 52).max(-1074);
    let (scaled_numerator, scaled_denominator) = if value_unit >= 0 {
        (numerator.clone(), denominator * pow_small(2, value_unit))
    } else {
        (numerator * pow_small(2, -value_unit), denominator.clone())
    };
    let whole = pow_small(2, 52);
    let mut value_unit = value_unit;
    let mut mantissa = rounded_quotient(&scaled_numerator, &scaled_denominator, true);
    if mantissa == pow_small(2, 53) {
        mantissa.clone_from(&whole);
        value_unit += 1;
    }
    if mantissa < whole {
        return f64::from_bits(mantissa.to_u64().unwrap_or(0));
    }
    let final_exponent = value_unit + 52;
    if final_exponent > 1023 {
        return f64::INFINITY;
    }
    let fraction = (&mantissa - &whole).to_u64().unwrap_or(0);
    let biased = u64::try_from(final_exponent + 1023).unwrap_or(0) & 0x7ff;
    f64::from_bits((biased << 52) | fraction)
}

fn increment_digits(digits: &[char], radix: u32) -> Option<Vec<char>> {
    let mut out = digits.to_vec();
    let mut index = out.len();
    while index > 0 {
        index -= 1;
        let value = out[index].to_digit(36).unwrap_or(0) + 1;
        if value < radix {
            out[index] = char::from_digit(value, 36).unwrap_or('0');
            return Some(out);
        }
        out[index] = '0';
    }
    None
}

fn fraction_rounds_up(
    mantissa: &BigUint,
    exponent: i32,
    integer: &BigUint,
    fraction: &BigUint,
    denominator: &BigUint,
) -> bool {
    let mut left = mantissa.clone() * 2u32 * denominator;
    let mut right = integer * denominator * 2u32 + fraction * 2u32 + 1u32;
    if exponent >= 0 {
        left *= pow_small(2, exponent);
    } else {
        right *= pow_small(2, -exponent);
    }
    left >= right
}

#[allow(clippy::float_cmp)]
fn prefix_round_trips(magnitude: f64, integer: &BigUint, digits: &[char], radix: u32) -> bool {
    let radix_big = BigUint::from(radix);
    let mut denominator = BigUint::one();
    let mut fraction = BigUint::zero();
    for digit in digits {
        denominator *= &radix_big;
        fraction *= &radix_big;
        fraction += BigUint::from(digit.to_digit(36).unwrap_or(0));
    }
    let numerator = integer * &denominator + &fraction;
    if rational_to_f64(&numerator, &denominator) == magnitude {
        return true;
    }
    increment_digits(digits, radix).is_some()
        && rational_to_f64(&(&numerator + 1u32), &denominator) == magnitude
}

#[allow(clippy::float_cmp)]
fn shortest_fraction(magnitude: f64, integer: &BigUint, exact: &[char], radix: u32) -> Vec<char> {
    let (mantissa, exponent) = decompose(magnitude);
    let radix_big = BigUint::from(radix);
    let leading = exact.iter().take_while(|digit| **digit == '0').count();
    let start = leading.max(1).min(exact.len().max(1));
    let mut denominator = BigUint::one();
    let mut fraction = BigUint::zero();
    for digit in &exact[..start] {
        denominator *= &radix_big;
        fraction *= &radix_big;
        fraction += BigUint::from(digit.to_digit(36).unwrap_or(0));
    }
    let mut numerator = integer * &denominator + &fraction;
    for keep in start..=exact.len() {
        if keep > start {
            denominator *= &radix_big;
            fraction *= &radix_big;
            fraction += BigUint::from(exact[keep - 1].to_digit(36).unwrap_or(0));
            numerator = integer * &denominator + &fraction;
        }
        let digits = &exact[..keep];
        let truncated_matches = rational_to_f64(&numerator, &denominator) == magnitude;
        let rounded_digits = increment_digits(digits, radix);
        let rounded_matches = rounded_digits.is_some()
            && rational_to_f64(&(&numerator + 1u32), &denominator) == magnitude;
        if truncated_matches || rounded_matches {
            let prefer_rounded = rounded_matches
                && (!truncated_matches
                    || fraction_rounds_up(&mantissa, exponent, integer, &fraction, &denominator));
            if prefer_rounded && let Some(rounded) = rounded_digits {
                return rounded;
            }
            if truncated_matches {
                return digits.to_vec();
            }
        }
    }
    exact.to_vec()
}

fn radix_to_string(value: f64, radix: u32) -> String {
    let negative = value < 0.0;
    let magnitude = value.abs();
    let (mantissa, exponent) = decompose(magnitude);
    let (numerator, denominator) = if exponent >= 0 {
        (
            mantissa << u32::try_from(exponent).unwrap_or(0),
            BigUint::one(),
        )
    } else {
        (
            mantissa,
            BigUint::one() << u32::try_from(-exponent).unwrap_or(0),
        )
    };
    let integer = &numerator / &denominator;
    let mut remainder = &numerator % &denominator;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&integer_digits(&integer, radix));
    if !remainder.is_zero() {
        let mut exact = Vec::new();
        while !remainder.is_zero() && exact.len() < MAX_RADIX_FRACTION_DIGITS {
            remainder *= radix;
            let digit = &remainder / &denominator;
            remainder %= &denominator;
            exact.push(radix_digit(&digit));
            let start = exact
                .iter()
                .take_while(|digit| **digit == '0')
                .count()
                .max(1);
            if exact.len() >= start && prefix_round_trips(magnitude, &integer, &exact, radix) {
                break;
            }
        }
        let shortest = shortest_fraction(magnitude, &integer, &exact, radix);
        if !shortest.is_empty() {
            out.push('.');
            out.extend(shortest);
        }
    }
    out
}

fn to_fixed_digits(magnitude: f64, keep: usize) -> String {
    let (mantissa, exponent) = decompose(magnitude);
    let shift = i32::try_from(keep).unwrap_or(0);
    let (numerator, denominator) = power_scaled(&mantissa, exponent, shift);
    let scaled = rounded_quotient(&numerator, &denominator, false);
    let digits = integer_digits(&scaled, 10);
    let width = shift.saturating_add(1);
    let mut padded = String::new();
    for _ in i32::try_from(digits.len()).unwrap_or(0)..width {
        padded.push('0');
    }
    padded.push_str(&digits);
    if keep == 0 {
        return padded;
    }
    let split = padded.len().saturating_sub(keep);
    format!("{}.{}", &padded[..split], &padded[split..])
}

fn signed(text: &str, negative: bool) -> String {
    if negative {
        format!("-{text}")
    } else {
        text.to_owned()
    }
}

#[must_use]
pub fn __ts_aot_number_to_string<'a>(
    ctx: &Ctx<'a>,
    value: f64,
    radix: Option<i32>,
) -> JsString<'a> {
    let radix = match radix {
        None => 10,
        Some(radix) => {
            if !(2..=36).contains(&radix) {
                let message = format!(
                    "Number.prototype.toString() radix must be between 2 and 36, got {radix}"
                );
                __ts_aot_throw_range_error(JsString::from(message.as_str()));
            }
            radix
        }
    };
    if let Some(text) = special(value) {
        return js_string(ctx, text.to_owned());
    }
    let text = if radix == 10 {
        decimal_to_string(value)
    } else {
        radix_to_string(value, radix.unsigned_abs())
    };
    js_string(ctx, text)
}

#[must_use]
pub fn __ts_aot_number_to_fixed<'a>(
    ctx: &Ctx<'a>,
    value: f64,
    digits: Option<i32>,
) -> JsString<'a> {
    let digits = match digits {
        None => 0,
        Some(digits) => {
            if !(0..=100).contains(&digits) {
                let message = format!(
                    "Number.prototype.toFixed() digits must be between 0 and 100, got {digits}"
                );
                __ts_aot_throw_range_error(JsString::from(message.as_str()));
            }
            digits
        }
    };
    if let Some(text) = special(value) {
        return js_string(ctx, text.to_owned());
    }
    if value.abs() >= 1e21 {
        return js_string(ctx, decimal_to_string(value));
    }
    let keep = usize::try_from(digits).unwrap_or(0);
    if value == 0.0 {
        let text = if keep == 0 {
            "0".to_owned()
        } else {
            format!("0.{}", "0".repeat(keep))
        };
        return js_string(ctx, text);
    }
    let rounded = to_fixed_digits(value.abs(), keep);
    js_string(ctx, signed(&rounded, value < 0.0))
}

#[must_use]
pub fn __ts_aot_number_to_exponential<'a>(
    ctx: &Ctx<'a>,
    value: f64,
    digits: Option<i32>,
) -> JsString<'a> {
    if let Some(text) = special(value) {
        return js_string(ctx, text.to_owned());
    }
    let negative = value < 0.0;
    let magnitude = value.abs();
    let fraction = match digits {
        None => {
            let (text, exponent) = digits_and_exponent(magnitude);
            let mantissa = if text.len() > 1 {
                format!("{}.{}", &text[..1], &text[1..])
            } else {
                text
            };
            let printed = exponent - 1;
            let out = format!(
                "{mantissa}e{}{}",
                if printed < 0 { '-' } else { '+' },
                printed.unsigned_abs()
            );
            return js_string(ctx, signed(&out, negative));
        }
        Some(digits) => {
            if !(0..=100).contains(&digits) {
                let message = format!(
                    "Number.prototype.toExponential() digits must be between 0 and 100, got {digits}"
                );
                __ts_aot_throw_range_error(JsString::from(message.as_str()));
            }
            usize::try_from(digits).unwrap_or(0)
        }
    };
    let (digits, exponent) = if magnitude == 0.0 {
        ("0".repeat(fraction + 1), 0)
    } else {
        let (mantissa, binary_exponent) = decompose(magnitude);
        significant_digits(
            &mantissa,
            binary_exponent,
            decimal_exponent(&mantissa, binary_exponent),
            fraction + 1,
            false,
        )
    };
    let mantissa = if fraction == 0 {
        digits
    } else {
        format!("{}.{}", &digits[..1], &digits[1..])
    };
    let out = format!(
        "{mantissa}e{}{}",
        if exponent < 0 { '-' } else { '+' },
        exponent.unsigned_abs()
    );
    js_string(ctx, signed(&out, negative))
}

#[must_use]
pub fn __ts_aot_number_to_precision<'a>(
    ctx: &Ctx<'a>,
    value: f64,
    precision: Option<i32>,
) -> JsString<'a> {
    if let Some(text) = special(value) {
        return js_string(ctx, text.to_owned());
    }
    let precision = match precision {
        None => return js_string(ctx, decimal_to_string(value)),
        Some(precision) => {
            if !(1..=100).contains(&precision) {
                let message = format!(
                    "Number.prototype.toPrecision() precision must be between 1 and 100, got {precision}"
                );
                __ts_aot_throw_range_error(JsString::from(message.as_str()));
            }
            precision
        }
    };
    let keep = usize::try_from(precision).unwrap_or(1);
    if value == 0.0 {
        let text = if keep == 1 {
            "0".to_owned()
        } else {
            format!("0.{}", "0".repeat(keep - 1))
        };
        return js_string(ctx, text);
    }
    let magnitude = value.abs();
    let (binary_mantissa, binary_exponent) = decompose(magnitude);
    let (digits, exponent) = significant_digits(
        &binary_mantissa,
        binary_exponent,
        decimal_exponent(&binary_mantissa, binary_exponent),
        keep,
        false,
    );
    let out = if exponent < -6 || exponent >= precision {
        let mantissa = if keep == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!(
            "{mantissa}e{}{}",
            if exponent < 0 { '-' } else { '+' },
            exponent.unsigned_abs()
        )
    } else if exponent == precision - 1 {
        digits.clone()
    } else if exponent >= 0 {
        let split = usize::try_from(exponent + 1).unwrap_or(digits.len());
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!(
            "0.{}{}",
            "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0)),
            digits
        )
    };
    js_string(ctx, signed(&out, value < 0.0))
}
