// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Host requirements at job creation (Template Schemas §3.3).

use openjd_expr::symbol_table::SymbolTable;

use crate::error::{path_field, ModelError, PathElement, ValidationErrors};
use crate::job;
use crate::template;
use crate::template::validate_v2023_09::helpers::{check_capability_name, CapabilityKind};

use super::ranges;

/// Template Schemas §3.3.2: an attribute's `anyOf` or `allOf` holds at
/// most 50 values.
const MAX_ATTRIBUTE_VALUES: usize = 50;

pub(super) fn resolve_host_requirements(
    hr: &template::HostRequirements,
    symtab: &SymbolTable,
    ctx: &crate::types::ValidationContext,
    step_index: usize,
    budgets: super::EvalBudgets,
) -> Result<job::HostRequirements, ModelError> {
    let amounts = hr
        .amounts
        .as_ref()
        .map(|amts| {
            let standard = crate::capabilities::standard_amount_capability_names(
                ctx.profile.revision(),
                ctx.profile.extensions(),
            )?;
            let names = amts
                .iter()
                .enumerate()
                .map(|(amount_index, a)| {
                    resolve_capability_name(
                        &a.name,
                        CapabilityKind::Amount,
                        standard,
                        symtab,
                        budgets,
                        step_index,
                        amount_index,
                    )
                })
                .collect::<Result<Vec<_>, ModelError>>()?;
            check_resolved_names_unique(&names, CapabilityKind::Amount, step_index)?;
            amts.iter()
                .zip(names)
                .enumerate()
                .map(|(amount_index, (a, name))| {
                    let min = a
                        .min
                        .as_ref()
                        .map(|fs| {
                            ranges::resolve_to_f64(
                                fs,
                                symtab,
                                "hostRequirements amount min",
                                budgets,
                            )
                        })
                        .transpose()?
                        .flatten();
                    let max = a
                        .max
                        .as_ref()
                        .map(|fs| {
                            ranges::resolve_to_f64(
                                fs,
                                symtab,
                                "hostRequirements amount max",
                                budgets,
                            )
                        })
                        .transpose()?
                        .flatten();
                    check_resolved_amount_bounds(min, max, step_index, amount_index)?;
                    Ok(job::AmountRequirement { name, min, max })
                })
                .collect::<Result<Vec<_>, ModelError>>()
        })
        .transpose()?;

    let attributes = hr
        .attributes
        .as_ref()
        .map(|attrs| {
            let standard = crate::capabilities::standard_attribute_capabilities(
                ctx.profile.revision(),
                ctx.profile.extensions(),
            )?;
            let standard_names: Vec<&str> = standard.iter().map(|(name, _)| *name).collect();
            let names = attrs
                .iter()
                .enumerate()
                .map(|(attr_index, a)| {
                    resolve_capability_name(
                        &a.name,
                        CapabilityKind::Attribute,
                        &standard_names,
                        symtab,
                        budgets,
                        step_index,
                        attr_index,
                    )
                })
                .collect::<Result<Vec<_>, ModelError>>()?;
            check_resolved_names_unique(&names, CapabilityKind::Attribute, step_index)?;
            attrs
                .iter()
                .zip(names)
                .enumerate()
                .map(|(attr_index, (a, name))| {
                    // §3.3.2 allows at most 50 values. Resolution stops as
                    // soon as a list would go past that, so a template cannot
                    // make job creation build a long list just to reject it.
                    let resolve = |field: &str,
                                   vals: &Option<Vec<openjd_expr::FormatString>>|
                     -> Result<Option<Vec<String>>, ModelError> {
                        let Some(vals) = vals else { return Ok(None) };
                        ranges::resolve_string_list(vals, symtab, budgets, MAX_ATTRIBUTE_VALUES)?
                            .map(Some)
                            .ok_or_else(|| {
                                ModelError::DecodeValidation(format!(
                                    "steps[{step_index}] -> hostRequirements -> attributes[{attr_index}] -> {field}: exceeds {MAX_ATTRIBUTE_VALUES} elements after resolution"
                                ))
                            })
                    };
                    let any_of = resolve("anyOf", &a.any_of)?;
                    let all_of = resolve("allOf", &a.all_of)?;
                    let attr_lower = name.to_lowercase();
                    let is_single_valued = attr_lower == "attr.worker.os.family"
                        || attr_lower == "attr.worker.cpu.arch";
                    for (field, values) in [("anyOf", &any_of), ("allOf", &all_of)] {
                        if let Some(values) = values {
                            // Decode checks these counts on the template's
                            // element list, but a whole-field expression
                            // flattens a list inline (Expression Language
                            // §1.3.2), so the resolved count is unrelated
                            // to the count decode saw. Re-apply the rules
                            // on the resolved list (the 50-value maximum
                            // is applied while resolving, above).
                            if values.is_empty() {
                                return Err(ModelError::DecodeValidation(format!(
                                    "steps[{step_index}] -> hostRequirements -> attributes[{attr_index}] -> {field}: has no elements after resolution"
                                )));
                            }
                            if is_single_valued && field == "allOf" && values.len() > 1 {
                                return Err(ModelError::DecodeValidation(format!(
                                    "steps[{step_index}] -> hostRequirements -> attributes[{attr_index}] -> {field}: single-valued attribute cannot have more than 1 element after resolution"
                                )));
                            }
                            check_resolved_attribute_values(
                                &name, field, values, standard, step_index, attr_index,
                            )?;
                        }
                    }
                    Ok(job::AttributeRequirement {
                        name,
                        any_of,
                        all_of,
                    })
                })
                .collect::<Result<Vec<_>, ModelError>>()
        })
        .transpose()?;

    Ok(job::HostRequirements {
        amounts,
        attributes,
    })
}

