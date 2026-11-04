// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Task parameter space and range resolution.

use std::borrow::Cow;

use indexmap::IndexMap;

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
    budgets: super::EvalBudgets,
) -> Result<Option<f64>, ModelError> {
    let target = openjd_expr::ExprType::union(vec![
        openjd_expr::ExprType::FLOAT,
        openjd_expr::ExprType::NULLTYPE,
    ]);
    let resolved = fs
        .resolve_with(symtab, &budgets.fs_options().with_target_type(&target))
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

/// Why a list item could not be resolved.
enum ListItemError {
    /// Evaluation or coercion failed.
    Expression(ExpressionError),
    /// The item flattened into `len` values, more than the room left in its
    /// list. Detected before the values are copied out of the list.
    TooMany { len: usize },
}

impl From<ExpressionError> for ListItemError {
    fn from(e: ExpressionError) -> Self {
        Self::Expression(e)
    }
}

/// Resolve one list item to the values it contributes, at most `room` of
/// them.
///
/// With a `target` (a `T? | list[T]` type from
/// `TaskParameterType::range_list_item_target`), the item follows Expression
/// Language §1.3.2: none for `null`, one per member for a list, and the
/// value itself otherwise. The target applies to a whole-field expression
/// only, a format string that is exactly one `{{...}}`. Any other format
/// string (literal text around its expressions, two or more expressions, or
/// no expressions at all) resolves to a string. With no target the item
/// always resolves to one string. The caller interprets a string for its
/// element type. Task parameter range lists of every type and
/// `hostRequirements` attribute values share this, so they cannot drift
/// apart in how they skip and flatten.
///
/// A list longer than `room` fails with [`ListItemError::TooMany`] before
/// its members are copied, so a range list's element limit bounds the work
/// for each element, not only the final count.
fn resolve_list_item(
    fs: &openjd_expr::FormatString,
    target: Option<&openjd_expr::ExprType>,
    symtab: &SymbolTable,
    budgets: super::EvalBudgets,
    room: usize,
) -> Result<Vec<ExprValue>, ListItemError> {
    #[cfg(test)]
    tests::LAST_ROOM.with(|last| last.set(Some(room)));
    let Some(target) = target else {
        let text = fs.resolve_string_with(symtab, &budgets.fs_options())?;
        return Ok(vec![ExprValue::String(text)]);
    };
    let value = fs.resolve_with(symtab, &budgets.fs_options().with_target_type(target))?;
    Ok(match value {
        ExprValue::Null => Vec::new(),
        val if val.is_list() => {
            let len = val.list_len().unwrap_or_default();
            if len > room {
                return Err(ListItemError::TooMany { len });
            }
            val.list_elements().unwrap_or_default()
        }
        val => vec![val],
    })
}

/// The error for a range with more than the element limit. For a range
/// list, `count` is the elements resolved up to and including the element
/// that went over; for a whole-field range expression, its length.
fn range_too_long(param_name: &str, limits: &EffectiveLimits, count: usize) -> ModelError {
    ModelError::DecodeValidation(format!(
        "Task parameter '{}' range exceeds {} elements ({} elements)",
        param_name, limits.max_task_param_range_len, count
    ))
}

/// The room left in a range list that already holds `len` elements.
fn room_left(limits: &EffectiveLimits, len: usize) -> usize {
    limits.max_task_param_range_len.saturating_sub(len)
}

/// The text of a resolved string list item value: a string as is, any other
/// value in its display form.
fn list_item_text(value: ExprValue) -> String {
    match value {
        ExprValue::String(s) => s,
        other => other.to_display_string(),
    }
}

/// Resolve a list of FormatStrings to strings, at most `max_len` of them.
///
/// Each element is a list item, so per Expression Language §1.3.2 a
/// whole-field expression resolves with target type
/// `string? | list[string]`: a `null` result skips the element, a list
/// result flattens inline, and a string is a single element. Callers
/// enforcing a non-empty list must re-check after resolution, since
/// null-skips can empty it.
///
/// Returns `Ok(None)` as soon as the resolved list would hold more than
/// `max_len` strings, before a flattened element's members are copied, so
/// the caller can report its own limit without the work of building an
/// over-long list.
pub(super) fn resolve_string_list(
    vals: &[openjd_expr::FormatString],
    symtab: &SymbolTable,
    budgets: super::EvalBudgets,
    max_len: usize,
) -> Result<Option<Vec<String>>, ModelError> {
    let target = crate::types::string_list_item_target();
    let mut out = Vec::new();
    for fs in vals {
        let room = max_len.saturating_sub(out.len());
        let values = match resolve_list_item(fs, Some(&target), symtab, budgets, room) {
            Ok(values) => values,
            Err(ListItemError::Expression(e)) => {
                return Err(ModelError::FormatStringError {
                    message: e.to_string(),
                    input: Some(fs.raw().to_string()),
                    start: None,
                    end: None,
                })
            }
            Err(ListItemError::TooMany { .. }) => return Ok(None),
        };
        out.extend(values.into_iter().map(list_item_text));
        if out.len() > max_len {
            return Ok(None);
        }
    }
    Ok(Some(out))
}

/// Resolve a StepParameterSpaceDefinition into a StepParameterSpace with concrete ranges.
///
/// `expr` says whether the template enables the EXPR extension (not whether
/// the `create_job` context does), which decides how a numeric range list
/// element resolves (see `TaskParameterType::range_list_item_target`).
pub(super) fn resolve_parameter_space(
    ps: &template::StepParameterSpaceDefinition,
    symtab: &SymbolTable,
    expr: bool,
    limits: &EffectiveLimits,
    budgets: super::EvalBudgets,
) -> Result<job::StepParameterSpace, ModelError> {
    let mut defs = IndexMap::new();
    for tp in &ps.task_parameter_definitions {
        let name = tp.name().to_string();
        let param = resolve_task_parameter(tp, symtab, expr, limits, budgets)?;
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
    expr: bool,
    limits: &EffectiveLimits,
    budgets: super::EvalBudgets,
) -> Result<job::TaskParameter, ModelError> {
    // The list-item target for this parameter's type (Expression Language
    // §1.3.2), shared with template validation.
    let item_target = tp.task_param_type().range_list_item_target(expr);
    let item_target = item_target.as_ref();
    match tp {
        template::TaskParameterDefinition::INT(p) => {
            let range = resolve_int_range(
                &p.range,
                item_target,
                symtab,
                p.name.as_str(),
                limits,
                budgets,
            )?;
            Ok(job::TaskParameter::Int {
                range,
                chunks: None,
            })
        }
        template::TaskParameterDefinition::FLOAT(p) => {
            let range = resolve_float_range(
                &p.range,
                item_target,
                symtab,
                p.name.as_str(),
                limits,
                budgets,
            )?;
            Ok(job::TaskParameter::Float { range })
        }
        template::TaskParameterDefinition::STRING(p) => {
            let range = resolve_string_range(
                &p.range,
                item_target,
                symtab,
                p.name.as_str(),
                false,
                limits,
                budgets,
            )?;
            Ok(job::TaskParameter::String { range })
        }
        template::TaskParameterDefinition::PATH(p) => {
            let range = resolve_string_range(
                &p.range,
                item_target,
                symtab,
                p.name.as_str(),
                true,
                limits,
                budgets,
            )?;
            Ok(job::TaskParameter::Path { range })
        }
        template::TaskParameterDefinition::CHUNK_INT(p) => {
            let range = resolve_int_range(
                &p.range,
                item_target,
                symtab,
                p.name.as_str(),
                limits,
                budgets,
            )?;
            // CHUNK[INT] chunking bounds values to |v| < 2^62, below what
            // list and range-expression ranges accept, so reject
            // out-of-bound values here — at job creation, with a
            // path-annotated error — rather than overflowing when a chunk
            // is built during iteration.
            crate::job::step_param_space::check_chunk_int_value_bound(p.name.as_str(), &range)?;
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
                        &budgets
                            .fs_options()
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
                                &budgets.fs_options().with_target_type(&target),
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
    item_target: Option<&openjd_expr::ExprType>,
    symtab: &SymbolTable,
    param_name: &str,
    limits: &EffectiveLimits,
    budgets: super::EvalBudgets,
) -> Result<job::TaskParamRange<i64>, ModelError> {
    match range {
        template::IntRange::List(items) => {
            let mut ints = Vec::with_capacity(items.len());
            for (k, item) in items.iter().enumerate() {
                match item {
                    template::IntOrFormatString::Int(i) => ints.push(*i),
                    template::IntOrFormatString::FormatString(fs) => {
                        let values = resolve_int_range_list_element(
                            fs,
                            k,
                            item_target,
                            symtab,
                            param_name,
                            budgets,
                            room_left(limits, ints.len()),
                        )
                        .map_err(|e| match e {
                            IntElementError::Model(e) => e,
                            IntElementError::TooMany { len } => {
                                range_too_long(param_name, limits, ints.len() + len)
                            }
                        })?;
                        ints.extend(values);
                    }
                }
                if ints.len() > limits.max_task_param_range_len {
                    return Err(range_too_long(param_name, limits, ints.len()));
                }
            }
            check_range_not_emptied(ints.len(), param_name)?;
            Ok(job::TaskParamRange::List(ints))
        }
        template::IntRange::Expression(expr) => {
            // Try typed evaluation first — may directly yield a RangeExpr or list[int].
            // For multi-segment format strings (e.g., "1-{{Param.Count}}"), typed
            // evaluation fails and we fall through to string resolution, which
            // concatenates segments and parses the result as a range expression.
            // Any real evaluation errors (division by zero, type errors) will be
            // caught by the string resolution fallback path.
            if let Ok(val) = expr.resolve_with(symtab, &budgets.fs_options()) {
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
                .resolve_string_with(symtab, &budgets.fs_options())
                .map_err(ModelError::Expression)?;
            let range_expr: RangeExpr = resolved
                .parse()
                .map_err(|e: openjd_expr::ExpressionError| ModelError::Expression(e))?;
            Ok(job::TaskParamRange::RangeExpr(range_expr))
        }
    }
}

/// Resolve element `k` of an `<IntRangeList>`, a `<TaskParameterStringValue>`
/// (Template Schemas §3.4.1.1), to the integers it contributes.
///
/// With the EXPR extension it is a list item (Expression Language §1.3.2),
/// resolved with `target` (`int? | list[int]`): `null` contributes nothing
/// and a list one integer per member. A string result (always, without
/// EXPR) parses like an `<intstring>` with surrounding whitespace tolerated.
/// Errors name the parameter and `range[k]`.
fn resolve_int_range_list_element(
    fs: &openjd_expr::FormatString,
    k: usize,
    target: Option<&openjd_expr::ExprType>,
    symtab: &SymbolTable,
    param_name: &str,
    budgets: super::EvalBudgets,
    room: usize,
) -> Result<Vec<i64>, IntElementError> {
    let error = |detail: std::fmt::Arguments<'_>| {
        IntElementError::Model(ModelError::Expression(ExpressionError::new(format!(
            "Task parameter '{param_name}' range[{k}]: {detail}"
        ))))
    };
    let values = resolve_list_item(fs, target, symtab, budgets, room).map_err(|e| match e {
        ListItemError::Expression(e) => error(format_args!("{e}")),
        ListItemError::TooMany { len } => IntElementError::TooMany { len },
    })?;
    values
        .into_iter()
        .map(|value| match value {
            ExprValue::Int(i) => Ok(i),
            other => {
                let text = list_item_text(other);
                text.trim().parse::<i64>().map_err(|e| {
                    let problem = match e.kind() {
                        std::num::IntErrorKind::PosOverflow
                        | std::num::IntErrorKind::NegOverflow => {
                            "is out of range for a 64-bit integer"
                        }
                        _ => "is not an integer",
                    };
                    error(format_args!("value '{text}' {problem}"))
                })
            }
        })
        .collect()
}

/// Why an `<IntRangeList>` element could not be resolved.
enum IntElementError {
    /// A located error, ready to report.
    Model(ModelError),
    /// The element flattened past the room left in the list; the caller
    /// reports it with the list's count.
    TooMany { len: usize },
}

/// The ≥1-element rule (Template Schemas §3.4.1) is checked on the list as
/// written, at decode. A format string element that resolves to `null`
/// contributes nothing, so a list can still end up empty here.
fn check_range_not_emptied(len: usize, param_name: &str) -> Result<(), ModelError> {
    if len == 0 {
        return Err(ModelError::DecodeValidation(format!(
            "Task parameter '{param_name}' range has no elements after resolution"
        )));
    }
    Ok(())
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

/// One value a FLOAT range list's format string element contributed, or why
/// it is not a valid one.
///
/// A float that carries a spelling different from what `format_float` gives,
/// alone or as a member of a list, and an integer render as written: the
/// text is what a `<floatstring>` would carry (§7.5). Any other float, such
/// as one the expression computed or an integer member of a list converted
/// to `float`, has no spelling stored and renders through `format_float`;
/// for a spelling equal to that rendering, storing it would change only the
/// serialized job. A string (from a format string that is not a single
/// whole-field expression, or from any element without EXPR) is a
/// `<floatstring>`, and its spelling is stored unless it is longer than
/// `max_task_param_string_len`.
fn float_range_value(value: ExprValue, limits: &EffectiveLimits) -> Result<Float64, String> {
    let resolved = match value {
        ExprValue::Float(f)
            if f.to_display_string() == openjd_expr::value::format_float(f.value()) =>
        {
            return Float64::new(f.value()).map_err(|e| e.to_string());
        }
        other => list_item_text(other),
    };
    let trimmed = resolved.trim();
    let value = trimmed
        .parse::<f64>()
        .map_err(|_| format!("value '{resolved}' is not a number"))?;
    if !value.is_finite() {
        return Err(format!("value '{resolved}' is not finite"));
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
        return Float64::new(value).map_err(|e| e.to_string());
    }
    Float64::with_str(value, text.into_owned()).map_err(|e| e.to_string())
}

fn resolve_float_range(
    range: &template::FloatRange,
    item_target: Option<&openjd_expr::ExprType>,
    symtab: &SymbolTable,
    param_name: &str,
    limits: &EffectiveLimits,
    budgets: super::EvalBudgets,
) -> Result<Vec<Float64>, ModelError> {
    let floats: Vec<Float64> = match range {
        template::FloatRange::List(items) => {
            let mut floats = Vec::with_capacity(items.len());
            for (k, item) in items.iter().enumerate() {
                match item {
                    // `2.50` and `2.5` are the same literal after parsing, so a
                    // `<float>` makes no request about how it renders.
                    template::FloatRangeItem::Float(f) => floats.push(float64(*f, param_name)?),
                    // With EXPR a list item (Expression Language §1.3.2):
                    // `null` contributes nothing and a list one value per
                    // member. Errors name the parameter and the element.
                    template::FloatRangeItem::FormatString(fs) => {
                        let error = |detail: std::fmt::Arguments<'_>| {
                            ModelError::Expression(ExpressionError::new(format!(
                                "Task parameter '{param_name}' range[{k}]: {detail}"
                            )))
                        };
                        let room = room_left(limits, floats.len());
                        let values = match resolve_list_item(fs, item_target, symtab, budgets, room)
                        {
                            Ok(values) => values,
                            Err(ListItemError::Expression(e)) => {
                                return Err(error(format_args!("{e}")))
                            }
                            Err(ListItemError::TooMany { len }) => {
                                return Err(range_too_long(param_name, limits, floats.len() + len))
                            }
                        };
                        for value in values {
                            floats.push(
                                float_range_value(value, limits)
                                    .map_err(|detail| error(format_args!("{detail}")))?,
                            );
                        }
                    }
                }
                if floats.len() > limits.max_task_param_range_len {
                    return Err(range_too_long(param_name, limits, floats.len()));
                }
            }
            check_range_not_emptied(floats.len(), param_name)?;
            floats
        }
        template::FloatRange::Expression(expr) => {
            // Typed evaluation — must yield a list. Propagate the actual error
            // if evaluation fails.
            match expr.resolve_with(symtab, &budgets.fs_options()) {
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
        return Err(range_too_long(param_name, limits, floats.len()));
    }
    Ok(floats)
}

fn resolve_string_range(
    range: &template::StringRange,
    item_target: Option<&openjd_expr::ExprType>,
    symtab: &SymbolTable,
    param_name: &str,
    is_path: bool,
    limits: &EffectiveLimits,
    budgets: super::EvalBudgets,
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
            let mut out = Vec::new();
            for fs in items {
                let room = room_left(limits, out.len());
                let values = match resolve_list_item(fs, item_target, symtab, budgets, room) {
                    Ok(values) => values,
                    Err(ListItemError::Expression(e)) => return Err(ModelError::Expression(e)),
                    Err(ListItemError::TooMany { len }) => {
                        return Err(range_too_long(param_name, limits, out.len() + len))
                    }
                };
                out.extend(values.into_iter().map(list_item_text));
                if out.len() > limits.max_task_param_range_len {
                    return Err(range_too_long(param_name, limits, out.len()));
                }
            }
            check_range_not_emptied(out.len(), param_name)?;
            out
        }
        template::StringRange::Expression(expr) => {
            // Typed evaluation — must yield a list. Propagate the actual error
            // if evaluation fails (e.g., division by zero, undefined variable).
            match expr.resolve_with(symtab, &budgets.fs_options()) {
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
        return Err(range_too_long(param_name, limits, resolved.len()));
    }
    for (i, s) in resolved.iter().enumerate() {
        let char_count = s.chars().count();
        if char_count > limits.max_task_param_string_len {
            return Err(ModelError::DecodeValidation(format!(
                "Task parameter '{}' range[{}]: resolved value exceeds {} characters ({} chars)",
                param_name, i, limits.max_task_param_string_len, char_count
            )));
        }
        // §3.4.2 sets a minimum length of 1 on <TaskParameterStringValue>,
        // but the reference implementation enforces it only for PATH ("an
        // empty string is not a valid path on any OS") and accepts empty
        // STRING elements end to end. Match that for compatibility; an
        // interpolated PATH element is only known here.
        if is_path && s.is_empty() {
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

    thread_local! {
        /// The `room` the most recent `resolve_list_item` call on this
        /// thread received, so a test can check what each caller passes.
        /// The early stop's only observable effect is the memory it saves.
        pub(super) static LAST_ROOM: std::cell::Cell<Option<usize>> =
            const { std::cell::Cell::new(None) };
    }

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

    fn no_budgets() -> crate::job::create_job::EvalBudgets {
        crate::job::create_job::EvalBudgets {
            memory: None,
            operations: None,
        }
    }

    /// A list item longer than the room left fails with its length, before
    /// its members are copied out (Template Schemas §3.4 element limit). The
    /// callers' per-element checks give the same message, so only the error
    /// variant shows the early stop.
    #[test]
    fn list_item_longer_than_room_stops_before_copying() {
        let symtab = openjd_expr::symbol_table::SymbolTable::new();
        let target = crate::types::TaskParameterType::Int
            .range_list_item_target(true)
            .unwrap();
        let fs = openjd_expr::FormatString::new("{{ range_expr('1-9000000') }}").unwrap();
        match super::resolve_list_item(&fs, Some(&target), &symtab, no_budgets(), 1024) {
            Err(super::ListItemError::TooMany { len }) => assert_eq!(len, 9_000_000),
            Err(super::ListItemError::Expression(e)) => panic!("unexpected error: {e}"),
            Ok(values) => panic!("expected TooMany, got {} values", values.len()),
        }
        // Exactly the room left fits.
        let fs = openjd_expr::FormatString::new("{{ range_expr('1-3') }}").unwrap();
        let values = super::resolve_list_item(&fs, Some(&target), &symtab, no_budgets(), 3)
            .unwrap_or_else(|_| panic!("three members fit in a room of three"));
        assert_eq!(values.len(), 3);
        // The INT element resolver passes the room on.
        match super::resolve_int_range_list_element(
            &fs,
            0,
            Some(&target),
            &symtab,
            "Frame",
            no_budgets(),
            2,
        ) {
            Err(super::IntElementError::TooMany { len }) => assert_eq!(len, 3),
            Err(super::IntElementError::Model(e)) => panic!("unexpected error: {e}"),
            Ok(values) => panic!("expected TooMany, got {values:?}"),
        }
    }

    /// Each range list type hands `resolve_list_item` the room left in the
    /// list: the element limit less the elements resolved so far.
    #[test]
    fn range_lists_pass_the_room_left() {
        use crate::template::{
            FloatRange, FloatRangeItem, IntOrFormatString, IntRange, StringRange,
        };
        use crate::types::TaskParameterType;

        let symtab = openjd_expr::symbol_table::SymbolTable::new();
        let ctx = crate::types::ValidationContext::from_profile(crate::ModelProfile::latest());
        let limits = super::EffectiveLimits::from_context(&ctx);
        let fs = |s: &str| openjd_expr::FormatString::new(s).unwrap();
        let expected = Some(limits.max_task_param_range_len - 3);

        let int = IntRange::List(vec![
            IntOrFormatString::Int(1),
            IntOrFormatString::FormatString(fs("{{ [2, 3] }}")),
            IntOrFormatString::FormatString(fs("{{ [4] }}")),
        ]);
        let target = TaskParameterType::Int.range_list_item_target(true);
        LAST_ROOM.with(|last| last.set(None));
        super::resolve_int_range(&int, target.as_ref(), &symtab, "X", &limits, no_budgets())
            .unwrap();
        assert_eq!(LAST_ROOM.with(|last| last.get()), expected, "INT");

        let float = FloatRange::List(vec![
            FloatRangeItem::Float(1.5),
            FloatRangeItem::FormatString(fs("{{ [2.5, 3.5] }}")),
            FloatRangeItem::FormatString(fs("{{ [4.5] }}")),
        ]);
        let target = TaskParameterType::Float.range_list_item_target(true);
        LAST_ROOM.with(|last| last.set(None));
        super::resolve_float_range(&float, target.as_ref(), &symtab, "X", &limits, no_budgets())
            .unwrap();
        assert_eq!(LAST_ROOM.with(|last| last.get()), expected, "FLOAT");

        let string = StringRange::List(vec![fs("a"), fs("{{ ['b', 'c'] }}"), fs("{{ ['d'] }}")]);
        let target = TaskParameterType::String.range_list_item_target(true);
        LAST_ROOM.with(|last| last.set(None));
        super::resolve_string_range(
            &string,
            target.as_ref(),
            &symtab,
            "X",
            false,
            &limits,
            no_budgets(),
        )
        .unwrap();
        assert_eq!(LAST_ROOM.with(|last| last.get()), expected, "STRING");
    }

    /// `resolve_string_list` reports a list over its maximum as `None`,
    /// without copying the element that goes over.
    #[test]
    fn string_list_stops_at_its_maximum() {
        let symtab = openjd_expr::symbol_table::SymbolTable::new();
        let fs = |s: &str| openjd_expr::FormatString::new(s).unwrap();
        let fits = [fs("{{ ['v'] * 49 }}"), fs("w")];
        assert_eq!(
            super::resolve_string_list(&fits, &symtab, no_budgets(), 50)
                .unwrap()
                .map(|v| v.len()),
            Some(50)
        );
        let over = [fs("{{ ['v'] * 50 }}"), fs("w")];
        assert_eq!(
            super::resolve_string_list(&over, &symtab, no_budgets(), 50).unwrap(),
            None
        );
    }
}
