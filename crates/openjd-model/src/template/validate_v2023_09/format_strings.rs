// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Pass 8: Format string validation.
//!
//! Validates all format string references resolve to defined variables by
//! building scope-appropriate symbol tables and evaluating expressions against
//! them. This works for both base spec (simple `{{Param.X}}` references) and
//! EXPR (full expressions) — the evaluator handles both.

use std::collections::HashSet;

use openjd_expr::eval::ParsedExpression;
use openjd_expr::function_library::FunctionLibrary;
use openjd_expr::path_mapping::PathFormat;
use openjd_expr::symbol_table::SymbolTable;
use openjd_expr::types::ExprType;
use openjd_expr::value::ExprValue;
use openjd_expr::FormatString;

use crate::error::{path_field, path_index, PathElement, ValidationErrors};
use crate::template::*;
use crate::types::{fixed_type, JobParameterType, ModelExtension, ValidationContext};

use super::helpers;

/// Maximum length of a `let` binding's `<UserIdentifier>` (§3.6.1).
///
/// Flat, so not `EffectiveLimits::max_identifier_len`: that is the §7.1 cap, 64
/// without `FEATURE_BUNDLE_1`, and a 512-character name must be accepted with
/// EXPR alone.
const MAX_LET_IDENTIFIER_LEN: usize = 512;

/// Add `Param.<name>` and `RawParam.<name>` for each definition in `defs`,
/// as unresolved values of the types Template Schemas §7.3.1 gives them:
/// `Param` has the expression type of the parameter type, and `RawParam`
/// the same except `string` for PATH and `list[string]` for LIST[PATH].
/// Without `host`, the `Param` of a PATH or LIST[PATH] parameter is left
/// out, since mapping a path needs a host (the template scope). A
/// name that `defs` yields twice (an error another pass reports) is set by
/// each in turn.
fn add_params<'d>(
    symtab: &mut SymbolTable,
    defs: impl IntoIterator<Item = (&'d str, JobParameterType)>,
    host: bool,
) {
    for (name, pt) in defs {
        // A parameter name is an `<Identifier>`, so `set` cannot fail.
        if host || !matches!(pt, JobParameterType::Path | JobParameterType::ListPath) {
            let _ = symtab.set(
                &format!("Param.{name}"),
                ExprValue::unresolved(pt.expr_type()),
            );
        }
        let _ = symtab.set(
            &format!("RawParam.{name}"),
            ExprValue::unresolved(pt.raw_expr_type()),
        );
    }
}

/// The name and type of each Job Parameter definition, for [`add_params`].
fn job_params(
    params: Option<&[JobParameterDefinition]>,
) -> impl Iterator<Item = (&str, JobParameterType)> {
    params
        .into_iter()
        .flatten()
        .map(|p| (p.name(), p.job_param_type()))
}

/// Add `Session.*` (§7.3.1), which only a Session on a host binds.
fn add_session_symbols(symtab: &mut SymbolTable) {
    for (name, t) in [
        ("Session.WorkingDirectory", ExprType::PATH),
        ("Session.HasPathMappingRules", ExprType::BOOL),
        ("Session.PathMappingRulesFile", ExprType::PATH),
    ] {
        symtab.set(name, ExprValue::unresolved(t)).expect("symtab");
    }
}

/// Add `Job.Name`, a symbol of the job, Step, session and task scopes,
/// for a caller that has checked the `EXPR` extension. It is not in the
/// template scope, the scope of the Job Template's `name`.
fn add_job_symbols(symtab: &mut SymbolTable) {
    symtab
        .set("Job.Name", ExprValue::unresolved(ExprType::STRING))
        .expect("symtab");
}

/// Add `{prefix}File.<name>` (`Env.File.*` or `Task.File.*`) for each
/// embedded file of a script. A `filename` is a plain string, so a Session
/// allocates the files before it evaluates the script's `let` bindings,
/// and these are in their scope.
fn add_files(symtab: &mut SymbolTable, prefix: &str, files: Option<&[EmbeddedFile]>) {
    for f in files.into_iter().flatten() {
        symtab
            .set(
                &format!("{prefix}File.{}", f.name),
                ExprValue::unresolved(ExprType::PATH),
            )
            .expect("symtab");
    }
}

/// The names `bindings` bind: the text before each `=`, trimmed.
fn let_names(bindings: Option<&[String]>) -> HashSet<String> {
    bindings
        .into_iter()
        .flatten()
        .filter_map(|b| b.find('=').map(|eq| b[..eq].trim().to_string()))
        .collect()
}

/// Copy the values of `step`'s `let` bindings from `step_scope`, a scope
/// that holds them, into `symtab`. They are copied last: a binding named like
/// a scope root (`Session`, an error pass 8 reports) collides with it and is
/// dropped, and a dotted name (an error too) sets the symbol it names. With
/// `evaluated_only`, a binding with an empty name or no expression, which
/// pass 8 reports and does not evaluate, is not copied: the task scope
/// copies only evaluated bindings, and a Step Environment's scope copies
/// every binding with an `=`. `symtab` is a host scope, so a value whose
/// type contains `path` is then made unresolved
/// ([`unresolve_template_paths`]).
fn copy_step_lets(
    symtab: &mut SymbolTable,
    step: &StepTemplate,
    step_scope: &SymbolTable,
    evaluated_only: bool,
) {
    if step.let_bindings.is_none() {
        return;
    }
    for binding in step.let_bindings.iter().flatten() {
        let Some(eq) = binding.find('=') else {
            continue;
        };
        let name = binding[..eq].trim();
        if evaluated_only && (name.is_empty() || binding[eq + 1..].trim().is_empty()) {
            continue;
        }
        if let Some(value) = step_scope.get_value(name) {
            let _ = symtab.set(name, value.clone());
        }
    }
    // Host scope: their paths are in the worker's format there.
    unresolve_template_paths(symtab);
}

/// A step's template-scope symtab with its step-level `let` bindings
/// evaluated in, built by [`FsCtx::step_let_scope`] (or from [`RetainedBindings`])
/// and shared by the sites of one step when evaluating them reported
/// nothing.
struct StepLetScope {
    symtab: SymbolTable,
    names: HashSet<String>,
}

/// Most bytes [`RetainedBindings`] may hold across all steps at once. This
/// is the extra peak memory sharing with host requirements costs; real
/// templates keep a few kilobytes.
const RETAINED_SCOPE_BUDGET: usize = 4 * 1024 * 1024;

/// A step's clean `let` values, kept from the host-requirements loop for the
/// step's ranges and body. Only the bindings are kept, not the symtab, and
/// only values whose heap size is known exactly, so [`Self::bytes`] plus
/// [`Self::DEQUE_MIN_BYTES`] is at least the bytes their allocations request
/// (allocator rounding is not counted).
struct RetainedBindings {
    step: usize,
    bindings: Vec<(String, ExprValue)>,
    bytes: usize,
}

impl RetainedBindings {
    /// The deque's smallest nonzero buffer (four entries) less what the
    /// first entry's charge covers, charged before anything is kept.
    const DEQUE_MIN_BYTES: usize = 2 * std::mem::size_of::<Self>();

    /// The scope's bindings, or `None` when there are none (nothing to save)
    /// or a value's heap size is not known exactly.
    fn from_scope(step: usize, scope: StepLetScope) -> Option<Self> {
        if scope.names.is_empty() {
            return None;
        }
        // The deque holding entries may have twice the capacity it uses
        // (its smallest buffer is charged up front, see `DEQUE_MIN_BYTES`).
        let mut bytes = 2 * std::mem::size_of::<Self>()
            + scope.names.len() * std::mem::size_of::<(String, ExprValue)>();
        let mut bindings = Vec::with_capacity(scope.names.len());
        for name in scope.names {
            let value = scope.symtab.get_value(&name)?.clone();
            bytes += name.capacity() + exact_heap_size(&value)?;
            bindings.push((name, value));
        }
        Some(Self {
            step,
            bindings,
            bytes,
        })
    }

    /// Bytes charged: twice this entry's deque slot (for spare capacity),
    /// the bindings vector, and each name's and value's allocation.
    fn bytes(&self) -> usize {
        self.bytes
    }

    /// The step's scope again: the base symtab with the bindings set in.
    fn into_scope(self, base: SymbolTable) -> StepLetScope {
        let mut symtab = base;
        let mut names = HashSet::with_capacity(self.bindings.len());
        for (name, value) in self.bindings {
            let _ = symtab.set(&name, value);
            names.insert(name);
        }
        StepLetScope { symtab, names }
    }
}

/// Heap bytes a scalar value owns, or `None` for lists, ranges, unresolved
/// types with parameters, and any variant added later, whose allocation this
/// crate cannot measure.
fn exact_heap_size(value: &ExprValue) -> Option<usize> {
    match value {
        ExprValue::Null | ExprValue::Bool(_) | ExprValue::Int(_) => Some(0),
        // A parameterless type (`unresolved[int]`) owns no heap.
        ExprValue::Unresolved(t) if t.params().is_empty() => Some(0),
        // `memory_size` counts a float's preserved spelling exactly.
        ExprValue::Float(_) => Some(value.memory_size() - std::mem::size_of::<ExprValue>()),
        ExprValue::String(s) | ExprValue::Path { value: s, .. } => Some(s.capacity()),
        _ => None,
    }
}

/// What pass 8 uses throughout one document: the validation context, the
/// document's own Job Parameters, the extensions that change a scope, the
/// evaluators, the limits and the standard capabilities. Its methods build
/// the scopes of Template Schemas §7.3.1 (each value's Scope) and §7.4
/// (the stage that resolves each field).
struct FsCtx<'a> {
    ctx: &'a ValidationContext,
    params: Option<&'a [JobParameterDefinition]>,
    /// `EXPR`: `let`, `Job.*`, `Step.Name`, and expressions beyond a
    /// reference.
    expr: bool,
    /// Whether the Job Template itself lists `EXPR`, which decides the
    /// target type of a numeric range list element
    /// (`TaskParameterType::range_list_item_target`). `false` for an
    /// Environment Template, which has no Steps.
    template_expr: bool,
    /// Template scope: no host functions.
    template: FsEval,
    /// Session and task scopes: host functions type-check, unresolved.
    host: FsEval,
    limits: super::EffectiveLimits,
    standards: HostRequirementsStandards,
}

impl<'a> FsCtx<'a> {
    fn new(ctx: &'a ValidationContext, params: Option<&'a [JobParameterDefinition]>) -> Self {
        // Only an unsupported revision errs, and it cannot reach v2023-09
        // validation.
        let attributes = crate::capabilities::standard_attribute_capabilities(
            ctx.profile.revision(),
            ctx.profile.extensions(),
        )
        .unwrap_or(&[]);
        let amounts = crate::capabilities::standard_amount_capability_names(
            ctx.profile.revision(),
            ctx.profile.extensions(),
        )
        .unwrap_or(&[]);
        Self {
            ctx,
            params,
            expr: ctx.profile.has_extension(ModelExtension::Expr),
            template_expr: false,
            template: FsEval::new(ctx, openjd_expr::HostContext::None),
            host: FsEval::new(ctx, openjd_expr::HostContext::Unresolved),
            limits: super::EffectiveLimits::from_context(ctx),
            standards: HostRequirementsStandards {
                amounts,
                attribute_names: attributes.iter().map(|(name, _)| *name).collect(),
                attributes,
            },
        }
    }

    /// The template scope: `RawParam.*` and the non-PATH
    /// `Param.*`. The scope of the Job Template's `name`.
    fn template_scope(&self) -> SymbolTable {
        let mut symtab = SymbolTable::new();
        add_params(&mut symtab, job_params(self.params), false);
        symtab
    }

    /// The template scope and, with `EXPR`, `Job.Name`: the scope of a Job
    /// Environment's fields that are resolved when the Job is created, such
    /// as `timeout`.
    fn job_scope(&self) -> SymbolTable {
        let mut symtab = self.template_scope();
        if self.expr {
            add_job_symbols(&mut symtab);
        }
        symtab
    }

    /// A Step's template scope before its `let` bindings: the job scope
    /// and, with `EXPR`, `Step.Name`.
    fn step_base_scope(&self) -> SymbolTable {
        let mut symtab = self.job_scope();
        if self.expr {
            symtab
                .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                .expect("symtab");
        }
        symtab
    }