/// Resolve a `hostRequirements` capability name and check the §3.3.1.1 /
/// §3.3.2.1 constraints on the resolved value.
///
/// `name` is `@fmtstring`, and template validation can only check a name
/// whose value it knows: a literal, or one that is fully static. Job
/// creation resolves every name, so the checks run here on the resolved
/// value, through the same [`check_capability_name`] template validation
/// uses, so a violation reads the same way whichever stage caught it.
fn resolve_capability_name(
    name: &openjd_expr::FormatString,
    kind: CapabilityKind,
    standard: &[&str],
    symtab: &SymbolTable,
    budgets: super::EvalBudgets,
    step_index: usize,
    index: usize,
) -> Result<String, ModelError> {
    // Required string field: a single whole-field expression resolves with
    // target type `string` (Expression Language §1.3.2).
    let resolved = name
        .resolve_with(
            symtab,
            &budgets
                .fs_options()
                .with_target_type(&openjd_expr::ExprType::STRING),
        )
        .map(|v| match v {
            openjd_expr::ExprValue::String(s) => s,
            other => other.to_display_string(),
        })
        .map_err(|e| ModelError::FormatStringError {
            message: format!("hostRequirements {} name: {e}", kind.noun()),
            input: Some(name.raw().to_string()),
            start: None,
            end: None,
            source: Some(e),
        })?;
    let path = vec![
        PathElement::Field("steps".to_string()),
        PathElement::Index(step_index),
        PathElement::Field("hostRequirements".to_string()),
        PathElement::Field(kind.field().to_string()),
        PathElement::Index(index),
    ];
    let mut errors = ValidationErrors::default();
    check_capability_name(&resolved, kind, standard, &path, &mut errors);
    errors.into_result("JobTemplate")?;
    Ok(resolved)
}

/// Check the §3.3 constraint that no two amounts, and no two attributes,
/// have the same name after the name format strings have been resolved.
///
/// Decode compares literal names only, since a name containing expressions
/// is unknown there. Two different templates names can resolve to the same
/// capability, so the resolved names are compared here, case-insensitively
/// as §3.3.1.1 / §3.3.2.1 require.
fn check_resolved_names_unique(
    names: &[String],
    kind: CapabilityKind,
    step_index: usize,
) -> Result<(), ModelError> {
    let mut seen = std::collections::HashSet::new();
    let mut errors = ValidationErrors::default();
    for (index, name) in names.iter().enumerate() {
        if !seen.insert(name.to_lowercase()) {
            errors.add(
                &[
                    PathElement::Field("steps".to_string()),
                    PathElement::Index(step_index),
                    PathElement::Field("hostRequirements".to_string()),
                    PathElement::Field(kind.field().to_string()),
                    PathElement::Index(index),
                ],
                format!("duplicate {} name '{name}'.", kind.noun()),
            );
        }
    }
    errors.into_result("JobTemplate")
}

