// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Task parameter space and range resolution.

use std::borrow::Cow;

use indexmap::IndexMap;

use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;
use openjd_expr::value::Float64;
use openjd_expr::ExprValue;
use openjd_expr::RangeExpr;

use crate::error::ModelError;
use crate::job;
use crate::template;
use crate::template::validate_v2023_09::EffectiveLimits;
use openjd_expr::ExpressionError;

/// Resolve an optional float field (`hostRequirements` amount `min`/`max`).
///
/// A single whole-field expression resolves with target type `float?`
/// (Expression Language §1.3.2 — optional field): `Ok(None)` means the
/// expression resolved to `null` and the field is treated as omitted.
/// Multi-segment strings concatenate and parse, whitespace-tolerated.
pub(super) fn resolve_to_f64(
    fs: &openjd_expr::FormatString,
    symtab: &SymbolTable,
    context: &str,
) -> Result<Option<f64>, ModelError> {
    let target = openjd_expr::ExprType::union(vec![
        openjd_expr::ExprType::FLOAT,
        openjd_expr::ExprType::NULLTYPE,
    ]);
    let resolved = fs
        .resolve_with(
            symtab,
            &openjd_expr::FormatStringOptions::new()
                .with_path_format(PathFormat::Posix)
                .with_target_type(&target),
        )
        .map_err(|e| ModelError::FormatStringError {
            message: format!("{context}: {e}"),
            input: Some(fs.raw().to_string()),
            start: None,
            end: None,
        })?;
    let value = match resolved {
        openjd_expr::ExprValue::Null => return Ok(None),
        // A coerced Float is always finite: Float64 excludes NaN and the
        // infinities by construction.
        openjd_expr::ExprValue::Float(f) => f.value(),
        // Multi-segment string: parse the concatenated text. Only this
        // path can produce a non-finite value (e.g. "1e999" parses to
        // inf), so the finite check lives here where the user's text is
        // available for the message.
        other => {
            let s = other.to_display_string();
            let value = s.trim().parse::<f64>().map_err(|_| {
                ModelError::Expression(ExpressionError::new(format!(
                    "{context}: '{s}' is not a valid number"
                )))
            })?;
            if !value.is_finite() {
                return Err(ModelError::Expression(ExpressionError::new(format!(
                    "{context}: '{s}' is not a finite number"
                ))));
            }
            value
        }
    };
    Ok(Some(value))
}

/// Resolve a list of FormatStrings to strings.
///
/// Each element is a list item, so per Expression Language §1.3.2 a
/// whole-field expression resolves with target type
/// `string? | list[string]`: a `null` result skips the element, a list
/// result flattens inline, and a string is a single element. Callers
/// enforcing a non-empty list must re-check after resolution, since
/// null-skips can empty it.
pub(super) fn resolve_string_list(
    vals: &[openjd_expr::FormatString],
    symtab: &SymbolTable,
) -> Result<Vec<String>, ModelError> {
    let target = openjd_expr::ExprType::union(vec![
        openjd_expr::ExprType::NULLTYPE,
        openjd_expr::ExprType::STRING,
        openjd_expr::ExprType::list(openjd_expr::ExprType::STRING),
    ]);
    let mut out = Vec::new();
    for fs in vals {
        let value = fs
            .resolve_with(
                symtab,
                &openjd_expr::FormatStringOptions::new()
                    .with_path_format(PathFormat::Posix)
                    .with_target_type(&target),
            )
            .map_err(|e| ModelError::FormatStringError {
                message: e.to_string(),
                input: Some(fs.raw().to_string()),
                start: None,
                end: None,
            })?;
        match value {
            openjd_expr::ExprValue::Null => continue,
            val if val.is_list() => {
                for elem in val.list_elements().unwrap_or_default() {
                    out.push(elem.to_display_string());
                }
            }
            openjd_expr::ExprValue::String(s) => out.push(s),
            other => out.push(other.to_display_string()),
        }
    }
    Ok(out)
}