    /// A Step's template scope: [`step_base_scope`](Self::step_base_scope)
    /// with, under `EXPR`, the Step's `let` bindings evaluated in by
    /// `validate_let_bindings`, which reports into `errors`. The scope of
    /// the Step's `hostRequirements`, `parameterSpace` and `timeout`s.
    /// Without `EXPR` it evaluates nothing, and the Step body reports the
    /// `let`.
    fn step_let_scope(
        &self,
        step: &StepTemplate,
        step_path: &[PathElement],
        errors: &mut ValidationErrors,
    ) -> StepLetScope {
        let mut symtab = self.step_base_scope();
        let mut names = HashSet::new();
        if let (true, Some(bindings)) = (self.expr, &step.let_bindings) {
            validate_let_bindings(
                bindings,
                &path_field(step_path, "let"),
                &HashSet::new(),
                &mut symtab,
                &self.template,
                errors,
            );
            names = let_names(Some(bindings));
        }
        StepLetScope { symtab, names }
    }

    /// The `parameterSpace` range scope of a Step whose `let` bindings are
    /// not clean. A range sees a failed binding as unbound, and the failure
    /// is reported at the Step as `let binding '<name>': <error>`. For
    /// clean bindings the result equals [`StepLetScope::symtab`], so this
    /// runs only for the others, to keep their diagnostics. Bindings are
    /// only evaluated, and so only fail to be clean, with `EXPR`, so the
    /// scope always has `Job.Name` and `Step.Name`.
    fn range_scope_for_unclean_lets(
        &self,
        step: &StepTemplate,
        step_path: &[PathElement],
        errors: &mut ValidationErrors,
    ) -> SymbolTable {
        let mut symtab = self.step_base_scope();
        let ev = &self.template;
        for binding in step.let_bindings.iter().flatten() {
            let Some(eq_pos) = binding.find('=') else {
                continue;
            };
            let name = binding[..eq_pos].trim();
            let expr_str = binding[eq_pos + 1..].trim();
            if name.is_empty() || expr_str.is_empty() {
                continue;
            }
            let result = ParsedExpression::with_profile(expr_str, &ev.profile).and_then(|parsed| {
                ev.budgeted(parsed.with_library(&ev.lib))
                    .evaluate(&[&symtab as &SymbolTable])
            });
            match result {
                Ok(val) => {
                    let _ = symtab.set(name, val);
                }
                Err(e) => errors.add(step_path, format!("let binding '{name}': {e}")),
            }
        }
        symtab
    }

    /// What the session and task scopes share: every `Param.*` and
    /// `RawParam.*`, `Session.*`, and, with `EXPR`, `Job.Name` and, for a
    /// Step, `Step.Name`.
    fn host_scope(&self, step: bool) -> SymbolTable {
        let mut symtab = SymbolTable::new();
        add_params(&mut symtab, job_params(self.params), true);
        add_session_symbols(&mut symtab);
        if self.expr {
            add_job_symbols(&mut symtab);
            if step {
                symtab
                    .set("Step.Name", ExprValue::unresolved(ExprType::STRING))
                    .expect("symtab");
            }
        }
        symtab
    }

    /// `env`'s session scope: the host scope, the Environment's `Env.File.*`,
    /// and for a Step Environment, with `EXPR`, `Step.Name` and the values
    /// of the Step's `let` bindings from `step`, the Step and a scope that
    /// holds them.
    fn session_scope(
        &self,
        env: &Environment,
        step: Option<(&StepTemplate, &SymbolTable)>,
    ) -> SymbolTable {
        let mut symtab = self.host_scope(step.is_some());
        add_files(
            &mut symtab,
            "Env.",
            env.script
                .as_ref()
                .and_then(|s| s.embedded_files.as_deref()),
        );
        if let (true, Some((step, step_scope))) = (self.expr, step) {
            copy_step_lets(&mut symtab, step, step_scope, false);
        }
        symtab
    }

    /// `step`'s task scope, the scope of its script: the host scope with
    /// `Task.Param.*`, `Task.RawParam.*` (`string` for PATH), `Task.File.*`,
    /// and, with `EXPR`, `Step.Name` and the Step's `let` values from
    /// `step_scope`, its template scope. `Env.File.*` is only in an Environment's own scripts
    /// (§7.3).
    fn task_scope(&self, step: &StepTemplate, step_scope: &SymbolTable) -> SymbolTable {
        let mut symtab = self.host_scope(true);
        for tp in step
            .parameter_space
            .iter()
            .flat_map(|ps| &ps.task_parameter_definitions)
        {
            let t = match tp {
                TaskParameterDefinition::INT(_) => ExprType::INT,
                TaskParameterDefinition::CHUNK_INT(_) => ExprType::RANGE_EXPR,
                TaskParameterDefinition::FLOAT(_) => ExprType::FLOAT,
                TaskParameterDefinition::STRING(_) => ExprType::STRING,
                TaskParameterDefinition::PATH(_) => ExprType::PATH,
            };
            // The same, except that a PATH's is the unmapped string (§7.3.1).
            let raw = match tp {
                TaskParameterDefinition::PATH(_) => ExprType::STRING,
                _ => t.clone(),
            };
            let name = tp.name();
            for (key, t) in [
                (format!("Task.Param.{name}"), t),
                (format!("Task.RawParam.{name}"), raw),
            ] {
                symtab.set(&key, ExprValue::unresolved(t)).expect("symtab");
            }
        }
        let script = step.resolve_syntax_sugar();
        add_files(
            &mut symtab,
            "Task.",
            script.as_ref().and_then(|s| s.embedded_files.as_deref()),
        );
        if self.expr {
            copy_step_lets(&mut symtab, step, step_scope, true);
        }
        symtab
    }

    /// Validate `bindings` and evaluate them into `symtab` with `ev`, or,
    /// without `EXPR`, report them at `let_path`. `enclosing` holds the
    /// names an enclosing scope binds.
    fn validate_lets(
        &self,
        bindings: Option<&[String]>,
        let_path: &[PathElement],
        enclosing: &HashSet<String>,
        symtab: &mut SymbolTable,
        ev: &FsEval,
        errors: &mut ValidationErrors,
    ) {
        let Some(bindings) = bindings else { return };
        if self.expr {
            validate_let_bindings(bindings, let_path, enclosing, symtab, ev, errors);
        } else {
            errors.add(let_path, "'let' requires the EXPR extension.");
        }
    }
}

/// Soft cap, in characters, on the resolved length of a numeric
/// (`<posintstring>` / `<intstring>` / float-string) format-string field,
/// not counting white space.
///
/// White space around a numeric string is ignored (Template Schemas §7.5),
/// so it is not counted: see [`min_numeric_len`]. A format string padded
/// with white space therefore passes exactly when the same padded literal
/// does. There is no exact maximum on the rest either, because leading
/// zeros are permitted. Any value longer than this is unreasonable for a
/// numeric field, though (an i64 needs at most 20 characters), so a lower
/// bound past it is reported at template validation time rather than
/// failing later at job creation or on the worker.
const MAX_RESOLVED_NUMERIC_LEN: usize = 100;

/// Lower bound, in characters, on the length of what `fs` resolves to,
/// not counting white space (Template Schemas §7.5), for the cap
/// [`MAX_RESOLVED_NUMERIC_LEN`].
///
/// When the resolved value is known this is exact: the resolved text with
/// the white space around it removed. Otherwise (a segment is unresolved,
/// or the concatenation passed `MAX_STATIC_RESOLVED_VALUE_LEN` so no value
/// was kept) it is
/// `min_resolved_string_len` less the white space in the literal segments,
/// which always appears in the resolution. White space that a concrete
/// expression segment evaluates to (`{{ ' ' * 200 }}` before an
/// unresolved segment, or in a value too long to keep) still counts, since the static resolution only
/// reports the total length. White space is the Unicode `White_Space`
/// property, what `str::trim` removes, as in job creation's parse.
fn min_numeric_len(sr: &openjd_expr::StaticResolution, fs: &FormatString) -> usize {
    match &sr.resolved_value {
        Some(ExprValue::Null) => 0,
        Some(v) => v.to_display_string().trim().chars().count(),
        None => {
            let literal_ws: usize = fs
                .literal_segments()
                .map(|text| text.chars().filter(|c| c.is_whitespace()).count())
                .sum();
            sr.min_resolved_string_len.saturating_sub(literal_ws)
        }
    }
}

/// Longest valid resolved cancelation `mode` value.
const MODE_MAX_LEN: usize = "NOTIFY_THEN_TERMINATE".len();

/// §5 action `timeout` (`<posintstring>` with FEATURE_BUNDLE_1): the
/// resolved value must be a positive integer; whole-field `null` is unset.
const TIMEOUT_CONSTRAINT: ResolvedConstraint<'static> = ResolvedConstraint::Int {
    min: 1,
    min_msg: "timeout must be > 0.",
    max: None,
    parse_msg: "timeout must be a positive integer.",
    nullable: true,
};

/// §5.3.2 `notifyPeriodInSeconds`: positive integer with a spec-mandated
/// maximum of 600; whole-field `null` is unset.
const NOTIFY_PERIOD_CONSTRAINT: ResolvedConstraint<'static> = ResolvedConstraint::Int {
    min: 1,
    min_msg: "notifyPeriodInSeconds must be > 0.",
    max: Some((600, "notifyPeriodInSeconds must not exceed 600.")),
    parse_msg: "notifyPeriodInSeconds must be a positive integer.",
    nullable: true,
};

