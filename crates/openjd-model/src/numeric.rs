// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! How a template's numbers are read: the one home for the rules that turn a
//! YAML or JSON number, or a numeric string (Template Schemas §7.5), into the
//! `i64` an `<integer>` field stores.

use openjd_expr::path_mapping::PathFormat;
use openjd_expr::value::Float64;
use openjd_expr::{ExprType, ExprValue};

/// The exact `i64` value of `v`, or `None` when `v` has a fractional part or
/// lies outside the 64-bit signed range. Never an `as i64` cast, which
/// saturates: `2^63` would become `i64::MAX`.
///
/// Decided by the Expression Language's own `float` to `int` conversion
/// (2026-02 §1.2.3: an error "if value cannot be represented exactly"), so a
/// template field and an expression read an integral float the same way.
pub(crate) fn float_as_exact_i64(v: f64) -> Option<i64> {
    let float = Float64::new(v).ok()?;
    match ExprValue::Float(float).coerce(&ExprType::INT, PathFormat::Posix) {
        Ok(ExprValue::Int(i)) => Some(i),
        _ => None,
    }
}

/// The `i64` a JSON or YAML number denotes, for a template field whose schema
/// type is `<integer>`. An integer-valued float such as `42.0` is accepted. A
/// number with a fractional part is refused, and so is one outside the 64-bit
/// signed range, rather than being rounded or saturated.
///
/// The YAML parser reads an integer outside both the `i64` and `u64` ranges as
/// an `f64`. One that rounds onto an `f64` inside the `i64` range, such as
/// -2^63-1 (which rounds to -2^63), arrives here as that in-range float and is
/// read as it.
pub(crate) fn json_number_as_i64(n: &serde_json::Number) -> Result<i64, String> {
    if let Some(i) = n.as_i64() {
        return Ok(i);
    }
    let Some(f) = n.as_f64() else {
        return Err("Invalid number".to_string());
    };
    if f.fract() != 0.0 {
        return Err(format!("Expected integer, got float: {f}"));
    }
    float_as_exact_i64(f).ok_or_else(|| out_of_range(&n.to_string()))
}

/// The `i64` an `<intstring>` denotes. White space around the number is
/// ignored (Template Schemas §7.5). A string of digits outside the 64-bit
/// signed range gets the same message as a number outside it from
/// [`json_number_as_i64`].
pub(crate) fn intstring_as_i64(s: &str) -> Result<i64, String> {
    use std::num::IntErrorKind;
    s.trim().parse::<i64>().map_err(|e| match e.kind() {
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => out_of_range(&format!("'{s}'")),
        _ => format!("Cannot parse '{s}' as integer"),
    })
}

fn out_of_range(written: &str) -> String {
    format!("Integer {written} is outside the 64-bit signed range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_as_exact_i64_takes_only_exact_in_range_integers() {
        assert_eq!(float_as_exact_i64(4.0), Some(4));
        assert_eq!(float_as_exact_i64(-0.0), Some(0));
        assert_eq!(float_as_exact_i64(i64::MIN as f64), Some(i64::MIN));
        // 2^63 is the f64 nearest i64::MAX, and is outside the range.
        assert_eq!(float_as_exact_i64(i64::MAX as f64), None);
        assert_eq!(float_as_exact_i64(2.5), None);
        assert_eq!(float_as_exact_i64(1e300), None);
        assert_eq!(float_as_exact_i64(f64::NAN), None);
        assert_eq!(float_as_exact_i64(f64::INFINITY), None);
    }

    #[test]
    fn json_number_as_i64_refuses_rather_than_saturates() {
        let n = |s: &str| serde_json::from_str::<serde_json::Number>(s).unwrap();
        assert_eq!(json_number_as_i64(&n("9223372036854775807")), Ok(i64::MAX));
        assert_eq!(json_number_as_i64(&n("-9223372036854775808")), Ok(i64::MIN));
        assert_eq!(json_number_as_i64(&n("42.0")), Ok(42));
        assert_eq!(
            json_number_as_i64(&n("9223372036854775808")),
            Err("Integer 9223372036854775808 is outside the 64-bit signed range".to_string())
        );
        assert!(json_number_as_i64(&n("1e19"))
            .unwrap_err()
            .contains("outside the 64-bit"));
        assert_eq!(
            json_number_as_i64(&n("2.5")),
            Err("Expected integer, got float: 2.5".to_string())
        );
    }

    #[test]
    fn intstring_as_i64_names_overflow_and_keeps_the_parse_message() {
        assert_eq!(intstring_as_i64(" +7 "), Ok(7));
        assert_eq!(
            intstring_as_i64("-9223372036854775809"),
            Err("Integer '-9223372036854775809' is outside the 64-bit signed range".to_string())
        );
        assert_eq!(
            intstring_as_i64("1 000"),
            Err("Cannot parse '1 000' as integer".to_string())
        );
    }
}
