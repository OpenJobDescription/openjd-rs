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

/// Resolve a FormatString to f64.
pub(super) fn resolve_to_f64(
    fs: &openjd_expr::FormatString,
    symtab: &SymbolTable,
    context: &str,
) -> Result<f64, ModelError> {
    let resolved = fs
        .resolve_string_with(
            symtab,
            &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
        )
        .map_err(|e| ModelError::FormatStringError {
            message: format!("{context}: {e}"),
            input: Some(fs.raw().to_string()),
            start: None,
            end: None,
        })?;
    let value = resolved.trim().parse::<f64>().map_err(|_| {
        ModelError::Expression(ExpressionError::new(format!(
            "{context}: '{resolved}' is not a valid number"
        )))
    })?;
    if !value.is_finite() {
        return Err(ModelError::Expression(ExpressionError::new(format!(
            "{context}: '{resolved}' is not a finite number"
        ))));
    }
    Ok(value)
}

/// Resolve a list of FormatStrings to strings.
pub(super) fn resolve_string_list(
    vals: &[openjd_expr::FormatString],
    symtab: &SymbolTable,
) -> Result<Vec<String>, ModelError> {
    vals.iter()
        .map(|fs| {
            fs.resolve_string_with(
                symtab,
                &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
            )
            .map_err(|e| ModelError::FormatStringError {
                message: e.to_string(),
                input: Some(fs.raw().to_string()),
                start: None,
                end: None,
            })
        })
        .collect()
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
            let range = resolve_string_range(&p.range, symtab, p.name.as_str(), false, limits)?;
            Ok(job::TaskParameter::String { range })
        }
        template::TaskParameterDefinition::PATH(p) => {
            let range = resolve_string_range(&p.range, symtab, p.name.as_str(), true, limits)?;
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
            let default_task_count = match &p.chunks.default_task_count {
                template::IntOrFormatString::Int(n) => (*n).max(1) as usize,
                template::IntOrFormatString::FormatString(fs) => {
                    let resolved = fs
                        .resolve_string_with(
                            symtab,
                            &openjd_expr::FormatStringOptions::new()
                                .with_path_format(PathFormat::Posix),
                        )
                        .map_err(|e| {
                            ModelError::Expression(ExpressionError::new(format!(
                                "chunks.defaultTaskCount: {e}"
                            )))
                        })?;
                    let count = resolved.trim().parse::<i64>().map_err(|_| {
                        ModelError::Expression(ExpressionError::new(format!(
                            "chunks.defaultTaskCount: '{resolved}' is not a valid integer"
                        )))
                    })?;
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
            let target_runtime_seconds = p.chunks.target_runtime_seconds.as_ref()
                .map(|v| match v {
                    template::IntOrFormatString::Int(n) => Ok((*n).max(0) as usize),
                    template::IntOrFormatString::FormatString(fs) => {
                        let resolved = fs.resolve_string_with(symtab, &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix))
                            .map_err(|e| ModelError::Expression(ExpressionError::new(format!("chunks.targetRuntimeSeconds: {e}"))))?;
                        let seconds = resolved.trim().parse::<i64>()
                            .map_err(|_| ModelError::Expression(ExpressionError::new(format!("chunks.targetRuntimeSeconds: '{resolved}' is not a valid integer"))))?;
                        // §3.4.1.5 sets a minimum of 0; same deferral as
                        // defaultTaskCount above, and the same reason to reject
                        // rather than clamp.
                        if seconds < 0 {
                            return Err(ModelError::Expression(ExpressionError::new(format!("chunks.targetRuntimeSeconds: resolved to {seconds}, but must be >= 0"))));
                        }
                        Ok(seconds as usize)
                    }
                })
                .transpose()?;
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
                    if text.len() > limits.max_task_param_string_len {
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
    is_path: bool,
    limits: &EffectiveLimits,
) -> Result<Vec<String>, ModelError> {
    let resolved: Vec<String> = match range {
        template::StringRange::List(items) => items
            .iter()
            .map(|fs| {
                fs.resolve_string_with(
                    symtab,
                    &openjd_expr::FormatStringOptions::new().with_path_format(PathFormat::Posix),
                )
                .map_err(ModelError::Expression)
            })
            .collect::<Result<Vec<_>, _>>()?,
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
        if s.len() > limits.max_task_param_string_len {
            return Err(ModelError::DecodeValidation(format!(
                "Task parameter '{}' range[{}]: resolved value exceeds {} characters ({} chars)",
                param_name,
                i,
                limits.max_task_param_string_len,
                s.len()
            )));
        }
        if is_path && s.is_empty() {
            return Err(ModelError::DecodeValidation(format!(
                "Task parameter '{}' range[{}]: PATH value must not be empty",
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