/// A spec-mandated constraint on the value a format string resolves to
/// — a constraint the spec applies to the value the format string
/// resolves to, "after the format string has been resolved" in the
/// spec's wording. Enforcing it here makes template validation the
/// earliest of the spec's three processing stages (Template Schemas
/// §7.4: template validation, job creation, task execution on the
/// worker host) to catch a violation that is already knowable.
///
/// Applied to the [`openjd_expr::StaticResolution`] that
/// `validate_expressions` returns, in two stages:
///
/// 1. **Lower bound** — if `min_resolved_string_len` already exceeds the
///    field's limit, no run-time resolution can conform, so validation
///    fails without knowing the unresolved parts. Numeric fields use the
///    soft [`MAX_RESOLVED_NUMERIC_LEN`] on the length without white space
///    ([`min_numeric_len`]; no exact maximum exists — leading zeros); the
///    cancelation mode uses the longest valid enum literal.
/// 2. **Full value check** — when `resolved_value` is present the field is
///    fully static, and the same check job creation or the worker would
///    run on the resolved value runs here.
///
/// Target types match resolution: every constrained field resolves its
/// single whole-field expressions with the schema-derived target type
/// from Expression Language §1.3.2 ([`Self::target_type`]), and job
/// creation and the session runtime resolve with the same targets.
/// Multi-segment strings concatenate
/// to a string regardless, and the stage-2 checks mirror the downstream
/// `display → trim → parse` handling for that case exactly.
///
/// Literal (non-interpolated) fields are excluded for the spec-mandated
/// constraints: the raw-text passes (structure/limits) already check
/// those, and for literals raw text and resolved value coincide. The
/// opt-in caps ([`Self::ResolvedString`] / [`Self::ArgListItem`]) have no
/// raw-text pass, so `validate_fs_with` checks a literal field's exact
/// raw length against them instead.
enum ResolvedConstraint<'a> {
    /// A string field with a maximum resolved length in characters.
    /// `forbid_control_chars` and `forbid_empty` add the §1.1.1 checks
    /// for the job name when the value is fully static (environment
    /// variable values set neither: §4.4.2's minimum length is 0 and any
    /// character is allowed).
    Text {
        max_len: usize,
        forbid_control_chars: bool,
        forbid_empty: bool,
    },
    /// A list-item string field (STRING and PATH task-parameter range
    /// elements, Template Schemas §3.4.2; INT, CHUNK[INT] and FLOAT
    /// elements of a template with EXPR use [`Self::NumericListItem`]). Per Expression Language §1.3.2 a
    /// whole-field expression targets `string? | list[string]`: `null`
    /// skips the element, a list flattens inline, and `max_len` applies
    /// to each resulting element. `forbid_empty` applies §3.4.2's
    /// minimum length of 1 — set for PATH elements only, matching the
    /// reference implementation, which accepts empty STRING elements
    /// (see `resolve_string_range` in create_job/ranges.rs).
    TextListItem { max_len: usize, forbid_empty: bool },
    /// A format string element of an INT, CHUNK[INT] or FLOAT range list
    /// (Template Schemas §3.4.1.1, §3.4.1.2) with the EXPR extension, with
    /// the list-item `target` from
    /// [`TaskParameterType::range_list_item_target`]: `null` skips the
    /// element and a list flattens. Static typing against the target is
    /// the whole check here. A value that only fails once known, such as a
    /// string that does not parse as a number, fails at job creation.
    /// Without EXPR these elements resolve to strings and have no
    /// constraint.
    ///
    /// [`TaskParameterType::range_list_item_target`]: crate::types::TaskParameterType::range_list_item_target
    NumericListItem { target: ExprType },
    /// A `hostRequirements` attribute value (§3.3.2.2): 100-char
    /// identifier-like values, or membership in the allowed set for a
    /// standard capability. Also a list item, with the same
    /// `string? | list[string]` skip/flatten semantics as
    /// [`Self::TextListItem`]; the §3.3.2.2 checks apply to each
    /// resulting element.
    AttributeValue {
        capability_name: &'a str,
        standard: &'static [(&'static str, &'static [&'static str])],
    },
    /// A `hostRequirements` amount or attribute `name` (§3.3.1.1 /
    /// §3.3.2.1): at most 100 characters, and when fully static, the
    /// capability name pattern for `kind` and the reserved scopes, checked
    /// by [`helpers::check_capability_name`] exactly as for a literal name
    /// and the resolved name at job creation. `standard` holds the
    /// standard capability names for `kind`.
    CapabilityName {
        kind: helpers::CapabilityKind,
        standard: &'a [&'a str],
    },
    /// An integer field (`<posintstring>` / `<intstring>`), resolved
    /// with target `int` — or `int?` when `nullable`, where a
    /// whole-field `null` resolution means the field is unset (schema
    /// defaults apply) — so a static `{{ 120.0 }}` coerces to the int it
    /// denotes. The messages match the raw-text checks for the literal
    /// forms of the same fields.
    Int {
        min: i64,
        min_msg: &'static str,
        max: Option<(i64, &'static str)>,
        parse_msg: &'static str,
        nullable: bool,
    },
    /// An amount capability bound (`min`/`max`): non-negative or
    /// strictly-positive finite float.
    Float { positive: bool, msg: &'static str },
    /// The cancelation `mode` enum (FEATURE_BUNDLE_1 deferred form):
    /// resolves to `TERMINATE`, `NOTIFY_THEN_TERMINATE`, or `null`.
    CancelationMode,
    /// A field consumed as one whole string with an opt-in caller cap:
    /// an action `command` (§5.1, `CallerLimits::max_resolved_arg_len`)
    /// or an embedded-file `data` value (§6.1.2,
    /// `CallerLimits::max_resolved_data_len`). The spec sets no maximum
    /// of its own. Resolution is `resolve_string_with` with **no**
    /// target type — every value renders inline into a single string —
    /// so the length bound applies unconditionally and there are no
    /// skip/flatten semantics.
    ResolvedString { max_len: usize },
    /// An action `args` element (§5.2) under the opt-in
    /// `CallerLimits::max_resolved_arg_len` cap. The session runtime
    /// (`resolve_action_args`) resolves each element with `resolve_with`
    /// and **no** target type: `null` skips the element, a list flattens
    /// into one argv entry per element, and anything else becomes one
    /// argv entry via its display form — so the cap applies to each
    /// resulting entry, with the same certainly-a-string gating as
    /// [`Self::TextListItem`].
    ArgListItem { max_len: usize },
}

impl ResolvedConstraint<'_> {
    /// The target type the field's resolution uses, per Expression
    /// Language §1.3.2 (single whole-field expressions only —
    /// multi-segment strings concatenate regardless, exactly as in
    /// resolution): `string` for the required scalar string fields,
    /// `string? | list[string]` for list items (range elements,
    /// attribute values — `null` skips, a list flattens), `float?`
    /// for the optional amount bounds, `int?` for `timeout` and
    /// `notifyPeriodInSeconds` (§5/§5.3.2 — `null` means "not
    /// provided"), plain `int` for the required `defaultTaskCount`, and
    /// `string?` for the deferred cancelation `mode`. Job creation and
    /// the session runtime resolve with the same targets — a field's
    /// validation-time target must always equal its resolution-time
    /// target. `None` for the fields whose resolution passes no target
    /// type at all (action `command`/`args`, embedded-file `data` —
    /// see `resolve_action_args` and embedded-file materialization in
    /// `openjd-sessions`).
    fn target_type(&self) -> Option<ExprType> {
        match self {
            Self::Text { .. } | Self::CapabilityName { .. } => Some(ExprType::STRING),
            Self::TextListItem { .. } | Self::AttributeValue { .. } => {
                Some(crate::types::string_list_item_target())
            }
            Self::NumericListItem { target } => Some(target.clone()),
            Self::Float { .. } => Some(fixed_type(ExprType::union(vec![
                ExprType::FLOAT,
                ExprType::NULLTYPE,
            ]))),
            Self::Int { nullable, .. } => {
                if *nullable {
                    Some(fixed_type(ExprType::union(vec![
                        ExprType::INT,
                        ExprType::NULLTYPE,
                    ])))
                } else {
                    Some(ExprType::INT)
                }
            }
            Self::CancelationMode => Some(fixed_type(ExprType::union(vec![
                ExprType::STRING,
                ExprType::NULLTYPE,
            ]))),
            Self::ResolvedString { .. } | Self::ArgListItem { .. } => None,
        }
    }
}

impl ResolvedConstraint<'_> {
    /// Apply this constraint to what static evaluation determined, `sr`, of
    /// the format string `fs` at `path`, with one helper function per
    /// variant.
    fn check(
        &self,
        sr: &openjd_expr::StaticResolution,
        fs: &FormatString,
        path: &[PathElement],
        errors: &mut ValidationErrors,
    ) {
        match self {
            Self::Text {
                max_len,
                forbid_control_chars,
                forbid_empty,
            } => check_text(
                sr,
                *max_len,
                *forbid_control_chars,
                *forbid_empty,
                path,
                errors,
            ),
            Self::TextListItem {
                max_len,
                forbid_empty,
            } => check_list_item_len(sr, *max_len, *forbid_empty, path, errors),
            // Static typing already ran with the target. See the variant doc.
            Self::NumericListItem { .. } => {}
            // `resolve_string_with` renders every value inline into one
            // string, so the display-form bound is a true lower bound on the
            // final string whatever the segment types. Exact when fully
            // static.
            Self::ResolvedString { max_len } => {
                check_min_len(sr, *max_len, path, errors);
            }
            // An argv entry has no minimum length.
            Self::ArgListItem { max_len } => check_list_item_len(sr, *max_len, false, path, errors),
            Self::AttributeValue {
                capability_name,
                standard,
            } => check_attribute_value(sr, capability_name, standard, path, errors),
            Self::CapabilityName { kind, standard } => {
                check_capability_name(sr, *kind, standard, path, errors)
            }
            Self::Int {
                min,
                min_msg,
                max,
                parse_msg,
                nullable,
            } => check_int(
                sr, fs, *min, min_msg, *max, parse_msg, *nullable, path, errors,
            ),
            Self::Float { positive, msg } => check_float(sr, fs, *positive, msg, path, errors),
            Self::CancelationMode => check_cancelation_mode(sr, path, errors),
        }
    }
}

/// The fully static resolved text, exactly as job creation's
/// `resolve_string_with` would produce it: the display form, with a
/// whole-field `null` interpolating as unset (`None` here).
fn static_text(sr: &openjd_expr::StaticResolution) -> Option<String> {
    match &sr.resolved_value {
        None | Some(ExprValue::Null) => None,
        Some(v) => Some(v.to_display_string()),
    }
}

/// Report a resolution whose lower bound on its length exceeds `max_len`.
/// Returns whether it did.
fn check_min_len(
    sr: &openjd_expr::StaticResolution,
    max_len: usize,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) -> bool {
    let exceeds = sr.min_resolved_string_len > max_len;
    if exceeds {
        errors.add(
            path,
            format!(
                "resolves to at least {} characters, exceeding the maximum of {max_len}.",
                sr.min_resolved_string_len
            ),
        );
    }
    exceeds
}

/// [`ResolvedConstraint::Text`]. The bound is exact when the value is fully
/// static, so one check covers both the bound-only and static cases.
fn check_text(
    sr: &openjd_expr::StaticResolution,
    max_len: usize,
    forbid_control_chars: bool,
    forbid_empty: bool,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    check_min_len(sr, max_len, path, errors);
    if let Some(s) = static_text(sr) {
        // §1.1.1 minimum length 1: a fully static empty resolution can
        // never conform. (A partially unresolved string may still resolve
        // empty; job creation re-checks the resolved value.)
        if forbid_empty && s.is_empty() {
            errors.add(path, "must not resolve to an empty string.");
        }
        if forbid_control_chars && s.chars().any(char::is_control) {
            errors.add(path, "contains control characters.");
        }
    }
}

/// [`ResolvedConstraint::TextListItem`] and
/// [`ResolvedConstraint::ArgListItem`]: a list item that `null` skips and a
/// list flattens (Expression Language §1.3.2), at most `max_len` characters
/// per resulting element and, with `forbid_empty`, at least 1 (§3.4.2).
fn check_list_item_len(
    sr: &openjd_expr::StaticResolution,
    max_len: usize,
    forbid_empty: bool,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    // The length bound is only sound when the resolution is certainly a
    // string: a list-valued resolution distributes its characters across
    // elements, so the display-form length says nothing about any single
    // element. Multi-segment strings are always strings, and a fully static
    // string is exact, so return rather than report the same violation
    // again per element.
    if sr.resolved_type == ExprType::STRING && check_min_len(sr, max_len, path, errors) {
        return;
    }
    // A fully static list flattens into one element per member: the limits
    // apply to each. A static `null` skips the element, which is valid here;
    // job creation re-checks that a range is non-empty after the skips.
    let Some(v) = &sr.resolved_value else { return };
    let elements = v.list_elements().unwrap_or_else(|| vec![v.clone()]);
    for (i, elem) in elements.iter().enumerate() {
        if matches!(elem, ExprValue::Null) {
            continue;
        }
        let n = elem.to_display_string().chars().count();
        let element_label = if v.is_list() {
            format!("list element {i} ")
        } else {
            String::new()
        };
        if n > max_len {
            errors.add(
                path,
                format!("{element_label}resolves to {n} characters, exceeding the maximum of {max_len}."),
            );
        } else if n == 0 && forbid_empty {
            errors.add(
                path,
                format!("{element_label}must not resolve to an empty string."),
            );
        }
    }
}

/// [`ResolvedConstraint::AttributeValue`].
fn check_attribute_value(
    sr: &openjd_expr::StaticResolution,
    capability_name: &str,
    standard: &[(&str, &[&str])],
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let lower = capability_name.to_lowercase();
    let allowed = standard
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, vals)| *vals);
    // Length bounds gated on a certainly-string resolution, as for a list
    // item: a list-valued resolution is checked per element below.
    if sr.resolved_type == ExprType::STRING {
        match allowed {
            // Standard capability: values come from a fixed set, so no
            // resolution longer than the longest member can conform.
            Some(vals) => {
                let longest = vals.iter().map(|v| v.chars().count()).max().unwrap_or(0);
                if sr.min_resolved_string_len > longest {
                    errors.add(
                        path,
                        format!(
                            "resolves to at least {} characters; no valid value for {} is longer than {} characters.",
                            sr.min_resolved_string_len, lower, longest
                        ),
                    );
                    return;
                }
            }
            // Identifier-like capability: §3.3.2.2 max of 100.
            None => {
                if check_min_len(sr, 100, path, errors) {
                    return;
                }
            }
        }
    }
    // Fully static: the §3.3.2.2 check runs on the value, or on each
    // element when the item flattens (Expression Language §1.3.2). A static
    // `null` skips the element; job creation re-checks non-emptiness after
    // skips.
    let Some(v) = sr
        .resolved_value
        .as_ref()
        .filter(|v| !matches!(v, ExprValue::Null))
    else {
        return;
    };
    for elem in v.list_elements().unwrap_or_else(|| vec![v.clone()]) {
        if let Err(message) = crate::capabilities::validate_attribute_capability_value(
            capability_name,
            &elem.to_display_string(),
            standard,
        ) {
            errors.add(path, message);
        }
    }
}