/// Resolve a StepParameterSpaceDefinition into a StepParameterSpace with concrete ranges.
pub(super) fn resolve_parameter_space(
    ps: &template::StepParameterSpaceDefinition,
    symtab: &SymbolTable,
    limits: &EffectiveLimits,
) -> Result<job::StepParameterSpace, ModelError> {
    let mut defs = IndexMap::new();
    for tp in &ps.task_parameter_definitions {
        let name = tp.name().to_string();
        let param = resolve_task_parameter(tp, symtab, limits)?;
        defs.insert(name, param);
    }
    Ok(job::StepParameterSpace {
        task_parameter_definitions: defs,
        combination: ps.combination.clone(),
    })
}

fn resolve_task_parameter(
    tp: &template::TaskParameterDefinition,
    symtab: &SymbolTable,
    limits: &EffectiveLimits,
) -> Result<job::TaskParameter, ModelError> {
    match tp {
        template::TaskParameterDefinition::INT(p) => {
            let range = resolve_int_range(&p.range, symtab, p.name.as_str(), limits)?;
            Ok(job::TaskParameter::Int {
                range,
                chunks: None,
            })
        }
        template::TaskParameterDefinition::FLOAT(p) => {
            let range = resolve_float_range(&p.range, symtab, p.name.as_str(), limits)?;
            Ok(job::TaskParameter::Float { range })
        }
        template::TaskParameterDefinition::STRING(p) => {
            let range = resolve_string_range(&p.range, symtab, p.name.as_str(), limits)?;
            Ok(job::TaskParameter::String { range })
        }
        template::TaskParameterDefinition::PATH(p) => {
            let range = resolve_string_range(&p.range, symtab, p.name.as_str(), limits)?;
            Ok(job::TaskParameter::Path { range })
        }
        template::TaskParameterDefinition::CHUNK_INT(p) => {
            let range = resolve_int_range(&p.range, symtab, p.name.as_str(), limits)?;
            // CHUNK[INT] regroups values into generated RangeExpr chunks,
            // which bound values to |v| < 2^62. List ranges accept full
            // i64, so reject out-of-bound values here — at job creation,
            // with a path-annotated error — rather than panicking when a
            // chunk is built during iteration.
            if let job::TaskParamRange::List(values) = &range {
                if let Some(v) = values
                    .iter()
                    .find(|v| v.unsigned_abs() >= openjd_expr::MAX_RANGE_VALUE_MAGNITUDE as u64)
                {
                    return Err(ModelError::DecodeValidation(format!(
                        "Task parameter '{}': value {} exceeds the CHUNK[INT] \
                         range value bound (magnitude must be below 2^62)",
                        p.name.as_str(),
                        v
                    )));
                }
            }
            // Chunks fields are `<integer> | <intstring>`: a single
            // whole-field expression resolves with target type `int`
            // (Expression Language §1.2.3 — every field gives its
            // expression a target type; there are no null semantics
            // here), so `{{ 4.0 }}` coerces to 4. A multi-segment string
            // concatenates and parses like Python's int(), tolerating
            // surrounding whitespace. Template validation applies the same
            // targeting, so the two stages accept identical values.
            let resolve_chunk_int = |fs: &crate::FormatString,
                                     field: &str|
             -> Result<i64, ModelError> {
                let value = fs
                    .resolve_with(
                        symtab,
                        &openjd_expr::FormatStringOptions::new()
                            .with_path_format(PathFormat::Posix)
                            .with_target_type(&openjd_expr::ExprType::INT),
                    )
                    .map_err(|e| {
                        ModelError::Expression(ExpressionError::new(format!("chunks.{field}: {e}")))
                    })?;
                match value {
                    openjd_expr::ExprValue::Int(n) => Ok(n),
                    other => {
                        let s = other.to_display_string();
                        s.trim().parse::<i64>().map_err(|_| {
                            ModelError::Expression(ExpressionError::new(format!(
                                "chunks.{field}: '{s}' is not a valid integer"
                            )))
                        })
                    }
                }
            };
            let default_task_count = match &p.chunks.default_task_count {
                template::IntOrFormatString::Int(n) => (*n).max(1) as usize,
                template::IntOrFormatString::FormatString(fs) => {
                    let count = resolve_chunk_int(fs, "defaultTaskCount")?;
                    // §3.4.1.5 sets a minimum of 1. The bound cannot be applied
                    // at decode when the value is a format string, so it is
                    // applied here. Rejecting rather than clamping: a resolved 0
                    // silently became 1, which ran the job with a chunk shape the
                    // author never asked for.
                    if count < 1 {
                        return Err(ModelError::Expression(ExpressionError::new(format!(
                            "chunks.defaultTaskCount: resolved to {count}, but must be >= 1"
                        ))));
                    }
                    count as usize
                }
            };
            let target_runtime_seconds = p
                .chunks
                .target_runtime_seconds
                .as_ref()
                .map(|v| match v {
                    template::IntOrFormatString::Int(n) => Ok(Some((*n).max(0) as usize)),
                    template::IntOrFormatString::FormatString(fs) => {
                        // Optional field: target type `int?` — a
                        // whole-field null means "field omitted".
                        let target = openjd_expr::ExprType::union(vec![
                            openjd_expr::ExprType::INT,
                            openjd_expr::ExprType::NULLTYPE,
                        ]);
                        let value = fs
                            .resolve_with(
                                symtab,
                                &openjd_expr::FormatStringOptions::new()
                                    .with_path_format(PathFormat::Posix)
                                    .with_target_type(&target),
                            )
                            .map_err(|e| {
                                ModelError::Expression(ExpressionError::new(format!(
                                    "chunks.targetRuntimeSeconds: {e}"
                                )))
                            })?;
                        let seconds = match value {
                            openjd_expr::ExprValue::Null => return Ok(None),
                            openjd_expr::ExprValue::Int(n) => n,
                            other => {
                                let s = other.to_display_string();
                                s.trim().parse::<i64>().map_err(|_| {
                                    ModelError::Expression(ExpressionError::new(format!(
                                        "chunks.targetRuntimeSeconds: '{s}' is not a valid integer"
                                    )))
                                })?
                            }
                        };
                        // §3.4.1.5 sets a minimum of 0; same deferral as
                        // defaultTaskCount above, and the same reason to reject
                        // rather than clamp.
                        if seconds < 0 {
                            return Err(ModelError::Expression(ExpressionError::new(format!(
                                "chunks.targetRuntimeSeconds: resolved to {seconds}, but must be >= 0"
                            ))));
                        }
                        Ok(Some(seconds as usize))
                    }
                })
                .transpose()?
                .flatten();
            let chunks = job::ResolvedChunks {
                default_task_count,
                target_runtime_seconds,
                range_constraint: p.chunks.range_constraint.clone(),
            };
            Ok(job::TaskParameter::ChunkInt { range, chunks })
        }
    }
}