/// Re-check the `amounts[].min` / `amounts[].max` bounds (§3.3.1) on resolved
/// values.
///
/// Under FEATURE_BUNDLE_1 both fields may be format strings, so the bounds
/// cannot be applied at decode — `validate_v2023_09::structure`'s
/// `parse_literal_amount` returns `None` for a non-literal for exactly that
/// reason, which skips all three checks. Job creation resolves the values, so
/// the deferred checks resume here.
///
/// `min` is `<nonnegativefloat>` and `max` is `<positivefloat>`, so `0` is
/// legal for one and not the other. Paths and wording match the decode-time
/// checks, so a violation reads the same way whichever path caught it.
fn check_resolved_amount_bounds(
    min: Option<f64>,
    max: Option<f64>,
    step_index: usize,
    amount_index: usize,
) -> Result<(), ModelError> {
    let amount_path = vec![
        PathElement::Field("steps".to_string()),
        PathElement::Index(step_index),
        PathElement::Field("hostRequirements".to_string()),
        PathElement::Field("amounts".to_string()),
        PathElement::Index(amount_index),
    ];
    let mut errors = ValidationErrors::default();
    // Decode enforces "at least one of min or max" on field *presence*,
    // but a whole-field expression resolving to null under the `float?`
    // target means "bound unset" — so a present field can still resolve
    // away. Re-apply the rule on the resolved values; without it the job
    // would carry a boundless amount requirement that matches every
    // worker. The "after resolution" suffix distinguishes this from the
    // decode-time message: the author *did* provide the field.
    if min.is_none() && max.is_none() {
        errors.add(
            &amount_path,
            "must have at least one of min or max after resolution.",
        );
    }
    if let Some(min) = min {
        if min < 0.0 {
            errors.add(&path_field(&amount_path, "min"), "must be non-negative.");
        }
    }
    if let Some(max) = max {
        if max <= 0.0 {
            errors.add(&path_field(&amount_path, "max"), "must be positive.");
        }
    }
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            errors.add(&amount_path, format!("min ({min}) > max ({max})."));
        }
    }
    errors.into_result("JobTemplate")
}

/// Re-check `<AttributeCapabilityValue>` constraints (§3.3.2.2) on resolved
/// `attributes[].anyOf` / `attributes[].allOf` values.
///
/// Both fields are `@fmtstring` in base 2023-09, so a value written as a
/// format string is unknown at decode and its constraints cannot be applied
/// there — `validate_v2023_09::structure` gates the decode-time check on
/// `FormatString::is_literal` for exactly that reason. Job creation resolves
/// the value, so the deferred check resumes here.
///
/// Errors are reported at the path and with the wording the decode-time check
/// uses for the same violation, so a violation reads the same way whichever
/// path caught it. Two differences are deliberate: the path always carries the
/// failing element's index, which decode omits for standard capabilities, and
/// the length message names the resolved value.
///
/// Only the first failing `anyOf`/`allOf` group on the first failing attribute
/// is reported, because the caller collects through `?`. Decode accumulates
/// every violation instead. Closing that gap means threading one
/// `ValidationErrors` through the whole of `resolve_host_requirements`.
fn check_resolved_attribute_values(
    capability_name: &str,
    field: &str,
    values: &[String],
    standard: &[(&str, &[&str])],
    step_index: usize,
    attr_index: usize,
) -> Result<(), ModelError> {
    let mut errors = ValidationErrors::default();
    for (value_index, value) in values.iter().enumerate() {
        if let Err(message) = crate::capabilities::validate_attribute_capability_value(
            capability_name,
            value,
            standard,
        ) {
            errors.add(
                &[
                    PathElement::Field("steps".to_string()),
                    PathElement::Index(step_index),
                    PathElement::Field("hostRequirements".to_string()),
                    PathElement::Field("attributes".to_string()),
                    PathElement::Index(attr_index),
                    PathElement::Field(field.to_string()),
                    PathElement::Index(value_index),
                ],
                message,
            );
        }
    }
    errors.into_result("JobTemplate")
}
