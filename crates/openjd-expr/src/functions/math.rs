// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Math function implementations (min, max, floor, ceil, round, sum).

use crate::error::ExpressionError;
use crate::function_library::EvalContext;
use crate::value::{float_fits_i64, ExprValue, Float64};

type R = Result<ExprValue, ExpressionError>;
type Ctx<'a> = &'a mut dyn EvalContext;

fn min_max_items(a: &[ExprValue], name: &str) -> Result<Vec<ExprValue>, ExpressionError> {
    if a.is_empty() {
        return Err(ExpressionError::new(format!(
            "{name}() requires at least 1 argument"
        )));
    }
    if a.len() == 1 {
        match &a[0] {
            val if val.is_list() => {
                let elements: Vec<ExprValue> =
                    val.list_iter().expect("guard ensures list").collect();
                if elements.is_empty() {
                    return Err(ExpressionError::new(format!(
                        "{name}() requires a non-empty list"
                    )));
                }
                Ok(elements)
            }
            ExprValue::RangeExpr(r) => {
                if r.is_empty() {
                    return Err(ExpressionError::new(format!(
                        "{name}() requires a non-empty list"
                    )));
                }
                if name == "min" {
                    return Ok(vec![ExprValue::Int(
                        r.iter().next().expect("guarded above: range is non-empty"),
                    )]);
                } else {
                    // get(-1) resolves against the exact logical length.
                    Ok(vec![ExprValue::Int(
                        r.get(-1).expect("guarded above: range is non-empty"),
                    )])
                }
            }
            _ => Ok(a.to_vec()),
        }
    } else {
        Ok(a.to_vec())
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn min_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let items = min_max_items(a, "min")?;
    ctx.count_ops(items.len())?;
    let mut result = items[0].clone();
    for item in &items[1..] {
        if result.compare(item)?.is_gt() {
            result = item.clone();
        }
    }
    if items.iter().any(|i| matches!(i, ExprValue::Float(_))) {
        if let ExprValue::Int(i) = &result {
            return Ok(ExprValue::Float(Float64::new(*i as f64)?));
        }
    }
    Ok(result)
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn max_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    let items = min_max_items(a, "max")?;
    ctx.count_ops(items.len())?;
    let mut result = items[0].clone();
    for item in &items[1..] {
        if result.compare(item)?.is_lt() {
            result = item.clone();
        }
    }
    if items.iter().any(|i| matches!(i, ExprValue::Float(_))) {
        if let ExprValue::Int(i) = &result {
            return Ok(ExprValue::Float(Float64::new(*i as f64)?));
        }
    }
    Ok(result)
}

/// Round `mag + frac` to the nearest multiple of `10^k`, ties to even, and
/// return that multiple. `mag` is an integer and `frac` is an exact fraction,
/// `0 <= frac < 1`, so the comparison with the halfway point is exact: for
/// `k >= 1` the halfway point `10^k / 2` is an integer, and `frac` only breaks
/// a tie in the integer remainder. `None` means the multiple overflows `u128`.
fn round_magnitude_to_pow10(mag: u128, frac: f64, k: u32) -> Option<u128> {
    if k == 0 {
        let up = frac > 0.5 || (frac == 0.5 && mag % 2 == 1);
        return mag.checked_add(u128::from(up));
    }
    // 10^39 exceeds 2^129, more than twice any `mag` that reaches here
    // (below 2^127), so every such value rounds to 0.
    let Some(q) = 10u128.checked_pow(k) else {
        return Some(0);
    };
    let (quot, rem) = (mag / q, mag % q);
    let half = q / 2;
    let up = rem > half || (rem == half && (frac > 0.0 || quot % 2 == 1));
    (quot + u128::from(up)).checked_mul(q)
}

/// The i64 with sign `negative` and magnitude `mag`, or an overflow error.
fn signed_i64(negative: bool, mag: u128) -> Result<i64, ExpressionError> {
    let overflow = |_| ExpressionError::integer_overflow();
    let v = i128::try_from(mag).map_err(overflow)?;
    i64::try_from(if negative { -v } else { v }).map_err(overflow)
}

/// `round(x, ndigits)` for `ndigits <= 0`: the exact value of `x` rounded
/// half-to-even to a multiple of `10^-ndigits`, as an i64.
fn round_float_to_int(x: f64, ndigits: i64) -> Result<i64, ExpressionError> {
    // round(v, n) is 0 whenever 10^|n| exceeds 2·|v|, and every finite f64
    // is below 10^309 / 2. This also keeps the negation below in range
    // (ndigits can be i64::MIN).
    if ndigits <= -309 {
        return Ok(0);
    }
    let k = u32::try_from(-ndigits).expect("0 <= -ndigits <= 308");
    let ax = x.abs();
    // 2^127, exactly. Below it the integer part fits in u128 exactly.
    const TWO_POW_127: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
    if ax < TWO_POW_127 {
        let int_part = ax.trunc();
        // Both conversions are exact: int_part is an integer below 2^127,
        // and the fraction of an f64 is itself representable.
        let mag = round_magnitude_to_pow10(int_part as u128, ax - int_part, k)
            .ok_or_else(ExpressionError::integer_overflow)?;
        return signed_i64(x.is_sign_negative(), mag);
    }
    // |x| >= 2^127 is an integer. The result is 0 when |x| <= 10^k / 2.
    // Otherwise it is a nonzero multiple of 10^k, so at least 10^k, and at
    // least |x| - 10^k / 2; one of those exceeds 2^126, far outside the
    // i64 range. `{:.0}` writes the exact decimal value of |x|, and
    // 10^k / 2 = 5·10^(k-1) is the k-digit number 50...0. No f64 this large
    // equals it (5^k·2^(k-1) is an f64 only if 5^k < 2^53, so k <= 22, and
    // then it is below 2^127), so comparing the leading digit is enough.
    if !ax.is_finite() {
        return Err(ExpressionError::integer_overflow());
    }
    let digits = format!("{ax:.0}");
    let k = k as usize;
    if digits.len() < k || (digits.len() == k && digits.as_bytes()[0] < b'5') {
        Ok(0)
    } else {
        Err(ExpressionError::integer_overflow())
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn floor_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            let v = f.floor();
            if !float_fits_i64(v) {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(v as i64))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn floor_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(*i)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn ceil_float(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            let v = f.ceil();
            if !float_fits_i64(v) {
                return Err(ExpressionError::integer_overflow());
            }
            Ok(ExprValue::Int(v as i64))
        }
        _ => Err(ExpressionError::type_error("type error")),
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn ceil_int(_: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Int(i) => Ok(ExprValue::Int(*i)),
        _ => Err(ExpressionError::type_error("type error")),
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn round_fn(ctx: Ctx, a: &[ExprValue]) -> R {
    match &a[0] {
        ExprValue::Float(f) => {
            // round(x) rounds as round(x, 0) does.
            let ndigits = a
                .get(1)
                .and_then(|v| match v {
                    ExprValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            if ndigits > 0 {
                // The formatted output grows with ndigits; charge the
                // budget before computing so a huge precision (e.g.
                // round(1.5, 10**18)) trips the operation limit instead
                // of attempting a matching allocation.
                let requested = usize::try_from(ndigits).unwrap_or(usize::MAX);
                ctx.count_string_ops(requested)?;
                // Rust's formatting machinery panics ("Formatting
                // argument out of range") for precision above
                // u16::MAX, and operation budgeting alone admits
                // precisions like 100,000. An f64's exact decimal
                // expansion has at most 1074 fractional digits, so
                // format at most that many and zero-fill the rest —
                // the digits past that point are always zero, so the
                // output is unchanged — with the full length checked
                // against the memory budget before allocation.
                const MAX_F64_FRACTION_DIGITS: usize = 1074;
                let prec = requested.min(MAX_F64_FRACTION_DIGITS);
                // `{:.prec$}` rounds the exact value of x half-to-even to
                // `prec` places, and parsing that text gives the nearest
                // f64, which is CPython's algorithm. Scaling by 10^ndigits
                // instead would round twice (round(2.675, 2) is 2.67, not
                // 2.68). The text reads back as the value, and a finite
                // value cannot round to infinity at a positive precision.
                let mut text = format!("{:.prec$}", f.value(), prec = prec);
                let rounded: f64 = text
                    .parse()
                    .expect("Rust's own float formatting parses as f64");
                let pad = requested - prec;
                if pad > 0 {
                    ctx.check_memory(text.len() + pad)?;
                    text.reserve_exact(pad);
                    for _ in 0..pad {
                        text.push('0');
                    }
                }
                Ok(ExprValue::Float(Float64::with_str(rounded, text)?))
            } else {
                // round(x) and round(x, ndigits <= 0) return int (§2.2.2).
                Ok(ExprValue::Int(round_float_to_int(f.value(), ndigits)?))
            }
        }
        ExprValue::Int(i) => {
            let ndigits = a
                .get(1)
                .and_then(|v| match v {
                    ExprValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            if ndigits >= 0 {
                Ok(ExprValue::Int(*i))
            } else {
                // 10^20 / 2 exceeds i64::MAX, so every i64 rounds to 0
                // for ndigits <= -20. Short-circuiting also keeps the
                // negation below from overflowing on extreme ndigits.
                if ndigits <= -20 {
                    return Ok(ExprValue::Int(0));
                }
                // Exact integer arithmetic; 10^19 needs more than 64 bits.
                let k = u32::try_from(-ndigits).expect("1 <= -ndigits <= 19");
                let mag = round_magnitude_to_pow10(u128::from(i.unsigned_abs()), 0.0, k)
                    .expect("a multiple of 10^19 near an i64 fits u128");
                Ok(ExprValue::Int(signed_i64(*i < 0, mag)?))
            }
        }
        _ => Err(ExpressionError::new("round() requires numeric argument")),
    }
}

#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub fn sum_list(ctx: Ctx, a: &[ExprValue]) -> R {
    if let Some(iter) = a[0].list_iter() {
        let mut int_sum: i64 = 0;
        // A `list[float]` sums to a float even when empty: the signature
        // `sum(list[float]) -> float` fixes the type, not the elements seen.
        let mut is_float = matches!(&a[0], ExprValue::ListFloat(_));
        let mut float_sum: f64 = 0.0;
        for e in iter {
            ctx.count_op()?;
            match e {
                ExprValue::Int(i) => {
                    int_sum = int_sum
                        .checked_add(i)
                        .ok_or_else(ExpressionError::integer_overflow)?;
                    float_sum += i as f64;
                }
                ExprValue::Float(f) => {
                    is_float = true;
                    float_sum += f.value();
                }
                _ => return Err(ExpressionError::new("sum() elements must be numeric")),
            }
        }
        if is_float {
            Ok(ExprValue::Float(Float64::new(float_sum)?))
        } else {
            Ok(ExprValue::Int(int_sum))
        }
    } else if let ExprValue::RangeExpr(r) = &a[0] {
        let mut sum: i64 = 0;
        for v in r.iter() {
            ctx.count_op()?;
            sum = sum
                .checked_add(v)
                .ok_or_else(ExpressionError::integer_overflow)?;
        }
        Ok(ExprValue::Int(sum))
    } else {
        Err(ExpressionError::new("sum() requires list or range_expr"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The f64 rounding that `round(int, ndigits < 0)` used before it
    /// switched to integer arithmetic.
    fn old_f64_round(i: i64, k: u32) -> f64 {
        let factor = 10f64.powi(k as i32);
        let y = i as f64 / factor;
        let rounded = y.round();
        let r = if (y - rounded).abs() == 0.5 && rounded as i64 % 2 != 0 {
            rounded - y.signum()
        } else {
            rounded
        };
        r * factor
    }

    fn new_round(i: i64, k: u32) -> i64 {
        let mag = round_magnitude_to_pow10(u128::from(i.unsigned_abs()), 0.0, k).unwrap();
        signed_i64(i < 0, mag).unwrap()
    }

    /// A property test without a property-testing dependency: for
    /// |x| < 2^53 the f64 quotient x / 10^k is off by under 1/10^k, and an
    /// exact quotient that is not a tie is at least 1/10^k from the halfway
    /// point, so the old f64 rounding was already exact there, ties
    /// included. The integer rounding must agree with it on every input.
    #[test]
    fn integer_rounding_matches_f64_below_2_pow_53() {
        // xorshift64*, fixed seed, so a failure reproduces.
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let limit = 1i64 << 53;
        for _ in 0..200_000 {
            let r = next();
            let k = (r % 19) as u32 + 1;
            let q = 10i64.saturating_pow(k).min(limit);
            // Half the samples sit on a tie, to exercise half-to-even.
            let x = if (r >> 8) & 1 == 0 {
                (next() % limit as u64) as i64
            } else {
                ((next() % (limit / q) as u64) as i64) * q + q / 2
            };
            let x = if (r >> 9) & 1 == 0 { x } else { -x };
            if x.abs() >= limit {
                continue;
            }
            assert_eq!(
                new_round(x, k) as f64,
                old_f64_round(x, k),
                "x = {x}, k = {k}"
            );
        }
    }

    #[test]
    fn round_magnitude_ties_go_to_even() {
        assert_eq!(round_magnitude_to_pow10(2, 0.5, 0), Some(2));
        assert_eq!(round_magnitude_to_pow10(3, 0.5, 0), Some(4));
        assert_eq!(round_magnitude_to_pow10(3, 0.25, 0), Some(3));
        assert_eq!(round_magnitude_to_pow10(3, 0.75, 0), Some(4));
        assert_eq!(round_magnitude_to_pow10(250, 0.0, 2), Some(200));
        assert_eq!(round_magnitude_to_pow10(350, 0.0, 2), Some(400));
        // A fraction past an integer tie rounds up.
        assert_eq!(round_magnitude_to_pow10(250, 0.5, 2), Some(300));
        assert_eq!(round_magnitude_to_pow10(249, 0.75, 2), Some(200));
        assert_eq!(round_magnitude_to_pow10(u128::MAX, 0.0, 39), Some(0));
        assert_eq!(round_magnitude_to_pow10(u128::MAX, 0.75, 0), None);
        assert_eq!(round_magnitude_to_pow10(u128::MAX, 0.0, 1), None);
    }

    #[test]
    fn signed_i64_range() {
        assert_eq!(signed_i64(false, i64::MAX as u128).ok(), Some(i64::MAX));
        assert!(signed_i64(false, 1 << 63).is_err());
        assert_eq!(signed_i64(true, 1 << 63).ok(), Some(i64::MIN));
        assert!(signed_i64(true, (1 << 63) + 1).is_err());
        assert!(signed_i64(true, u128::MAX).is_err());
        assert_eq!(signed_i64(true, 0).ok(), Some(0));
    }
}