fn resolve_int_range(
    range: &template::IntRange,
    symtab: &SymbolTable,
    param_name: &str,
    limits: &EffectiveLimits,
) -> Result<job::TaskParamRange<i64>, ModelError> {
    match range {
        template::IntRange::List(items) => {
            let ints: Vec<i64> = items.iter().map(|i| i.0).collect();
            if ints.len() > limits.max_task_param_range_len {
                return Err(ModelError::DecodeValidation(format!(
                    "Task parameter '{}' range exceeds {} elements ({} elements)",
                    param_name,
                    limits.max_task_param_range_len,
                    ints.len()
                )));
            }
            Ok(job::TaskParamRange::List(ints))
        }
        template::IntRange::Expression(expr) => {
            // Try typed evaluation first — may directly yield a RangeExpr or list[int].
            // For multi-segment format strings (e.g., "1-{{Param.Count}}"), typed
            // evaluation fails and we fall through to string resolution, which
            // concatenates segments and parses the result as a range expression.
            // Any real evaluation errors (division by zero, type errors) will be
            // caught by the string resolution fallback path.
            if let Ok(val) = expr.resolve_with(
                symtab,
                &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
            ) {
                match val {
                    // Range expressions are not length-capped; only the list
                    // forms are. See `EffectiveLimits::max_task_param_range_len`.
                    ExprValue::RangeExpr(r) => {
                        return Ok(job::TaskParamRange::RangeExpr(r));
                    }
                    val if val.is_list() => {
                        let elements = val.list_elements().unwrap();
                        let ints: Result<Vec<i64>, _> = elements
                            .iter()
                            .map(|e| match e {
                                ExprValue::Int(i) => Ok(*i),
                                other => Err(ModelError::Expression(ExpressionError::new(
                                    format!("Expected int in range, got {}", other.type_name()),
                                ))),
                            })
                            .collect();
                        let ints = ints?;
                        if ints.len() > limits.max_task_param_range_len {
                            return Err(ModelError::DecodeValidation(format!(
                                "Task parameter '{}' range exceeds {} elements ({} elements)",
                                param_name,
                                limits.max_task_param_range_len,
                                ints.len()
                            )));
                        }
                        return Ok(job::TaskParamRange::List(ints));
                    }
                    _ => {}
                }
            }
            let resolved = expr
                .resolve_string_with(
                    symtab,
                    &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
                )
                .map_err(ModelError::Expression)?;
            let range_expr: RangeExpr = resolved
                .parse()
                .map_err(|e: openjd_expr::ExpressionError| ModelError::Expression(e))?;
            Ok(job::TaskParamRange::RangeExpr(range_expr))
        }
    }
}

