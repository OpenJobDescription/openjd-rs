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

/// Why a string is not an `<intstring>` this crate can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntStringError {
    /// The string is a base-10 integer outside the 64-bit signed range.
    OutOfRange,
    /// The string is not a base-10 integer.
    NotAnInteger,
}

/// The `i64` an `<intstring>` denotes: a base-10 integer with an optional
/// sign, with white space around it ignored (Template Schemas §7.5).
///
/// The value is read by openjd-expr's own `string` to `int` conversion
/// (`ExprValue::from_str_coerce`, 2026-02 Expression Language §1.2.3), which
/// also reads a value submitted for an INT parameter, so a numeric string reads
/// the same in a template field, in a resolved format string and as a submitted
/// value. The error says whether the string was an integer outside the range,
/// so each caller can say so in its own located message.
pub(crate) fn parse_intstring(s: &str) -> Result<i64, IntStringError> {
    match ExprValue::from_str_coerce(s, &ExprType::INT, PathFormat::Posix) {
        Ok(ExprValue::Int(i)) => Ok(i),
        _ if is_integer_text(s.trim()) => Err(IntStringError::OutOfRange),
        _ => Err(IntStringError::NotAnInteger),
    }
}

/// An optional sign and one or more ASCII digits.
fn is_integer_text(t: &str) -> bool {
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The `i64` a JSON or YAML number denotes, for a template field whose schema
/// type is `<integer>`. An integer-valued float such as `42.0` is accepted. A
/// number with a fractional part is refused, and so is one outside the 64-bit
/// signed range, rather than being rounded or saturated.
///
/// The YAML parser reads an integer above `u64::MAX` or below `i64::MIN` as
/// the nearest `f64`, so the text it was written with is gone by the time it
/// reaches this crate, and the message names the number as that `f64`
/// (`1e+20`). Each such integer reads as an `f64` outside the range, except
/// those just below `i64::MIN`, which read as exactly `-2^63`. An `f64` of
/// `-2^63` is therefore refused too: it cannot be told apart from them. The
/// integer `-9223372036854775808` itself is read as an `i64`, before this
/// point, and is accepted.
pub(crate) fn json_number_as_i64(n: &serde_json::Number) -> Result<i64, String> {
    if let Some(i) = n.as_i64() {
        return Ok(i);
    }
    if let Some(u) = n.as_u64() {
        return Err(too_large(&u.to_string()));
    }
    let Some(f) = n.as_f64() else {
        return Err("Invalid number".to_string());
    };
    if f.fract() != 0.0 {
        return Err(format!("Expected integer, got float: {f}"));
    }
    let text = openjd_expr::value::format_float(f);
    if f == i64::MIN as f64 {
        return Err(format!(
            "Cannot convert {text} to int: a YAML integer below -9223372036854775808 is read as \
             this float; write -9223372036854775808 as an integer"
        ));
    }
    float_as_exact_i64(f).ok_or_else(|| {
        if f > 0.0 {
            too_large(&text)
        } else {
            format!("Cannot convert {text} to int: number too small to fit in target type")
        }
    })
}

/// The `i64` an `<intstring>` in a template field denotes ([`parse_intstring`]),
/// with the field's decode message. A string outside the range gets the
/// message openjd-expr, and openjd-model-for-python, give for it:
/// `Cannot convert '9223372036854775808' to int: number too large to fit in
/// target type`.
pub(crate) fn intstring_as_i64(s: &str) -> Result<i64, String> {
    parse_intstring(s).map_err(|e| match e {
        IntStringError::OutOfRange => {
            let size = if s.trim().starts_with('-') {
                "small"
            } else {
                "large"
            };
            format!("Cannot convert '{s}' to int: number too {size} to fit in target type")
        }
        IntStringError::NotAnInteger => format!("Cannot parse '{s}' as integer"),
    })
}

fn too_large(text: &str) -> String {
    format!("Cannot convert {text} to int: number too large to fit in target type")
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
        let err = |s: &str| json_number_as_i64(&n(s)).unwrap_err();
        assert_eq!(json_number_as_i64(&n("9223372036854775807")), Ok(i64::MAX));
        assert_eq!(json_number_as_i64(&n("-9223372036854775808")), Ok(i64::MIN));
        assert_eq!(json_number_as_i64(&n("42.0")), Ok(42));
        // The f64 next above -2^63 is in range and integral.
        assert_eq!(
            json_number_as_i64(&n("-9.223372036854774784e18")),
            Ok(-9_223_372_036_854_774_784)
        );
        assert_eq!(
            err("9223372036854775808"),
            "Cannot convert 9223372036854775808 to int: number too large to fit in target type"
        );
        assert_eq!(
            err("1e19"),
            "Cannot convert 1e+19 to int: number too large to fit in target type"
        );
        assert_eq!(
            err("-1e19"),
            "Cannot convert -1e+19 to int: number too small to fit in target type"
        );
        // -2^63 as a float, which is what a YAML integer just below i64::MIN
        // reads as.
        assert_eq!(
            err("-9223372036854775808.0"),
            "Cannot convert -9.223372036854776e+18 to int: a YAML integer below \
             -9223372036854775808 is read as this float; write -9223372036854775808 as an \
             integer"
        );
        assert_eq!(err("2.5"), "Expected integer, got float: 2.5");
    }

    #[test]
    fn parse_intstring_classifies_what_it_refuses() {
        assert_eq!(parse_intstring(" +7 "), Ok(7));
        assert_eq!(
            parse_intstring("\u{3000}-9223372036854775808"),
            Ok(i64::MIN)
        );
        for out in [
            "9223372036854775808",
            "-9223372036854775809",
            "+99999999999999999999",
        ] {
            assert_eq!(
                parse_intstring(out),
                Err(IntStringError::OutOfRange),
                "{out}"
            );
        }
        // Leading digits that overflow before a character that is not a digit
        // still make a string that is not an integer.
        for not in [
            "99999999999999999999abc",
            "1 000",
            "",
            " ",
            "1.0",
            "\u{663}",
            "+",
            "--1",
        ] {
            assert_eq!(
                parse_intstring(not),
                Err(IntStringError::NotAnInteger),
                "{not:?}"
            );
        }
    }

    #[test]
    fn intstring_as_i64_messages() {
        assert_eq!(intstring_as_i64(" +7 "), Ok(7));
        assert_eq!(
            intstring_as_i64("-9223372036854775809"),
            Err(
                "Cannot convert '-9223372036854775809' to int: number too small to fit in \
                 target type"
                    .to_string()
            )
        );
        assert_eq!(
            intstring_as_i64("1 000"),
            Err("Cannot parse '1 000' as integer".to_string())
        );
    }

    /// The overflow message is the one openjd-expr gives for the same string,
    /// so a template field and a submitted value say the same thing.
    #[test]
    fn the_overflow_message_is_openjd_exprs() {
        for s in [
            "9223372036854775808",
            "-9223372036854775809",
            " 99999999999999999999 ",
        ] {
            let expr =
                ExprValue::from_str_coerce(s, &ExprType::INT, PathFormat::Posix).unwrap_err();
            assert_eq!(intstring_as_i64(s).unwrap_err(), expr, "{s:?}");
        }
    }
}