/// [`ResolvedConstraint::CapabilityName`].
fn check_capability_name(
    sr: &openjd_expr::StaticResolution,
    kind: helpers::CapabilityKind,
    standard: &[&str],
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    // Fully static: run the check job creation runs on the resolved name. It
    // includes the length, so the bound below would only report the same
    // violation twice.
    if let Some(name) = static_text(sr) {
        helpers::check_capability_name(&name, kind, standard, path, errors);
        return;
    }
    // The target type is `string`, so the resolution is certainly a string
    // and the bound applies. The pattern and reserved-scope checks need the
    // whole name, so job creation applies them.
    check_min_len(sr, 100, path, errors);
}

/// [`ResolvedConstraint::Int`].
#[allow(clippy::too_many_arguments)]
fn check_int(
    sr: &openjd_expr::StaticResolution,
    fs: &FormatString,
    min: i64,
    min_msg: &str,
    max: Option<(i64, &str)>,
    parse_msg: &str,
    nullable: bool,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    // Stage 1: soft character bound, not counting white space (§7.5). No
    // exact maximum exists (leading zeros), but a resolution this long
    // cannot be a reasonable integer.
    let len = min_numeric_len(sr, fs);
    if len > MAX_RESOLVED_NUMERIC_LEN {
        errors.add(
            path,
            format!(
                "resolves to at least {len} characters, which cannot be a reasonable integer value."
            ),
        );
        return;
    }
    // Stage 2: exact value check when fully static, mirroring the downstream
    // handling: a whole-field expression is already the coerced int (or
    // null); a multi-segment string resolves to text and parses with
    // surrounding white space tolerated, like Python's `int()`, as job
    // creation and the session runtime do.
    let value = match &sr.resolved_value {
        None => return,
        Some(ExprValue::Null) => {
            if !nullable {
                errors.add(path, parse_msg);
            }
            return;
        }
        Some(ExprValue::Int(v)) => *v,
        Some(other) => match other.to_display_string().trim().parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                errors.add(path, parse_msg);
                return;
            }
        },
    };
    if value < min {
        errors.add(path, min_msg);
    } else if let Some((max_v, max_msg)) = max {
        if value > max_v {
            errors.add(path, max_msg);
        }
    }
}

/// [`ResolvedConstraint::Float`].
fn check_float(
    sr: &openjd_expr::StaticResolution,
    fs: &FormatString,
    positive: bool,
    msg: &str,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    // Soft character bound, not counting white space (§7.5).
    let len = min_numeric_len(sr, fs);
    if len > MAX_RESOLVED_NUMERIC_LEN {
        errors.add(
            path,
            format!("resolves to at least {len} characters, which cannot be a reasonable number."),
        );
        return;
    }
    if let Some(s) = static_text(sr) {
        match s.trim().parse::<f64>() {
            Ok(v) if !v.is_finite() => errors.add(path, "must be a finite number."),
            Ok(v) if (positive && v <= 0.0) || (!positive && v < 0.0) => errors.add(path, msg),
            Ok(_) => {}
            Err(_) => errors.add(path, "must be a finite number."),
        }
    }
}

/// [`ResolvedConstraint::CancelationMode`]. A whole-field `null` means the
/// cancelation is not provided.
fn check_cancelation_mode(
    sr: &openjd_expr::StaticResolution,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if sr.min_resolved_string_len > MODE_MAX_LEN {
        errors.add(
            path,
            format!(
                "mode resolves to at least {} characters; valid values are TERMINATE and NOTIFY_THEN_TERMINATE.",
                sr.min_resolved_string_len
            ),
        );
        return;
    }
    if let Some(s) = static_text(sr) {
        if s != "TERMINATE" && s != "NOTIFY_THEN_TERMINATE" {
            errors.add(
                path,
                format!("mode must resolve to TERMINATE or NOTIFY_THEN_TERMINATE, got '{s}'."),
            );
        }
    }
}

/// Profile, function library and caller evaluation budgets for one validation
/// scope. Pass 8 and the job-creation re-checks both evaluate every
/// format-string expression; the budgets
/// (`CallerLimits::max_eval_memory_bytes` / `max_eval_operations`) bound
/// each of those evaluations exactly as they bound resolution at job
/// creation and run time, so a lowered budget fails at the earliest
/// stage that can detect it. Evaluation/parse errors are always
/// reported: `create_job` requires a context that enables the
/// template's declared extensions (see `create_job`'s `ctx` docs), so
/// an error at either stage is a template defect or a deterministic
/// value-dependent failure that every session would hit.
///
/// Every evaluation runs under `PathFormat::Posix`: template validation
/// and job creation happen outside host context, where the model keeps
/// all paths POSIX (only `openjd-sessions` evaluates under
/// `PathFormat::host()` — see `preprocess_job_parameters` in
/// `specs/model/job-creation.md`, which explains why paths stay POSIX
/// until template evaluation on the host). This keeps the two stages consistent
/// with each other and with the POSIX-format values `create_job` seeds
/// into its check symbol tables, and makes validation outcomes
/// independent of the OS running them.
///
/// A path built from a literal in a host-scope field is not folded in
/// POSIX: the host-scope library (`HostContext::Unresolved`) returns
/// `unresolved[path]` from `path()`, because the worker's path format
/// isn't known yet, and a TEMPLATE-scope path reaching a host-scope
/// symbol table is unresolved too ([`unresolve_template_paths`]; see
/// specs/model/validation.md § Path Format).
struct FsEval {
    profile: openjd_expr::ExprProfile,
    lib: std::sync::Arc<FunctionLibrary>,
    memory_limit: Option<usize>,
    operation_limit: Option<usize>,
}

impl FsEval {
    /// The evaluator for `ctx`'s profile in `host_context`: `None` for
    /// template scope, `Unresolved` for session and task scopes, where host
    /// functions such as `apply_path_mapping` type-check.
    fn new(ctx: &ValidationContext, host_context: openjd_expr::HostContext) -> Self {
        let profile = ctx.profile.to_expr_profile(host_context);
        Self {
            lib: FunctionLibrary::for_profile(&profile),
            profile,
            memory_limit: ctx.caller_limits.max_eval_memory_bytes,
            operation_limit: ctx.caller_limits.max_eval_operations,
        }
    }

    /// Evaluation options matching how the field will later resolve,
    /// with this scope's library and the caller budgets applied.
    fn options<'b>(
        &'b self,
        target: Option<&'b openjd_expr::ExprType>,
    ) -> openjd_expr::FormatStringOptions<'b> {
        let mut opts = openjd_expr::FormatStringOptions::new()
            .with_library(&*self.lib)
            .with_path_format(PathFormat::Posix);
        if let Some(t) = target {
            opts = opts.with_target_type(t);
        }
        if let Some(m) = self.memory_limit {
            opts = opts.with_memory_limit(m);
        }
        if let Some(o) = self.operation_limit {
            opts = opts.with_operation_limit(o);
        }
        opts
    }

    /// Apply the caller budgets and the POSIX path format (see the
    /// struct docs) to a raw [`openjd_expr::EvalBuilder`] — for the
    /// evaluation sites (let bindings) that parse expressions
    /// directly rather than resolving a `FormatString`.
    fn budgeted<'b>(
        &self,
        mut builder: openjd_expr::EvalBuilder<'b>,
    ) -> openjd_expr::EvalBuilder<'b> {
        builder = builder.with_path_format(PathFormat::Posix);
        if let Some(m) = self.memory_limit {
            builder = builder.with_memory_limit(m);
        }
        if let Some(o) = self.operation_limit {
            builder = builder.with_operation_limit(o);
        }
        builder
    }
}

/// Validate a format string against a symbol table, reporting errors at the given path.
fn validate_fs(
    fs: &FormatString,
    symtab: &SymbolTable,
    ev: &FsEval,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    validate_fs_with(fs, symtab, ev, path, None, errors);
}

/// Validate the format strings in an INT or CHUNK[INT] `range` at
/// `range_path`: the whole-field `<IntRangeExpr>`, or each
/// `<TaskParameterStringValue>` element of an `<IntRangeList>` (Template
/// Schemas §3.4.1.1) at `range_path[k]`. A list element is checked with
/// `item_constraint`, which carries the target `resolve_int_range` resolves
/// it with (none without EXPR). The expression has no target:
/// `resolve_int_range` evaluates it untyped and then, failing a range or
/// list result, resolves it to a string. Literal elements were checked at
/// decode.
fn validate_int_range_fs(
    range: &crate::template::IntRange,
    item_constraint: Option<&ResolvedConstraint<'_>>,
    symtab: &SymbolTable,
    ev: &FsEval,
    range_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    match range {
        crate::template::IntRange::Expression(expr) => {
            validate_fs(expr, symtab, ev, range_path, errors);
        }
        crate::template::IntRange::List(items) => {
            for (k, item) in items.iter().enumerate() {
                if let crate::template::IntOrFormatString::FormatString(fs) = item {
                    validate_fs_with(
                        fs,
                        symtab,
                        ev,
                        &path_index(range_path, k),
                        item_constraint,
                        errors,
                    );
                }
            }
        }
    }
}

/// A `hostRequirements` capability name whose value template validation
/// knows: a literal, or a format string that is fully static.
struct KnownCapabilityName {
    name: String,
    /// Structure already checked a literal name, including its uniqueness
    /// against the other literal names.
    literal: bool,
}

/// The value of a capability name, when template validation knows it. `sr`
/// is what [`validate_fs_with`] returned for the name.
fn known_capability_name(
    name: &FormatString,
    sr: Option<&openjd_expr::StaticResolution>,
) -> Option<KnownCapabilityName> {
    if name.is_literal() {
        return Some(KnownCapabilityName {
            name: name.raw().to_string(),
            literal: true,
        });
    }
    match sr?.resolved_value.as_ref()? {
        ExprValue::Null => None,
        v => Some(KnownCapabilityName {
            name: v.to_display_string(),
            literal: false,
        }),
    }
}

/// Check the §3.3 constraint that no two amounts, and no two attributes,
/// have the same resolved name, for the names template validation knows.
///
/// Structure reports duplicates between literal names, so a pair is only
/// reported here when at least one of its names is fully static. A name
/// that is not known yet is compared at job creation. Comparison is
/// case-insensitive, and the message matches the one for literal names.
fn check_known_names_unique(
    known: &[Option<KnownCapabilityName>],
    kind: helpers::CapabilityKind,
    list_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let mut seen: Vec<(String, bool)> = Vec::new();
    for (index, k) in known.iter().enumerate() {
        let Some(k) = k else { continue };
        let lower = k.name.to_lowercase();
        if seen
            .iter()
            .any(|(name, literal)| *name == lower && !(*literal && k.literal))
        {
            errors.add(
                &path_index(list_path, index),
                format!("duplicate {} name '{}'.", kind.noun(), k.name),
            );
        }
        seen.push((lower, k.literal));
    }
}

/// Apply the standard-capability checks on an attribute's literal values
/// when its name is fully static.
///
/// Structure applies these for a literal name and cannot see a static one,
/// and the `AttributeValue` constraint skips literal values. The checks are
/// the ones job creation runs on the resolved values: membership in the
/// standard capability's allowed values, and the single-valued `allOf`
/// rule. A static name that is not a standard capability needs nothing
/// here, since structure already checked the literal values' pattern.
fn check_values_for_static_name(
    capability_name: &str,
    attr: &AttributeRequirement,
    standard: &[(&str, &[&str])],
    attr_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let lower = capability_name.to_lowercase();
    if !standard.iter().any(|(name, _)| *name == lower) {
        return;
    }
    for (field, vals) in [("anyOf", &attr.any_of), ("allOf", &attr.all_of)] {
        for (k, v) in vals.iter().flatten().enumerate() {
            if !v.is_literal() {
                continue;
            }
            if let Err(message) = crate::capabilities::validate_attribute_capability_value(
                capability_name,
                v.raw(),
                standard,
            ) {
                errors.add(&path_index(&path_field(attr_path, field), k), message);
            }
        }
    }
    helpers::check_single_valued_all_of(capability_name, attr.all_of.as_deref(), attr_path, errors);
}