/// A `Float64` with no preserved spelling — a `<float>` literal, or a value whose
/// text could not be carried. Renders through `format_float`.
fn float64(value: f64, param_name: &str) -> Result<Float64, ModelError> {
    Float64::new(value).map_err(|_| {
        ModelError::Expression(ExpressionError::new(format!(
            "FLOAT parameter '{param_name}' range value {value} is not finite"
        )))
    })
}

/// Template Schemas §7.5 rule 1, keeping the sign and one digit: `'02.50'` ->
/// `2.50`, `'007'` -> `7`, `'000'` -> `0`, `'0.50'` unchanged. Textual, so it
/// cannot lose precision or change notation: `'01E+2'` -> `1E+2`.
pub(crate) fn strip_redundant_leading_zeros(text: &str) -> Cow<'_, str> {
    let sign_len = usize::from(text.starts_with(['+', '-']));
    let digits = &text[sign_len..];
    let zeros = digits.bytes().take_while(|b| *b == b'0').count();
    // Redundant only when another digit follows. Otherwise the last zero is the
    // integer part, as in '0.50' or '000'.
    let strip = if digits.as_bytes().get(zeros).is_some_and(u8::is_ascii_digit) {
        zeros
    } else {
        zeros.saturating_sub(1)
    };
    if strip == 0 {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() - strip);
    out.push_str(&text[..sign_len]);
    out.push_str(&digits[strip..]);
    Cow::Owned(out)
}

fn resolve_float_range(
    range: &template::FloatRange,
    symtab: &SymbolTable,
    param_name: &str,
    limits: &EffectiveLimits,
) -> Result<Vec<Float64>, ModelError> {
    let floats: Vec<Float64> = match range {
        template::FloatRange::List(items) => items
            .iter()
            .map(|v| match v {
                // `2.50` and `2.5` are the same literal after parsing, so a
                // `<float>` makes no request about how it renders.
                template::FloatRangeItem::Float(f) => float64(*f, param_name),
                template::FloatRangeItem::FormatString(fs) => {
                    let resolved = fs
                        .resolve_string_with(
                            symtab,
                            &openjd_expr::FormatStringOptions::new()
                                .with_path_format(PathFormat::Posix),
                        )
                        .map_err(ModelError::Expression)?;
                    let trimmed = resolved.trim();
                    let value = trimmed.parse::<f64>().map_err(|_| {
                        ModelError::Expression(ExpressionError::new(format!(
                            "Cannot parse '{}' as float",
                            resolved
                        )))
                    })?;
                    if !value.is_finite() {
                        return Err(ModelError::Expression(ExpressionError::new(format!(
                            "FLOAT parameter '{param_name}' range value '{resolved}' is not finite"
                        ))));
                    }
                    // §7.5 rule 2: the f64 cannot carry the decimal places, so
                    // the text rides alongside it.
                    let text = strip_redundant_leading_zeros(trimmed);
                    // Same per-element cap resolve_string_range applies, for the
                    // same reason: a <FormatString> resolves to arbitrary length,
                    // and this text is what lands on a command line.
                    //
                    // Over the cap this downgrades to value-only where the STRING
                    // and PATH paths error. Deliberate: before this change a long
                    // <floatstring> was parsed to an f64 and its text discarded,
                    // so erroring would reject templates that were valid, whereas
                    // a STRING element over the cap was always an error.
                    if text.chars().count() > limits.max_task_param_string_len {
                        return float64(value, param_name);
                    }
                    Float64::with_str(value, text.into_owned()).map_err(ModelError::Expression)
                }
            })
            .collect::<Result<Vec<_>, _>>()?,
        template::FloatRange::Expression(expr) => {
            // Typed evaluation — must yield a list. Propagate the actual error
            // if evaluation fails.
            match expr.resolve_with(
                symtab,
                &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
            ) {
                Ok(val) if val.is_list() => {
                    let elements = val.list_elements().unwrap();
                    elements
                        .iter()
                        .map(|e| match e {
                            // No text: an expression element came from a literal
                            // or an int, so `{{ [2.50] }}` is 2.5 (§7.5).
                            ExprValue::Float(f) => float64(f.value(), param_name),
                            ExprValue::Int(i) => float64(*i as f64, param_name),
                            other => Err(ModelError::Expression(ExpressionError::new(format!(
                                "Expected float in range, got {}",
                                other.type_name()
                            )))),
                        })
                        .collect::<Result<Vec<_>, _>>()?
                }
                Ok(_) => {
                    return Err(ModelError::Expression(ExpressionError::new(
                        "Float range expression must evaluate to a list",
                    )));
                }
                Err(e) => {
                    return Err(ModelError::Expression(ExpressionError::new(format!(
                        "Float range expression: {e}"
                    ))));
                }
            }
        }
    };
    if floats.len() > limits.max_task_param_range_len {
        return Err(ModelError::DecodeValidation(format!(
            "Task parameter '{}' range exceeds {} elements ({} elements)",
            param_name,
            limits.max_task_param_range_len,
            floats.len()
        )));
    }
    Ok(floats)
}