/// [`validate_fs`] plus a spec-mandated resolved-value constraint
/// (see the Spec-Mandated Resolved-Value Constraints section of
/// `specs/model/validation.md`), applied to the
/// [`openjd_expr::StaticResolution`] the validation already computes.
///
/// Returns that resolution, so a caller can apply checks that span more
/// than one field — such as the uniqueness of `hostRequirements` names —
/// to the values that are fully static. `None` for a literal (its raw text
/// is its value) or when the expressions fail to validate.
fn validate_fs_with(
    fs: &FormatString,
    symtab: &SymbolTable,
    ev: &FsEval,
    path: &[PathElement],
    constraint: Option<&ResolvedConstraint<'_>>,
    errors: &mut ValidationErrors,
) -> Option<openjd_expr::StaticResolution> {
    if fs.is_literal() {
        // Spec-mandated constraints are covered for literal fields by the
        // raw-text passes (structure/limits) — raw text and resolved value
        // coincide, so re-checking here would double-report. The opt-in
        // caps have no raw-text pass, and a literal is the one case where
        // the resolved value is known exactly: one argv entry / one data
        // blob of exactly the raw text.
        if let Some(
            ResolvedConstraint::ResolvedString { max_len }
            | ResolvedConstraint::ArgListItem { max_len },
        ) = constraint
        {
            let n = fs.raw().chars().count();
            if n > *max_len {
                errors.add(
                    path,
                    format!("is {n} characters, exceeding the maximum of {max_len}."),
                );
            }
        }
        return None;
    }
    let target = constraint.and_then(ResolvedConstraint::target_type);
    match fs.validate_expressions(symtab, &ev.options(target.as_ref())) {
        Ok(sr) => {
            if let Some(c) = constraint {
                c.check(&sr, fs, path, errors);
            }
            Some(sr)
        }
        Err(e) => {
            add_fs_validation_error(&e, path, errors);
            None
        }
    }
}

/// Report a failed [`FormatString::validate_expressions`] at `path`, with
/// one diagnostic span per sub-error (or one covering the interpolation).
fn add_fs_validation_error(
    e: &openjd_expr::FormatStringValidationError,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let mut spans = Vec::new();
    if let Some(ref expr_err) = e.expression_error {
        // A compound error (e.g., if/else both branches fail): one span per
        // sub-error.
        for sub in expr_err.sub_errors() {
            if let (Some(expr), Some(col), Some(end_col)) =
                (sub.expr(), sub.col_offset(), sub.end_col_offset())
            {
                spans.push(crate::error::DiagnosticSpan {
                    summary: sub.message(),
                    source: expr.to_string(),
                    start: col,
                    end: end_col,
                    caret: sub.caret_offset().unwrap_or(0),
                });
            }
        }
    }
    // Fallback: single span covering the whole interpolation
    if spans.is_empty() {
        spans.push(crate::error::DiagnosticSpan {
            summary: e.message.clone(),
            source: e.input.clone(),
            start: e.start,
            end: e.end,
            caret: 0,
        });
    }
    let summary = if let Some(ref expr_err) = e.expression_error {
        expr_err.message()
    } else {
        e.message.clone()
    };
    let detail = crate::error::ErrorDetail { summary, spans };
    errors.add_with_detail(
        path,
        format!(
            "Failed to parse interpolation expression at [{}, {}]. {}",
            e.start, e.end, e.message
        ),
        detail,
    );
}

/// Pass-8 checks for one step script (§3.5), rooted at `script_path`:
/// script-level `let` bindings (evaluated into `task_symtab`, the Step's
/// task scope), the EXPR-gated complex-expression rejection, the body
/// ([`validate_step_script_body`]), and comprehension-variable rules against
/// the Step and script `let` names. `step_lets` are the Step's `let`
/// bindings and `step_template_symtab` its template scope. Their names are
/// collected where they are needed and dropped before the body is checked.
///
/// Shared by the authored `script` field and the desugared SimpleAction
/// forms (§8); the latter run it into a scratch collection rooted at `[]`
/// and re-root the paths onto the sugar field via
/// [`SimpleActionKind::remap_desugared_path`].
fn validate_step_script(
    f: &FsCtx<'_>,
    script: &StepScript,
    script_path: &[PathElement],
    step_lets: Option<&[String]>,
    task_symtab: &mut SymbolTable,
    step_template_symtab: &SymbolTable,
    errors: &mut ValidationErrors,
) {
    // Script-level `let` bindings are in task scope. `Task.File.*` is in
    // it: a Session allocates the files before it evaluates `let`. The
    // enclosing names are collected only when there are bindings to check.
    if let Some(bindings) = &script.let_bindings {
        f.validate_lets(
            Some(bindings),
            &path_field(script_path, "let"),
            &let_names(step_lets),
            task_symtab,
            &f.host,
            errors,
        );
    }
    let action_path = path_field(&path_field(script_path, "actions"), "onRun");
    if !f.expr {
        reject_complex_action(&script.actions.on_run, &action_path, errors);
    }
    validate_step_script_body(
        f,
        script,
        task_symtab,
        step_template_symtab,
        script_path,
        errors,
    );
    if !f.expr {
        return;
    }
    let mut all_let_names = let_names(step_lets);
    all_let_names.extend(let_names(script.let_bindings.as_deref()));
    if all_let_names.is_empty() {
        return;
    }
    let action = &script.actions.on_run;
    if let Err(e) = action.command.validate_comprehension_vars(&all_let_names) {
        errors.add(&path_field(&action_path, "command"), e.to_string());
    }
    let args_path = path_field(&action_path, "args");
    for (j, arg) in action.args.iter().flatten().enumerate() {
        if let Err(e) = arg.validate_comprehension_vars(&all_let_names) {
            errors.add(&path_index(&args_path, j), e.to_string());
        }
    }
    // Embedded-file `data` sees the same `let` names as `args` (a
    // SimpleAction's `script` desugars to it).
    let files_path = path_field(script_path, "embeddedFiles");
    for (j, file) in script.embedded_files.iter().flatten().enumerate() {
        if let Some(data) = &file.data {
            if let Err(e) = data.validate_comprehension_vars(&all_let_names) {
                errors.add(
                    &path_field(&path_index(&files_path, j), "data"),
                    e.to_string(),
                );
            }
        }
    }
}

/// The format strings of a step script's body at `script_path`: the
/// `onRun` action's `command` and `args` in `symtab`, its task scope, its
/// `timeout` and `cancelation` in `template_symtab`, the Step's template
/// scope, and each embedded file's `data` in `symtab`. Pass 8 and job
/// creation's re-check ([`check_carried_forward_step_script`]) both run it.
fn validate_step_script_body(
    f: &FsCtx<'_>,
    script: &StepScript,
    symtab: &SymbolTable,
    template_symtab: &SymbolTable,
    script_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let action_path = path_field(&path_field(script_path, "actions"), "onRun");
    let action = &script.actions.on_run;
    let max_arg_len = f.ctx.caller_limits.max_resolved_arg_len;
    validate_action_fs(action, symtab, &f.host, &action_path, max_arg_len, errors);
    // `timeout` and `notifyPeriodInSeconds` are plain `@fmtstring`, resolved
    // at job creation before any Session exists, so they are in template
    // scope: no `Session.*`, `Task.*`, `Env.File.*`, or host functions.
    validate_action_timing_fs(action, template_symtab, &f.template, &action_path, errors);
    check_embedded_files_data(
        f,
        script.embedded_files.as_deref(),
        symtab,
        false,
        script_path,
        errors,
    );
}

/// Validate an action's template-scope numeric/enum fields: `timeout`
/// against [`TIMEOUT_CONSTRAINT`], and the cancelation's deferred `mode`
/// ([`ResolvedConstraint::CancelationMode`]) and `notifyPeriodInSeconds`
/// ([`NOTIFY_PERIOD_CONSTRAINT`]) — the latter two reported at the
/// `cancelation` path. `symtab`/`ev` are the template scope (plus the
/// unresolved `WrappedAction.*` scope for an RFC 0008 wrap hook).
///
/// Shared by pass 8 and the job-creation re-check, so the constraints,
/// target types, paths, and messages cannot drift between the stages.
fn validate_action_timing_fs(
    action: &Action,
    symtab: &SymbolTable,
    ev: &FsEval,
    action_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if let Some(timeout) = &action.timeout {
        validate_fs_with(
            timeout,
            symtab,
            ev,
            &path_field(action_path, "timeout"),
            Some(&TIMEOUT_CONSTRAINT),
            errors,
        );
    }
    let (mode_fs, notify_fs) = match &action.cancelation {
        Some(CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds,
        }) => (None, notify_period_in_seconds.as_ref()),
        Some(CancelationMode::DeferredMode {
            mode,
            notify_period_in_seconds,
        }) => (Some(mode), notify_period_in_seconds.as_ref()),
        _ => (None, None),
    };
    if let Some(mode) = mode_fs {
        validate_fs_with(
            mode,
            symtab,
            ev,
            &path_field(action_path, "cancelation"),
            Some(&ResolvedConstraint::CancelationMode),
            errors,
        );
    }
    if let Some(notify) = notify_fs {
        validate_fs_with(
            notify,
            symtab,
            ev,
            &path_field(action_path, "cancelation"),
            Some(&NOTIFY_PERIOD_CONSTRAINT),
            errors,
        );
    }
}

/// [`validate_action_timing_fs`] for every action of an environment
/// script. On the plain lifecycle actions (`onEnter`/`onExit`) these
/// fields resolve at job creation, before any session exists, so they
/// validate against the template-scope `template_symtab`. On the RFC 0008
/// wrap hooks they resolve at run time with the `WrappedAction.*`
/// variables seeded — that is what makes round-trip forwarding
/// (`timeout: "{{WrappedAction.Timeout}}"`,
/// `mode: "{{WrappedAction.Cancelation.Mode}}"`) possible — so they see
/// that scope too, unresolved.
fn validate_env_actions_timing_fs(
    script: &EnvironmentScript,
    template_symtab: &SymbolTable,
    template_ev: &FsEval,
    actions_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let wrap_hook_names: [&str; 3] = ["onWrapEnvEnter", "onWrapTaskRun", "onWrapEnvExit"];
    for (name, action) in script.actions.iter_named() {
        let action_path = path_field(actions_path, name);
        let scoped_symtab: SymbolTable;
        let field_symtab: &SymbolTable = if wrap_hook_names.contains(&name) {
            let mut st = template_symtab.clone();
            add_wrapped_action_scope(&mut st);
            scoped_symtab = st;
            &scoped_symtab
        } else {
            template_symtab
        };
        validate_action_timing_fs(action, field_symtab, template_ev, &action_path, errors);
    }
}

/// Validate a format string in an action (command + args). When the
/// caller opted into `CallerLimits::max_resolved_arg_len`, the resolved
/// command string and each argv entry the args produce are bounded by it
/// (§5.1/§5.2 set no spec maximum — see the `ResolvedString` /
/// `ArgListItem` constraint docs).
fn validate_action_fs(
    action: &Action,
    symtab: &SymbolTable,
    ev: &FsEval,
    action_path: &[PathElement],
    max_resolved_arg_len: Option<usize>,
    errors: &mut ValidationErrors,
) {
    let command_constraint =
        max_resolved_arg_len.map(|max_len| ResolvedConstraint::ResolvedString { max_len });
    let arg_constraint =
        max_resolved_arg_len.map(|max_len| ResolvedConstraint::ArgListItem { max_len });
    validate_fs_with(
        &action.command,
        symtab,
        ev,
        &path_field(action_path, "command"),
        command_constraint.as_ref(),
        errors,
    );
    if let Some(args) = &action.args {
        let args_path = path_field(action_path, "args");
        for (j, arg) in args.iter().enumerate() {
            validate_fs_with(
                arg,
                symtab,
                ev,
                &path_index(&args_path, j),
                arg_constraint.as_ref(),
                errors,
            );
        }
    }
}