fn resolve_string_range(
    range: &template::StringRange,
    symtab: &SymbolTable,
    param_name: &str,
    limits: &EffectiveLimits,
) -> Result<Vec<String>, ModelError> {
    let resolved: Vec<String> = match range {
        // Each range element is a list item, so per Expression Language
        // §1.3.2 a whole-field expression resolves with target type
        // `string? | list[string]`: a `null` result skips the element, a
        // list result flattens inline (one range element per list
        // element), and a string is a single element. This is what lets a
        // template mix literal elements with expansions, e.g.
        // `range: ["first", "{{ RawParam.Paths }}", "last"]`; a range
        // that is entirely one expression can also use the whole-field
        // `<ListExpressionString>` form (§1.3.12) handled below.
        template::StringRange::List(items) => {
            let target = openjd_expr::ExprType::union(vec![
                openjd_expr::ExprType::NULLTYPE,
                openjd_expr::ExprType::STRING,
                openjd_expr::ExprType::list(openjd_expr::ExprType::STRING),
            ]);
            let mut out = Vec::new();
            for fs in items {
                let value = fs
                    .resolve_with(
                        symtab,
                        &openjd_expr::FormatStringOptions::new()
                            .with_path_format(PathFormat::Posix)
                            .with_target_type(&target),
                    )
                    .map_err(ModelError::Expression)?;
                match value {
                    openjd_expr::ExprValue::Null => continue,
                    val if val.is_list() => {
                        for elem in val.list_elements().unwrap_or_default() {
                            out.push(elem.to_display_string());
                        }
                    }
                    openjd_expr::ExprValue::String(s) => out.push(s),
                    other => out.push(other.to_display_string()),
                }
            }
            // The ≥1-element rule is checked on field presence at decode,
            // but null-skips can empty the list after resolution.
            if out.is_empty() {
                return Err(ModelError::DecodeValidation(format!(
                    "Task parameter '{param_name}' range has no elements after resolution"
                )));
            }
            out
        }
        template::StringRange::Expression(expr) => {
            // Typed evaluation — must yield a list. Propagate the actual error
            // if evaluation fails (e.g., division by zero, undefined variable).
            match expr.resolve_with(
                symtab,
                &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
            ) {
                Ok(val) if val.is_list() => {
                    let elements = val.list_elements().unwrap();
                    elements.iter().map(|e| e.to_display_string()).collect()
                }
                Ok(_) => {
                    return Err(ModelError::Expression(ExpressionError::new(
                        "String range expression must evaluate to a list",
                    )));
                }
                Err(e) => {
                    return Err(ModelError::Expression(ExpressionError::new(format!(
                        "String range expression: {e}"
                    ))));
                }
            }
        }
    };
    if resolved.len() > limits.max_task_param_range_len {
        return Err(ModelError::DecodeValidation(format!(
            "Task parameter '{}' range exceeds {} elements ({} elements)",
            param_name,
            limits.max_task_param_range_len,
            resolved.len()
        )));
    }
    for (i, s) in resolved.iter().enumerate() {
        if s.chars().count() > limits.max_task_param_string_len {
            return Err(ModelError::DecodeValidation(format!(
                "Task parameter '{}' range[{}]: resolved value exceeds {} characters ({} chars)",
                param_name,
                i,
                limits.max_task_param_string_len,
                s.len()
            )));
        }
        // §3.4.2 minimum length 1: applies to STRING and PATH elements
        // alike (an interpolated element is only known here; literal
        // empties are rejected at decode).
        if s.is_empty() {
            return Err(ModelError::DecodeValidation(format!(
                "Task parameter '{}' range[{}]: value must not resolve to an empty string",
                param_name, i
            )));
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::strip_redundant_leading_zeros as strip;

    /// §7.5 rule 1. openjd-model-for-python asserts the same cases against its
    /// regex form, `^([+-]?)0+(?=[0-9])`.
    #[test]
    fn strips_only_the_redundant_leading_zeros() {
        // Redundant: another digit follows.
        assert_eq!(strip("02.50"), "2.50");
        assert_eq!(strip("007"), "7");
        assert_eq!(strip("0007"), "7");
        assert_eq!(strip("01E+2"), "1E+2");
        assert_eq!(strip("-02.50"), "-2.50");
        assert_eq!(strip("+02.50"), "+2.50");

        // Not redundant: the zero is the integer part, so one digit stays.
        assert_eq!(strip("0.50"), "0.50");
        assert_eq!(strip("00.50"), "0.50");
        assert_eq!(strip("0"), "0");
        assert_eq!(strip("000"), "0");
        assert_eq!(strip("-0.0"), "-0.0");
        assert_eq!(strip("0e5"), "0e5");
        assert_eq!(strip("00e5"), "0e5");

        // Nothing to do.
        assert_eq!(strip("1.5"), "1.5");
        assert_eq!(strip("100"), "100");
        assert_eq!(strip("3.500"), "3.500");

        // §7.5 rule 2 is not this function's job: it never touches the fraction.
        assert_eq!(strip("2.50"), "2.50");
        assert_eq!(strip("0.0000001"), "0.0000001");
    }

    /// Not just an optimization: evidence that an unchanged element is returned
    /// untouched rather than rebuilt.
    #[test]
    fn borrows_when_there_is_nothing_to_strip() {
        assert!(matches!(strip("2.50"), Cow::Borrowed(_)));
        assert!(matches!(strip("0.50"), Cow::Borrowed(_)));
        assert!(matches!(strip("02.50"), Cow::Owned(_)));
    }
}