// ── Job-creation resolved-value checks ──────────────────────────────
//
// `create_job` carries the session/task-scope format strings — action
// `command`/`args`, environment `variables` values, embedded-file
// `data` — forward unresolved (only a worker host can bind `Session.*`
// / `Task.*` / `Env.File.*`), along with action `timeout` /
// `cancelation`, which are template scope but also resolve on the
// worker. Of the spec's three processing stages
// (Template Schemas §7.4: template validation, job creation, task
// execution on the worker host), job creation is the first at which
// `Param.*` / `RawParam.*` have real values. The functions below
// re-run exactly the resolved-value checks pass 8 applies to those
// fields (and to the `let` bindings feeding them), against symbol
// tables with the parameters bound: a violation
// pass 8 could only lower-bound becomes decidable at job creation, and
// fails at submission instead of on every worker.
//
// Both walk the *template* types (the fields are carried forward
// as-is), report at the same field paths as pass 8, and evaluate under
// the same caller budgets (`FsEval`).

/// Job-creation resolved-value checks for a step script: the body pass 8
/// checks ([`validate_step_script_body`]), in pass 8's order.
///
/// `symtab` is the task-scope check table: concrete `Param.*` /
/// `RawParam.*` / `Job.Name` / `Step.Name` / `let` bindings, with
/// `Unresolved` placeholders for `Session.*`, `Task.*`, and PATH
/// `Param.*`. `template_symtab` is the step's template-scope table
/// (concrete `Param.*` minus PATH, `RawParam.*`, `Job.Name`,
/// `Step.Name`, step-level `let` bindings) that `timeout` /
/// `cancelation` resolve against, as at pass 8.
pub(crate) fn check_carried_forward_step_script(
    script: &StepScript,
    symtab: &SymbolTable,
    template_symtab: &SymbolTable,
    ctx: &ValidationContext,
    script_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let f = FsCtx::new(ctx, None);
    validate_step_script_body(&f, script, symtab, template_symtab, script_path, errors);
}

/// Job-creation resolved-value checks for an environment (job-level or
/// step-level): the body pass 8 checks ([`validate_env_body`]), in pass 8's
/// order.
///
/// `symtab` is the session-scope check table: concrete `Param.*` /
/// `RawParam.*` / `Job.Name` (and `Step.Name` for step environments) /
/// `let` bindings, with `Unresolved` placeholders for `Session.*`,
/// `Env.File.*`, and PATH `Param.*`. `template_symtab` is the
/// template-scope table `timeout` / `cancelation` resolve against (the
/// job's table for a job environment, the step's for a step environment),
/// as at pass 8.
pub(crate) fn check_carried_forward_environment(
    env: &Environment,
    symtab: &SymbolTable,
    template_symtab: &SymbolTable,
    ctx: &ValidationContext,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let f = FsCtx::new(ctx, None);
    validate_env_body(&f, env, symtab, template_symtab, false, path, errors);
}

/// Each embedded file's `data` in `symtab` against the opt-in
/// `CallerLimits::max_resolved_data_len` cap (`filename` is a plain string
/// per the 2023-09 schema: no format-string check), and, with
/// `reject_complex`, its use of more than a bare reference.
fn check_embedded_files_data(
    f: &FsCtx<'_>,
    files: Option<&[EmbeddedFile]>,
    symtab: &SymbolTable,
    reject_complex: bool,
    script_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let files_path = path_field(script_path, "embeddedFiles");
    let data_constraint = f
        .ctx
        .caller_limits
        .max_resolved_data_len
        .map(|max_len| ResolvedConstraint::ResolvedString { max_len });
    for (j, file) in files.into_iter().flatten().enumerate() {
        let Some(data) = &file.data else { continue };
        let data_path = path_field(&path_index(&files_path, j), "data");
        if reject_complex && data.has_complex_expressions() {
            errors.add(
                &data_path,
                "complex expressions require the EXPR extension.",
            );
        }
        validate_fs_with(
            data,
            symtab,
            &f.host,
            &data_path,
            data_constraint.as_ref(),
            errors,
        );
    }
}

/// The standard capability tables a `hostRequirements` check needs
/// (§3.3.1.1, §3.3.2.1, §3.3.2.2), for the document's profile.
struct HostRequirementsStandards {
    amounts: &'static [&'static str],
    attribute_names: Vec<&'static str>,
    attributes: &'static [(&'static str, &'static [&'static str])],
}

/// Pass-8 checks of a Step's `hostRequirements` (§3.3) at `hr_path`,
/// against `hr_symtab`, the scope its format strings resolve in: the
/// template scope, `Job.Name` and `Step.Name` with `EXPR`, and the Step's
/// `let` bindings.
fn validate_host_requirements_fs(
    f: &FsCtx<'_>,
    hr: &HostRequirements,
    hr_symtab: &SymbolTable,
    hr_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let template_ev = &f.template;
    let standard_amounts = f.standards.amounts;
    let standard_attr_names = &f.standards.attribute_names;
    let standard_attrs = f.standards.attributes;
    // §3.3.1.1 / §3.3.2.1: a capability name is `@fmtstring`, and its
    // constraints apply to the resolved name. The name's
    // expressions are checked against the symbols available when it
    // is resolved at job creation, and the `CapabilityName`
    // constraint runs the full name check when the name is fully
    // static. Structure checks literal names, and job creation
    // checks the resolved name.
    if let Some(amounts) = &hr.amounts {
        let amounts_path = path_field(hr_path, "amounts");
        let name_constraint = ResolvedConstraint::CapabilityName {
            kind: helpers::CapabilityKind::Amount,
            standard: standard_amounts,
        };
        let mut known_names = Vec::with_capacity(amounts.len());
        for (j, amt) in amounts.iter().enumerate() {
            let amt_path = path_index(&amounts_path, j);
            let sr = validate_fs_with(
                &amt.name,
                hr_symtab,
                template_ev,
                &path_field(&amt_path, "name"),
                Some(&name_constraint),
                errors,
            );
            known_names.push(known_capability_name(&amt.name, sr.as_ref()));
            for (field, bound, positive, msg) in [
                ("min", &amt.min, false, "must be non-negative."),
                ("max", &amt.max, true, "must be positive."),
            ] {
                if let Some(bound) = bound {
                    validate_fs_with(
                        bound,
                        hr_symtab,
                        template_ev,
                        &path_field(&amt_path, field),
                        Some(&ResolvedConstraint::Float { positive, msg }),
                        errors,
                    );
                }
            }
        }
        check_known_names_unique(
            &known_names,
            helpers::CapabilityKind::Amount,
            &amounts_path,
            errors,
        );
    }
    if let Some(attrs) = &hr.attributes {
        let attrs_path = path_field(hr_path, "attributes");
        let name_constraint = ResolvedConstraint::CapabilityName {
            kind: helpers::CapabilityKind::Attribute,
            standard: standard_attr_names,
        };
        let mut known_names = Vec::with_capacity(attrs.len());
        for (j, attr) in attrs.iter().enumerate() {
            let attr_path = path_index(&attrs_path, j);
            let sr = validate_fs_with(
                &attr.name,
                hr_symtab,
                template_ev,
                &path_field(&attr_path, "name"),
                Some(&name_constraint),
                errors,
            );
            let known = known_capability_name(&attr.name, sr.as_ref());
            // The values are checked against the name when it is
            // known here, and against the resolved name at job
            // creation otherwise.
            let capability_name = known.as_ref().map_or("", |k| k.name.as_str());
            if let Some(k) = known.as_ref().filter(|k| !k.literal) {
                check_values_for_static_name(&k.name, attr, standard_attrs, &attr_path, errors);
            }
            let attr_constraint = ResolvedConstraint::AttributeValue {
                capability_name,
                standard: standard_attrs,
            };
            for (field, vals) in [("anyOf", &attr.any_of), ("allOf", &attr.all_of)] {
                let field_path = path_field(&attr_path, field);
                for (k, v) in vals.iter().flatten().enumerate() {
                    validate_fs_with(
                        v,
                        hr_symtab,
                        template_ev,
                        &path_index(&field_path, k),
                        Some(&attr_constraint),
                        errors,
                    );
                }
            }
            known_names.push(known);
        }
        check_known_names_unique(
            &known_names,
            helpers::CapabilityKind::Attribute,
            &attrs_path,
            errors,
        );
    }
}

/// Pass 8 for a Job Template: its `name`; the `let` bindings and
/// `hostRequirements` of each Step that has `hostRequirements`; each Job
/// Environment ([`validate_environment`]); each
/// Step's other fields ([`validate_step`]); and, with `EXPR`, the
/// comprehension-variable rules of every Environment's script.
pub fn validate_format_strings(
    jt: &JobTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let f = FsCtx {
        // `create_job` takes this from the template too, since its context
        // may enable more extensions than the template declares.
        template_expr: jt.profile().has_extension(ModelExtension::Expr),
        ..FsCtx::new(ctx, jt.parameter_definitions.as_deref())
    };
    validate_fs_with(
        &jt.name,
        &f.template_scope(),
        &f.template,
        &path_field(&[], "name"),
        Some(&ResolvedConstraint::Text {
            max_len: f.limits.max_job_name_len,
            forbid_control_chars: true,
            forbid_empty: true,
        }),
        errors,
    );
    // The `let` bindings and `hostRequirements` of each Step that has
    // `hostRequirements`. A clean scope's bindings are kept for the Step's
    // ranges and body while all kept bindings stay within
    // `RETAINED_SCOPE_BUDGET`; past it, those sites evaluate the bindings
    // again.
    let mut retained = std::collections::VecDeque::new();
    let mut retained_bytes = RetainedBindings::DEQUE_MIN_BYTES;
    for (i, step) in jt.steps.iter().enumerate() {
        let Some(hr) = &step.host_requirements else {
            continue;
        };
        let step_path = path_index(&path_field(&[], "steps"), i);
        let before = errors.len();
        let scope = f.step_let_scope(step, &step_path, errors);
        let clean = errors.len() == before;
        let hr_path = path_field(&step_path, "hostRequirements");
        validate_host_requirements_fs(&f, hr, &scope.symtab, &hr_path, errors);
        if let Some(kept) = clean
            .then(|| RetainedBindings::from_scope(i, scope))
            .flatten()
        {
            let bytes = kept.bytes();
            if retained_bytes + bytes <= RETAINED_SCOPE_BUDGET {
                retained_bytes += bytes;
                retained.push_back(kept);
            }
        }
    }
    let job_scope = f.job_scope();
    let envs_path = path_field(&[], "jobEnvironments");
    for (i, env) in jt.job_environments.iter().flatten().enumerate() {
        validate_environment(&f, env, &job_scope, &path_index(&envs_path, i), errors);
    }
    for (i, step) in jt.steps.iter().enumerate() {
        let step_path = path_index(&path_field(&[], "steps"), i);
        let kept = match retained.front() {
            Some(kept) if kept.step == i => retained.pop_front(),
            _ => None,
        };
        validate_step(&f, step, kept, &step_path, errors);
    }
    if f.expr {
        let envs = jt
            .steps
            .iter()
            .flat_map(|s| s.step_environments.iter().flatten());
        for env in jt.job_environments.iter().flatten().chain(envs) {
            validate_env_comprehensions(env, errors);
        }
    }
}

/// Pass-8 checks of a Job Environment or an Environment Template's
/// `environment` at `env_path`: its script's `let` bindings and body
/// ([`validate_env_body`]) in its session scope, its `timeout`s in
/// `job_scope`.
fn validate_environment(
    f: &FsCtx<'_>,
    env: &Environment,
    job_scope: &SymbolTable,
    env_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let mut symtab = f.session_scope(env, None);
    f.validate_lets(
        env.script.as_ref().and_then(|s| s.let_bindings.as_deref()),
        &path_field(&path_field(env_path, "script"), "let"),
        &HashSet::new(),
        &mut symtab,
        &f.host,
        errors,
    );
    validate_env_body(f, env, &symtab, job_scope, !f.expr, env_path, errors);
}

/// Pass-8 checks of a Step after its `hostRequirements`: its
/// `parameterSpace`, its `let` bindings, its script or SimpleAction (§8)
/// in its task scope, and its Step Environments. `kept` holds the Step's
/// clean `let` values from its `hostRequirements`, if they were kept.
fn validate_step(
    f: &FsCtx<'_>,
    step: &StepTemplate,
    kept: Option<RetainedBindings>,
    step_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    // The ranges and the body share one evaluation of the Step's `let`
    // bindings when it is clean.
    let mut shared = kept.map(|kept| kept.into_scope(f.step_base_scope()));
    if let Some(ps) = &step.parameter_space {
        let ps_path = path_field(step_path, "parameterSpace");
        // Shared only when evaluating them reported nothing (also when it
        // did not run: no EXPR, or no bindings).
        if shared.is_none() {
            let mut let_errors = ValidationErrors::default();
            let scope = f.step_let_scope(step, step_path, &mut let_errors);
            shared = let_errors.is_empty().then_some(scope);
        }
        match &shared {
            Some(scope) => validate_parameter_space(f, ps, &scope.symtab, &ps_path, errors),
            None => {
                let range_scope = f.range_scope_for_unclean_lets(step, step_path, errors);
                validate_parameter_space(f, ps, &range_scope, &ps_path, errors);
            }
        }
    }
    let StepLetScope {
        symtab: step_scope,
        names,
    } = match shared {
        Some(scope) => scope,
        None => f.step_let_scope(step, step_path, errors),
    };
    drop(names);
    let step_scope = &step_scope;
    if step.let_bindings.is_some() && !f.expr {
        errors.add(
            &path_field(step_path, "let"),
            "'let' requires the EXPR extension.",
        );
    }
    let step_lets = step.let_bindings.as_deref();
    let mut task_scope = f.task_scope(step, step_scope);
    if let Some(script) = &step.script {
        let script_path = path_field(step_path, "script");
        validate_step_script(
            f,
            script,
            &script_path,
            step_lets,
            &mut task_scope,
            step_scope,
            errors,
        );
    } else if f.ctx.profile.has_extension(ModelExtension::FeatureBundle1) {
        // SimpleAction fields (§8, FEATURE_BUNDLE_1): validate the desugared
        // script with exactly the same code, into a scratch collection
        // rooted at the synthesized `script` node, then re-root every error
        // onto the field the author wrote (`steps[i] -> bash -> script`,
        // `-> args[k]`, `-> timeout`, ...). Each field gets its own copy of
        // the task scope so its `let` bindings do not leak into a sibling.
        // Skipped when the extension is off (pass 7 rejects the field) and
        // when the step also has `script` (pass 7 rejects the combination,
        // and the task scope's `Task.File.*` come from the authored
        // script).
        for (kind, sa) in step.simple_actions() {
            let sa_path = path_field(step_path, kind.field_name());
            let mut symtab = task_scope.clone();
            let mut scratch = ValidationErrors::default();
            let script = kind.desugar(&step.name, sa);
            validate_step_script(
                f,
                &script,
                &[],
                step_lets,
                &mut symtab,
                step_scope,
                &mut scratch,
            );
            errors.extend_remapped(scratch, |rel| {
                let mut p = sa_path.clone();
                p.extend(kind.remap_desugared_path(rel));
                p
            });
        }
    }
    let envs_path = path_field(step_path, "stepEnvironments");
    for (j, env) in step.step_environments.iter().flatten().enumerate() {
        let env_path = path_index(&envs_path, j);
        // The Step's `let` values are read from the task scope, after its
        // script's own `let` bindings.
        let mut symtab = f.session_scope(env, Some((step, &task_scope)));
        if let Some(bindings) = env.script.as_ref().and_then(|s| s.let_bindings.as_deref()) {
            f.validate_lets(
                Some(bindings),
                &path_field(&path_field(&env_path, "script"), "let"),
                &let_names(step_lets),
                &mut symtab,
                &f.host,
                errors,
            );
        }
        validate_env_body(f, env, &symtab, step_scope, !f.expr, &env_path, errors);
    }
}

/// Pass-8 checks of a Step's `parameterSpace` at `ps_path`: the format
/// strings of each task parameter's `range` (Template Schemas §3.4) and, for
/// CHUNK[INT], `chunks`, in `step_scope`, the Step's template scope.
fn validate_parameter_space(
    f: &FsCtx<'_>,
    ps: &StepParameterSpaceDefinition,
    step_scope: &SymbolTable,
    ps_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let tpd_path = path_field(ps_path, "taskParameterDefinitions");
    // INT, CHUNK[INT] and FLOAT elements are typed with the target job
    // creation resolves them with, which depends on whether the template
    // itself enables EXPR (see `TaskParameterType::range_list_item_target`).
    for (j, tp) in ps.task_parameter_definitions.iter().enumerate() {
        let range_path = path_field(&path_index(&tpd_path, j), "range");
        let numeric_item = tp
            .task_param_type()
            .range_list_item_target(f.template_expr)
            .map(|target| ResolvedConstraint::NumericListItem { target });
        // §3.4.2: a STRING or PATH range element is at most 1024 characters
        // once resolved. As a list item it may also resolve to `null`
        // (skipped) or a list (flattened), Expression Language §1.3.2. The
        // §3.4.2 minimum of 1 is enforced for PATH only, matching the
        // reference implementation (see the TextListItem doc).
        let string_item = |forbid_empty| ResolvedConstraint::TextListItem {
            max_len: f.limits.max_task_param_string_len,
            forbid_empty,
        };
        let check_items = |items: &[FormatString],
                           constraint: &ResolvedConstraint<'_>,
                           errors: &mut ValidationErrors| {
            for (k, item) in items.iter().enumerate() {
                let path = path_index(&range_path, k);
                validate_fs_with(
                    item,
                    step_scope,
                    &f.template,
                    &path,
                    Some(constraint),
                    errors,
                );
            }
        };
        match tp {
            TaskParameterDefinition::INT(t) => {
                validate_int_range_fs(
                    &t.range,
                    numeric_item.as_ref(),
                    step_scope,
                    &f.template,
                    &range_path,
                    errors,
                );
            }
            TaskParameterDefinition::FLOAT(t) => {
                // §3.4.1.2: a format string element is checked with the
                // target `resolve_float_range` resolves it with. Literal
                // elements are skipped by `validate_fs_with`.
                if let crate::template::FloatRange::List(items) = &t.range {
                    for (k, item) in items.iter().enumerate() {
                        if let crate::template::FloatRangeItem::FormatString(fs) = item {
                            let path = path_index(&range_path, k);
                            validate_fs_with(
                                fs,
                                step_scope,
                                &f.template,
                                &path,
                                numeric_item.as_ref(),
                                errors,
                            );
                        }
                    }
                }
            }
            TaskParameterDefinition::STRING(t) => {
                if let crate::template::StringRange::List(items) = &t.range {
                    check_items(items, &string_item(false), errors);
                }
            }
            TaskParameterDefinition::PATH(t) => {
                if let crate::template::StringRange::List(items) = &t.range {
                    check_items(items, &string_item(true), errors);
                }
            }
            TaskParameterDefinition::CHUNK_INT(t) => {
                validate_int_range_fs(
                    &t.range,
                    numeric_item.as_ref(),
                    step_scope,
                    &f.template,
                    &range_path,
                    errors,
                );
                // TASK_CHUNKING chunks fields are `<intstring>` format
                // strings resolved at job creation; their literal forms are
                // checked in the task-chunking pass, the format-string forms
                // here.
                let chunks_path = path_field(&path_index(&tpd_path, j), "chunks");
                if let crate::template::IntOrFormatString::FormatString(fs) =
                    &t.chunks.default_task_count
                {
                    validate_fs_with(
                        fs,
                        step_scope,
                        &f.template,
                        &path_field(&chunks_path, "defaultTaskCount"),
                        Some(&ResolvedConstraint::Int {
                            min: 1,
                            min_msg: "defaultTaskCount must be >= 1.",
                            max: None,
                            parse_msg: "defaultTaskCount must be an integer.",
                            nullable: false,
                        }),
                        errors,
                    );
                }
                if let Some(crate::template::IntOrFormatString::FormatString(fs)) =
                    &t.chunks.target_runtime_seconds
                {
                    validate_fs_with(
                        fs,
                        step_scope,
                        &f.template,
                        &path_field(&chunks_path, "targetRuntimeSeconds"),
                        Some(&ResolvedConstraint::Int {
                            min: 0,
                            min_msg: "targetRuntimeSeconds must be >= 0.",
                            max: None,
                            parse_msg: "targetRuntimeSeconds must be an integer.",
                            nullable: true,
                        }),
                        errors,
                    );
                }
            }
        }
    }
}

/// Format string validation for a standalone environment template.
///
/// Scope selection is the one for a Job Environment
/// ([`validate_environment`]), with the template's own
/// `parameterDefinitions` as the Job Parameters: the body is validated in
/// session scope, and action `timeout` and `notifyPeriodInSeconds` (plain
/// `@fmtstring`, resolved at job creation) in template scope with `Job.*`.
/// `Step.Name` is in neither: an environment template is not attached to a
/// step.
pub fn validate_format_strings_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
    errors: &mut ValidationErrors,
) {
    let f = FsCtx::new(ctx, et.parameter_definitions.as_deref());
    let env_path = path_field(&[], "environment");
    validate_environment(&f, &et.environment, &f.job_scope(), &env_path, errors);
    if f.expr {
        validate_env_comprehensions(&et.environment, errors);
    }
}

/// The format strings of an Environment's body at `path`: its `variables`
/// values and every action's `command` and `args` in `symtab`, its session
/// scope; every action's `timeout` and `cancelation` in `template_symtab`
/// (the wrap hooks with their `WrappedAction.*` scope); each embedded
/// file's `data` in `symtab`. With `reject_complex` (pass 8, without
/// `EXPR`), anything beyond a bare `{{Name.Path}}` reference is an error.
/// Pass 8 and job creation's re-check ([`check_carried_forward_environment`])
/// both run it.
fn validate_env_body(
    f: &FsCtx<'_>,
    env: &Environment,
    symtab: &SymbolTable,
    template_symtab: &SymbolTable,
    reject_complex: bool,
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let vars_path = path_field(path, "variables");
    for (name, value) in env.variables.iter().flatten() {
        let var_path = path_field(&vars_path, name);
        if reject_complex && value.has_complex_expressions() {
            errors.add(&var_path, "complex expressions require the EXPR extension.");
        }
        // §4.4.2: an environment variable value is at most
        // `max_env_var_value_len` (2048) characters. The value is
        // `@fmtstring[host]`, so the limit describes the resolved value; the
        // raw-text pass checks literal values against the same limit.
        validate_fs_with(
            value,
            symtab,
            &f.host,
            &var_path,
            Some(&ResolvedConstraint::Text {
                max_len: f.limits.max_env_var_value_len,
                forbid_control_chars: false,
                forbid_empty: false,
            }),
            errors,
        );
    }
    let Some(script) = &env.script else { return };
    let script_path = path_field(path, "script");
    let actions_path = path_field(&script_path, "actions");
    if reject_complex {
        for (name, action) in script.actions.iter_named() {
            reject_complex_action(action, &path_field(&actions_path, name), errors);
        }
    }
    let max_arg_len = f.ctx.caller_limits.max_resolved_arg_len;
    if let Some(action) = &script.actions.on_enter {
        let action_path = path_field(&actions_path, "onEnter");
        validate_action_fs(action, symtab, &f.host, &action_path, max_arg_len, errors);
    }
    // RFC 0008: all three wrap hooks see `WrappedAction.*`. `onWrapEnvEnter`
    // and `onWrapEnvExit` also see `WrappedEnv.Name`, and `onWrapTaskRun`
    // `WrappedStep.Name`. A reference outside the hooks that have it is an
    // undefined variable.
    for (hook_name, action_opt, extra) in script.actions.wrap_hooks() {
        if let Some(action) = action_opt {
            let mut st = symtab.clone();
            add_wrapped_action_scope(&mut st);
            match extra {
                WrapHookScope::EnvName => add_wrapped_env_name_scope(&mut st),
                WrapHookScope::StepName => add_wrapped_step_name_scope(&mut st),
            }
            let action_path = path_field(&actions_path, hook_name);
            validate_action_fs(action, &st, &f.host, &action_path, max_arg_len, errors);
        }
    }
    if let Some(action) = &script.actions.on_exit {
        let action_path = path_field(&actions_path, "onExit");
        validate_action_fs(action, symtab, &f.host, &action_path, max_arg_len, errors);
    }
    validate_env_actions_timing_fs(script, template_symtab, &f.template, &actions_path, errors);
    check_embedded_files_data(
        f,
        script.embedded_files.as_deref(),
        symtab,
        reject_complex,
        &script_path,
        errors,
    );
}

/// Without `EXPR`, report an action's `command` and `args` that use more
/// than a bare `{{Name.Path}}` reference.
fn reject_complex_action(
    action: &Action,
    action_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if action.command.has_complex_expressions() {
        errors.add(
            &path_field(action_path, "command"),
            "complex expressions require the EXPR extension.",
        );
    }
    let args_path = path_field(action_path, "args");
    for (j, arg) in action.args.iter().flatten().enumerate() {
        if arg.has_complex_expressions() {
            errors.add(
                &path_index(&args_path, j),
                "complex expressions require the EXPR extension.",
            );
        }
    }
}

/// Augment a session-scope symtab with `WrappedAction.*`, available in
/// all three wrap hooks (RFC 0008):
///
/// - `WrappedAction.Command` — string
/// - `WrappedAction.Args` — list[string]
/// - `WrappedAction.Environment` — list[string] (entries of the form `"KEY=value"`)
/// - `WrappedAction.Timeout` — int? (seconds, or `null` when the wrapped
///   action specified no timeout)
/// - `WrappedAction.Cancelation.Mode` — string? (`"TERMINATE"`,
///   `"NOTIFY_THEN_TERMINATE"`, or `null` when the wrapped action defines
///   no `<Cancelation>`)
/// - `WrappedAction.Cancelation.NotifyPeriodInSeconds` — int? (the
///   effective grace period when the mode is `NOTIFY_THEN_TERMINATE`;
///   `null` for `TERMINATE` or when no `<Cancelation>` is defined)
///
/// The caller has already cloned the session symtab, so we mutate in place.
fn add_wrapped_action_scope(symtab: &mut SymbolTable) {
    for (name, ty) in [
        ("WrappedAction.Command", ExprType::STRING),
        (
            "WrappedAction.Args",
            fixed_type(ExprType::list(ExprType::STRING)),
        ),
        (
            "WrappedAction.Environment",
            fixed_type(ExprType::list(ExprType::STRING)),
        ),
        (
            "WrappedAction.Timeout",
            fixed_type(ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE])),
        ),
        (
            "WrappedAction.Cancelation.Mode",
            fixed_type(ExprType::union(vec![ExprType::STRING, ExprType::NULLTYPE])),
        ),
        (
            "WrappedAction.Cancelation.NotifyPeriodInSeconds",
            fixed_type(ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE])),
        ),
    ] {
        symtab.set(name, ExprValue::unresolved(ty)).expect("symtab");
    }
}

/// Augment with `WrappedEnv.Name`, available only in `onWrapEnvEnter` and
/// `onWrapEnvExit` (RFC 0008).
fn add_wrapped_env_name_scope(symtab: &mut SymbolTable) {
    symtab
        .set("WrappedEnv.Name", ExprValue::unresolved(ExprType::STRING))
        .expect("symtab");
}

/// Augment with `WrappedStep.Name`, available only in `onWrapTaskRun`
/// (RFC 0008).
fn add_wrapped_step_name_scope(symtab: &mut SymbolTable) {
    symtab
        .set("WrappedStep.Name", ExprValue::unresolved(ExprType::STRING))
        .expect("symtab");
}

/// The comprehension-variable rules for an Environment's script against
/// its `let` names.
fn validate_env_comprehensions(env: &Environment, errors: &mut ValidationErrors) {
    let Some(script) = &env.script else { return };
    let env_let_names = let_names(script.let_bindings.as_deref());
    if env_let_names.is_empty() {
        return;
    }
    let path: Vec<PathElement> = vec![];
    for action in script.actions.iter_actions() {
        if let Err(e) = action.command.validate_comprehension_vars(&env_let_names) {
            errors.add(&path, e.to_string());
        }
        for arg in action.args.iter().flatten() {
            if let Err(e) = arg.validate_comprehension_vars(&env_let_names) {
                errors.add(&path, e.to_string());
            }
        }
    }
}

fn validate_let_bindings(
    bindings: &[String],
    path: &[PathElement],
    enclosing_names: &HashSet<String>,
    symtab: &mut SymbolTable,
    ev: &FsEval,
    errors: &mut ValidationErrors,
) {
    if bindings.is_empty() {
        errors.add(path, "if provided, must not be empty.");
        return;
    }
    if bindings.len() > 50 {
        errors.add(path, "must not contain more than 50 bindings.");
    }
    let mut names = HashSet::new();
    for (i, binding) in bindings.iter().enumerate() {
        let b_path = path_index(path, i);
        let Some(eq_pos) = binding.find('=') else {
            errors.add(&b_path, format!("missing '=' in '{binding}'."));
            continue;
        };
        let name = binding[..eq_pos].trim();
        let expr = binding[eq_pos + 1..].trim();
        if name.is_empty() {
            errors.add(&b_path, "has empty name.");
            continue;
        }
        if expr.is_empty() {
            errors.add(
                &b_path,
                format!("binding '{name}' has no expression after '='."),
            );
            continue;
        }
        let first = name.chars().next().unwrap();
        if !first.is_ascii_lowercase() && first != '_' {
            errors.add(
                &b_path,
                format!("name '{name}' must start with lowercase letter or underscore."),
            );
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            errors.add(
                &b_path,
                format!("name '{name}' contains invalid characters."),
            );
        }
        // Characters, not bytes: §3.6.1 caps characters, and the charset check
        // above does not `continue`, so a multi-byte name would otherwise draw a
        // spurious length error alongside the real one.
        if name.chars().count() > MAX_LET_IDENTIFIER_LEN {
            errors.add(
                &b_path,
                format!("name '{name}' exceeds {MAX_LET_IDENTIFIER_LEN} characters."),
            );
        }
        if !names.insert(name.to_string()) {
            errors.add(&b_path, format!("duplicate name '{name}'."));
        }
        if enclosing_names.contains(name) {
            errors.add(&b_path, format!("'{name}' shadows enclosing scope."));
        }
        evaluate_let_binding(binding, eq_pos, &b_path, true, symtab, ev, errors);
    }
}

/// Parse and evaluate one well-formed `name = expr` binding (`eq_pos` is
/// the `=`) into `symtab`, reporting a failure at `b_path` as
/// `Invalid expression in let binding '<name>': <error>` with the caret
/// aligned to the full binding string (matching Python and the run-time
/// `evaluate_let_bindings`). A failed binding is bound as
/// `Unresolved(ANY)` so later bindings and format strings in the scope
/// still evaluate instead of cascading "Undefined variable" errors.
///
/// `check_self_reference` adds pass 8's structural self-reference check
/// (the job-creation re-check leaves structure to template validation).
fn evaluate_let_binding(
    binding: &str,
    eq_pos: usize,
    b_path: &[PathElement],
    check_self_reference: bool,
    symtab: &mut SymbolTable,
    ev: &FsEval,
    errors: &mut ValidationErrors,
) {
    let name = binding[..eq_pos].trim();
    let expr = binding[eq_pos + 1..].trim();
    // The prefix is included in error messages so caret positions align with
    // the full binding string, matching Python's behavior.
    let expr_start =
        eq_pos + 1 + binding[eq_pos + 1..].len() - binding[eq_pos + 1..].trim_start().len();
    let prefix = &binding[..expr_start];
    let report = |e: &openjd_expr::ExpressionError, errors: &mut ValidationErrors| {
        errors.add(
            b_path,
            format!(
                "Invalid expression in let binding '{name}': {}",
                e.message_with_expr_prefix(prefix)
            ),
        );
    };
    match ParsedExpression::with_profile(expr, &ev.profile) {
        Ok(parsed) => {
            // Check self-reference using the parsed AST's accessed symbols
            // rather than heuristic regex matching on the raw expression string.
            if check_self_reference && parsed.accessed_symbols().contains(name) {
                errors.add(b_path, format!("'{name}' references itself."));
            }
            match ev
                .budgeted(parsed.with_library(&ev.lib))
                .evaluate(&[symtab as &SymbolTable])
            {
                Ok(result) => {
                    // Set the binding in the symtab with its inferred value/type
                    // so subsequent bindings and format strings see the correct type.
                    // `set` fails only when the key path collides with an existing
                    // scalar or table, as a binding named like a scope root
                    // (`Session`) does. Template validation reports the name, so
                    // the binding is dropped and the scope keeps its symbol.
                    let _ = symtab.set(name, result);
                }
                Err(e) => {
                    report(&e, errors);
                    // Still add as unresolved(ANY) so later bindings don't cascade errors
                    let _ = symtab.set(name, ExprValue::unresolved(ExprType::ANY));
                }
            }
        }
        Err(e) => {
            report(&e, errors);
            let _ = symtab.set(name, ExprValue::unresolved(ExprType::ANY));
        }
    }
}

/// Make every concrete value in `symtab` whose type contains `path`
/// unresolved, for a host-scope check made before the worker is known.
///
/// Those values come from TEMPLATE scope (step-level `let` bindings), where
/// paths are POSIX. The session converts them to the worker's path format
/// before host-scope fields use them (`SerializedSymbolTable::to_symtab`
/// with `PathFormat::host()`), so `C:\a\b` may have one component or
/// three there, and a host-scope check can no more fold them in POSIX than
/// it can a literal `path()` (specs/model/validation.md § Path Format).
pub(crate) fn unresolve_template_paths(symtab: &mut SymbolTable) {
    fn has_path(t: &ExprType) -> bool {
        t.code() == openjd_expr::TypeCode::Path || t.params().iter().any(has_path)
    }
    for name in symtab.all_paths("") {
        let ty = match symtab.get_value(&name) {
            Some(v) if !v.is_unresolved() => v.expr_type(),
            _ => continue,
        };
        if has_path(&ty) {
            symtab
                .set(&name, ExprValue::unresolved(ty))
                .expect("replacing an existing leaf value cannot conflict");
        }
    }
}

/// Job-creation evaluation of a `let` block into a check symbol table
/// (step-level bindings with `host_context = None`, script-level ones in
/// step scripts and environments with `Unresolved`): each binding is
/// parsed under the context's profile for `host_context` — the profile
/// pass 8 parsed it with — and evaluated under `PathFormat::Posix` with
/// the caller's budgets, through the same [`evaluate_let_binding`] pass 8
/// uses. A failure is reported at `let_path[j]` with pass 8's message
/// and caret, and the name is bound `Unresolved(ANY)` so the scope's
/// remaining bindings and checks still run (pass 8's continuation
/// policy).
///
/// Structural checks (names, duplicates, shadowing, count) are template
/// validation's job and are not repeated; a structurally malformed
/// binding — no `=`, empty name or expression, which decode rejects — is
/// skipped.
pub(crate) fn check_carried_forward_let_bindings(
    bindings: &[String],
    let_path: &[PathElement],
    symtab: &mut SymbolTable,
    ctx: &ValidationContext,
    host_context: openjd_expr::HostContext,
    errors: &mut ValidationErrors,
) {
    let ev = FsEval::new(ctx, host_context);
    for (j, binding) in bindings.iter().enumerate() {
        let Some(eq_pos) = binding.find('=') else {
            continue;
        };
        if binding[..eq_pos].trim().is_empty() || binding[eq_pos + 1..].trim().is_empty() {
            continue;
        }
        evaluate_let_binding(
            binding,
            eq_pos,
            &path_index(let_path, j),
            false,
            symtab,
            &ev,
            errors,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The charge for one kept binding covers its deque slots, its entry in
    /// the bindings vector, its name, and its value.
    #[test]
    fn retained_bindings_charge_every_allocation() {
        let mut symtab = SymbolTable::new();
        symtab.set("ab", ExprValue::String("xyz".into())).unwrap();
        let scope = StepLetScope {
            symtab,
            names: HashSet::from(["ab".to_string()]),
        };
        let kept = RetainedBindings::from_scope(0, scope).unwrap();
        assert_eq!(
            kept.bytes(),
            2 * std::mem::size_of::<RetainedBindings>()
                + std::mem::size_of::<(String, ExprValue)>()
                + 2
                + 3
        );
    }
}
