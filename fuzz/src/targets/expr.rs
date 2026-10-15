// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-expr targets.
//!
//! Beyond "never panics", these encode the bug classes the project's fix
//! history shows recur:
//!
//! - **Type soundness** (the largest class: #300, #316, #329, #396, #434,
//!   2e6e1a4, 9bc8c88, ...). The same expression is evaluated statically,
//!   with every symbol `Unresolved(T)` under the validation-time library,
//!   and concretely with values of type `T`. A statically accepted
//!   expression must produce a value of the static type and must not fail
//!   at run time with a name/function/type error; a generated (well-typed
//!   by construction) expression must not be rejected statically when it
//!   evaluates fine.
//! - **Budget enforcement** (#272, #284, #410, #417, #418, `zfill('5', -1)`):
//!   peak heap use during evaluation stays within a constant factor of the
//!   memory limit, and reported metrics stay within their limits.
//! - **Round trips** (f0b7040, #354, report Rec 9): transport encoding,
//!   `RangeExpr` display/parse, `ExprType` display/parse, symbol table
//!   serialization.
//! - **Reference models** for `RangeExpr` (`len`/`get`/`iter`/`contains`/
//!   `slice` vs a plain `Vec`) and `IntRange` across the whole `i64` domain.
//! - **Signature/dispatch agreement**: a library call that succeeds must
//!   have a derivable return type that its value satisfies.

use crate::alloc::measure;
use crate::gen::expr::{validation_library, ExprGen, Symbol};
use crate::gen::value::{any_type, concrete_type, path_format, value, value_of};
use crate::gen::{self, one_in, pick, upto};
use crate::targets::{digest, text_and_control};
use crate::{known_issue, oracle};
use arbitrary::Unstructured;
use openjd_expr::path_mapping::{apply_rules_with_format, is_uri};
use openjd_expr::value::Float64;
use openjd_expr::{
    escape_format_string, EvalContext, ExprProfile, ExprType, ExprValue, ExpressionError,
    ExpressionErrorKind, FormatString, FormatStringOptions, FunctionLibrary, HostContext,
    ParsedExpression, PathFormat, PathMappingRule, RangeExpr, SerializedSymbolTable, SymbolTable,
    TypeCode,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

/// Memory limit for fuzzed evaluations: small enough that a missing budget
/// check shows up as a peak far above it.
const MEMORY_LIMIT: usize = 4 << 20;
/// Operation limit for fuzzed evaluations.
const OP_LIMIT: usize = 200_000;
/// Allowed peak heap during one evaluation. The evaluator's accounting is
/// approximate and transient copies are legitimate, so this is a generous
/// constant factor of the limit — an unchecked allocation bomb is orders of
/// magnitude past it.
const PEAK_ALLOWANCE: usize = 8 * MEMORY_LIMIT + (16 << 20);

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Errors that are legitimate at run time even after static acceptance:
/// they depend on concrete values, not types.
fn value_dependent(kind: &ExpressionErrorKind) -> bool {
    matches!(
        kind,
        ExpressionErrorKind::IntegerOverflow
            | ExpressionErrorKind::DivisionByZero { .. }
            | ExpressionErrorKind::FloatError { .. }
            | ExpressionErrorKind::IndexOutOfBounds { .. }
            | ExpressionErrorKind::MemoryLimitExceeded { .. }
            | ExpressionErrorKind::OperationLimitExceeded { .. }
            | ExpressionErrorKind::ExpressionTooDeep { .. }
            | ExpressionErrorKind::ExplicitFail(_)
            // `range_expr('a')`: a value that fails to parse as a range.
            | ExpressionErrorKind::ParseError(_)
            | ExpressionErrorKind::Other(_)
    )
}

/// Errors that a statically accepted expression must never raise at run
/// time: the static pass already resolved every name and function and
/// checked every operand type. (Value-dependent conversion failures such as
/// `int('x')` are `Other`, not `TypeError`.)
pub(crate) fn static_class(kind: &ExpressionErrorKind) -> bool {
    matches!(
        kind,
        ExpressionErrorKind::UndefinedVariable { .. }
            | ExpressionErrorKind::UnknownFunction { .. }
            | ExpressionErrorKind::UnsupportedSyntax { .. }
            | ExpressionErrorKind::TypeError { .. }
    )
}

/// The ordering operators' cross-type refusal is a run-time check only
/// (`do_compare`; specs/expr/function-library.md): known issue
/// `expr-ordering-runtime-type-check`.
pub(crate) fn is_ordering_type_error(e: &ExpressionError) -> bool {
    match e.kind() {
        ExpressionErrorKind::TypeError { message } => ["<", "<=", ">", ">="]
            .iter()
            .any(|op| message.starts_with(&format!("Cannot use '{op}' operator with "))),
        _ => false,
    }
}

/// `[f(x) for ...]` where `f` returns `T?`: the list is `list[T]` whenever
/// it builds, and a `null` element fails it at run time. Value-dependent,
/// though reported as a `TypeError` (list literals report the same case as
/// `Other`).
///
/// Likewise iterating an optional result (`re_search(...)` is `null` on no
/// match): `Cannot iterate over nulltype` depends on the value.
pub(crate) fn is_null_element_error(e: &ExpressionError) -> bool {
    matches!(e.kind(), ExpressionErrorKind::TypeError { message }
        if message == "Cannot create list from null elements"
            || message == "Cannot iterate over nulltype")
}

/// A list built from elements of different types fails at run time with
/// one of these (concatenation, comprehension, literal).
pub(crate) fn is_list_element_mix_error(e: &ExpressionError) -> bool {
    matches!(e.kind(), ExpressionErrorKind::TypeError { message }
        if message.starts_with("make_list expected ") || message.starts_with("Cannot concatenate "))
}

/// What, per known issue, may void static/run-time agreement for one
/// expression. Generated expressions report it from the generator (what it
/// actually emitted); text inputs have no generator to ask, so it is
/// inferred from the source.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    /// `and` / `or` (`expr-boolop-static-type`).
    pub boolop: bool,
    /// A union-typed argument or result (`expr-union-arg-overload`,
    /// `expr-flatten-union-static-type`).
    pub union: bool,
    /// A signature with an unbound return type variable
    /// (`expr-unbound-return-typevar`).
    pub unbound_return: bool,
    /// A list concatenation (`+`), whose `(list[T1], list[T2]) -> list[T3]`
    /// signature accepts mismatched element types.
    pub list_add: bool,
    /// A conditional (`expr-conditional-list-element-join`).
    pub conditional: bool,
}

impl Facts {
    pub fn from_gen(g: &ExprGen<'_, '_>) -> Self {
        Self {
            boolop: g.used_boolop,
            union: g.used_union,
            unbound_return: g.used_unbound_return,
            // List `+` is `(list[T1], list[T2]) -> list[T3]`, an
            // unbound-return signature.
            list_add: g.used_unbound_return,
            conditional: g.used_conditional,
        }
    }

    fn from_text(src: &str) -> Self {
        let has = |fs: &[&str]| fs.iter().any(|f| src.contains(f));
        Self {
            boolop: uses_boolop(src),
            union: (src.contains(" if ") && src.contains("[]"))
                || has(&[
                    "::",
                    "re_findall",
                    "re_match",
                    "re_search",
                    "round",
                    "**",
                    "flatten",
                ]),
            unbound_return: src.contains(" + ") && has(&["min(", "max(", "sum("]),
            list_add: src.contains('+'),
            conditional: src.contains(" if "),
        }
    }
}

/// A statically accepted `src` failed at run time with `e`. Fails the
/// oracle when the static pass should have rejected it, unless the
/// disagreement is a known issue (`unbound_return`: the generator went
/// through a signature with an unbound return type variable).
pub(crate) fn check_static_miss(e: &ExpressionError, facts: Facts, msg: impl FnOnce() -> String) {
    if !static_class(e.kind()) || is_null_element_error(e) {
        return;
    }
    let is_type_error = matches!(e.kind(), ExpressionErrorKind::TypeError { .. });
    if is_ordering_type_error(e) {
        crate::known::hit("expr-ordering-runtime-type-check", msg);
    } else if facts.boolop {
        crate::known::hit("expr-boolop-static-type", msg);
    } else if (facts.unbound_return && is_type_error)
        || (is_list_element_mix_error(e) && facts.list_add)
    {
        crate::known::hit("expr-unbound-return-typevar", msg);
    } else if is_list_element_mix_error(e) && facts.conditional {
        crate::known::hit("expr-conditional-list-element-join", msg);
    } else {
        oracle!(false, "{}", msg());
    }
}

/// The evaluation environment: each symbol with a concrete value and the
/// matching `Unresolved(T)` placeholder.
pub struct Env {
    pub symbols: Vec<Symbol>,
    /// Concrete values in the path format the environment was built for.
    pub concrete: SymbolTable,
    pub unresolved: SymbolTable,
    pub rules: Vec<PathMappingRule>,
}

/// `v` with every PATH re-created in `fmt` — what the session does at the
/// template → host boundary. Evaluating a PATH in a format other than its
/// own is a (correct) "path format mismatch" error, so each evaluator
/// format gets its own table.
fn in_format(v: &ExprValue, fmt: PathFormat) -> ExprValue {
    match v {
        ExprValue::Path { value, format, .. } if *format != PathFormat::Uri => {
            ExprValue::new_path(value.clone(), fmt)
        }
        _ if v.is_list() && v.expr_type().to_string().contains("path") => {
            let elems: Vec<ExprValue> = v
                .list_elements()
                .unwrap_or_default()
                .iter()
                .map(|e| in_format(e, fmt))
                .collect();
            let hint = v.list_elem_type().unwrap_or(ExprType::PATH);
            ExprValue::make_list(elems, hint).unwrap_or_else(|_| v.clone())
        }
        _ => v.clone(),
    }
}

impl Env {
    /// The concrete table with PATH values in `fmt`.
    pub fn concrete_in(&self, fmt: PathFormat) -> SymbolTable {
        let mut st = SymbolTable::new();
        for s in &self.symbols {
            if let Some(v) = self.concrete.get_value(&s.name) {
                let _ = st.set(&s.name, in_format(v, fmt));
            }
        }
        st
    }

    /// Bind each symbol to a fresh value of its type. Symbols that conflict
    /// with an earlier one (`A` vs `A.B`) are dropped.
    pub fn new(u: &mut Unstructured<'_>, symbols: Vec<Symbol>, fmt: PathFormat) -> Env {
        let mut concrete = SymbolTable::new();
        let mut unresolved = SymbolTable::new();
        let mut kept = Vec::new();
        for s in symbols {
            let v = value_of(u, &s.ty, fmt);
            if concrete.set(&s.name, v).is_ok() {
                let _ = unresolved.set(&s.name, ExprValue::unresolved(s.ty.clone()));
                kept.push(s);
            }
        }
        let rules = path_rules(u);
        Env {
            symbols: kept,
            concrete,
            unresolved,
            rules,
        }
    }
}

/// Path-mapping rules for `apply_path_mapping`.
pub fn path_rules(u: &mut Unstructured<'_>) -> Vec<PathMappingRule> {
    (0..upto(u, 2))
        .map(|_| PathMappingRule {
            source_path_format: *pick(u, &[PathFormat::Posix, PathFormat::Windows]),
            source_path: gen::path_string(u),
            destination_path: gen::path_string(u),
        })
        .collect()
}

fn runtime_library(rules: &[PathMappingRule]) -> Arc<FunctionLibrary> {
    FunctionLibrary::for_profile(
        &ExprProfile::latest().with_host_context(HostContext::with_rules(rules.to_vec())),
    )
}

fn eval_concrete(
    parsed: &ParsedExpression,
    lib: &FunctionLibrary,
    st: &SymbolTable,
    fmt: PathFormat,
) -> (Result<ExprValue, ExpressionError>, usize) {
    measure(|| {
        let tabs = [st];
        parsed
            .with_library(lib)
            .with_memory_limit(MEMORY_LIMIT)
            .with_operation_limit(OP_LIMIT)
            .with_path_format(fmt)
            .evaluate(&tabs)
    })
}

/// Does `v` belong to static type `ty`? Type variables left in a static
/// type (from signatures whose return variable the arguments do not bind,
/// e.g. `__add__: (list[T1], list[T2]) -> list[T3]`) read as wildcards, and
/// an empty list (`list[nulltype]`) belongs to every list type.
pub fn fits(v: &ExprValue, ty: &ExprType) -> bool {
    let wild = std::collections::HashMap::from([
        (TypeCode::TypeVarT, ExprType::ANY),
        (TypeCode::TypeVarT1, ExprType::ANY),
        (TypeCode::TypeVarT2, ExprType::ANY),
        (TypeCode::TypeVarT3, ExprType::ANY),
    ]);
    let ty = ty.substitute(&wild);
    if v.expr_type().satisfies(&ty) {
        return true;
    }
    // Lists: element-wise, against the list type or any list member of a
    // union (so `[[], []]` belongs to `list[list[int]]`).
    if let Some(elems) = v.list_elements() {
        let candidates: Vec<ExprType> = if ty.code() == TypeCode::Union {
            ty.params().to_vec()
        } else {
            vec![ty.clone()]
        };
        return candidates.iter().any(|c| {
            *c == ExprType::ANY
                || c.list_element_type()
                    .is_some_and(|e| elems.iter().all(|x| fits(x, e)))
        });
    }
    false
}

/// Does `src` use `and` / `or` as an operator (word-bounded)?
pub(crate) fn uses_boolop(src: &str) -> bool {
    let b = src.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'.';
    ["or", "and"].iter().any(|w| {
        src.match_indices(w).any(|(i, _)| {
            let before = i == 0 || !ident(b[i - 1]);
            let after = i + w.len() >= b.len() || !ident(b[i + w.len()]);
            before && after
        })
    })
}

/// Equality that compares `RangeExpr`s by their values (known issue
/// `expr-range-structural-eq`: `RangeExpr` equality is structural).
fn eq_by_range_values(a: &ExprValue, b: &ExprValue) -> bool {
    match (a, b) {
        (ExprValue::RangeExpr(x), ExprValue::RangeExpr(y)) => {
            x.len() == y.len() && (x.len() > 100_000 || x.to_vec() == y.to_vec())
        }
        _ => match (a.list_elements(), b.list_elements()) {
            (Some(xs), Some(ys)) if a.expr_type().to_string().contains("range_expr") => {
                xs.len() == ys.len() && xs.iter().zip(&ys).all(|(x, y)| eq_by_range_values(x, y))
            }
            _ => a.equals(b),
        },
    }
}

/// Does `t` still contain a type variable? A static result typed
/// `list[T3]` means the checker lost the element type through a signature
/// whose return variable no parameter binds (`expr-unbound-return-typevar`).
fn has_type_var(t: &ExprType) -> bool {
    matches!(
        t.code(),
        TypeCode::TypeVarT | TypeCode::TypeVarT1 | TypeCode::TypeVarT2 | TypeCode::TypeVarT3
    ) || t.params().iter().any(has_type_var)
}

/// Static type of a static evaluation result.
fn static_type(v: &ExprValue) -> ExprType {
    match v {
        ExprValue::Unresolved(t) => t.clone(),
        other => other.expr_type(),
    }
}

/// Every oracle that applies to one parsed expression in one environment.
/// `generated` carries the generator's [`Facts`] for generator output, which
/// is well-typed by construction (modulo those facts); `None` for text.
pub fn check_expression(
    parsed: &ParsedExpression,
    env: &Env,
    u: &mut Unstructured<'_>,
    generated: Option<Facts>,
) {
    let src = parsed.expression();
    let well_typed = generated.is_some();
    let facts = generated.unwrap_or_else(|| Facts::from_text(src));
    let runtime = runtime_library(&env.rules);
    let validation = validation_library();

    // Static pass, as template validation runs it.
    let static_result = {
        let tabs = [&env.unresolved];
        parsed
            .with_library(&validation)
            .with_memory_limit(MEMORY_LIMIT)
            .with_operation_limit(OP_LIMIT)
            .evaluate(&tabs)
    };

    for fmt in [PathFormat::Posix, PathFormat::Windows] {
        let table = env.concrete_in(fmt);
        let (result, peak) = eval_concrete(parsed, &runtime, &table, fmt);
        if let Err(e) = &result {
            // The error's source chain is finite and every link explains
            // itself.
            let mut link = std::error::Error::source(e);
            let mut depth = 0;
            while let Some(x) = link {
                depth += 1;
                oracle!(
                    depth < 32 && !x.to_string().is_empty(),
                    "error source chain of {e} is degenerate"
                );
                link = x.source();
            }
        }
        oracle!(
            peak <= PEAK_ALLOWANCE,
            "evaluation of {src:?} ({fmt:?}) peaked at {peak} bytes with a {MEMORY_LIMIT}-byte limit"
        );

        // Determinism: the evaluator has no global state.
        let (again, _) = eval_concrete(parsed, &runtime, &table, fmt);
        match (&result, &again) {
            (Ok(a), Ok(b)) => oracle!(
                a.equals(b) && a.to_display_string() == b.to_display_string(),
                "nondeterministic result for {src:?}: {a:?} vs {b:?}"
            ),
            (Err(a), Err(b)) => oracle!(
                a.to_string() == b.to_string(),
                "nondeterministic error for {src:?}: {a} vs {b}"
            ),
            _ => oracle!(
                false,
                "nondeterministic outcome for {src:?}: {result:?} vs {again:?}"
            ),
        }

        match (&static_result, &result) {
            (Ok(sv), Ok(cv)) => {
                let st = static_type(sv);
                let ct = cv.expr_type();
                let mismatch = !fits(cv, &st);
                let flatten_null = st.to_string().contains("nulltype") && src.contains("flatten");
                // Every known issue that can explain the mismatch is recorded
                // (the cause is ambiguous when several apply); a mismatch none
                // explains is a new bug.
                let mut explained: Vec<&'static str> = Vec::new();
                if mismatch {
                    if facts.unbound_return || has_type_var(&st) {
                        explained.push("expr-unbound-return-typevar");
                    }
                    // The union may come from an untyped `[]` in a conditional branch.
                    if (facts.union || facts.conditional) && flatten_null {
                        explained.push("expr-flatten-union-static-type");
                    }
                    if facts.union && !flatten_null {
                        explained.push("expr-union-arg-overload");
                    }
                    if st == ExprType::FLOAT
                        && matches!(cv, ExprValue::Int(0))
                        && src.contains("sum(")
                    {
                        explained.push("expr-sum-empty-float");
                    }
                    // The static pass folds path operations in the host's
                    // format; in the other format a folded condition can go
                    // the other way. Only when the host-format value fits.
                    if fmt != PathFormat::host() {
                        let host = PathFormat::host();
                        let (hv, _) = eval_concrete(parsed, &runtime, &env.concrete_in(host), host);
                        if hv.is_ok_and(|hv| fits(&hv, &st)) {
                            explained.push("expr-static-fold-host-path-format");
                        }
                    }
                }
                if !explained.is_empty() {
                    for id in explained {
                        crate::known::hit(id, || {
                            format!(
                                "{src:?}: static type {st} but run-time value {cv:?} has type {ct}"
                            )
                        });
                    }
                } else {
                    oracle!(
                        fits(cv, &st),
                        "{src:?}: static type {st} but run-time value {cv:?} has type {ct}"
                    );
                }
                if !sv.is_unresolved() && fmt == PathFormat::host() {
                    oracle!(
                        sv.equals(cv),
                        "{src:?}: statically folded to {sv:?} but evaluates to {cv:?}"
                    );
                }
            }
            (Ok(sv), Err(e)) => {
                let lost = Facts {
                    unbound_return: facts.unbound_return || has_type_var(&static_type(sv)),
                    ..facts
                };
                check_static_miss(e, lost, || {
                    format!("{src:?}: accepted statically ({sv:?}) but run-time evaluation fails with {e}")
                });
                if well_typed && !static_class(e.kind()) && !is_null_element_error(e) {
                    oracle!(
                        value_dependent(e.kind()),
                        "{src:?}: accepted statically ({sv:?}) but fails at run time: {e}"
                    );
                }
            }
            (Err(e), Ok(cv)) if well_typed => {
                let static_reject = matches!(
                    e.kind(),
                    ExpressionErrorKind::TypeError { .. }
                        | ExpressionErrorKind::UndefinedVariable { .. }
                        | ExpressionErrorKind::UnknownFunction { .. }
                );
                if is_null_element_error(e) {
                    // Dead code that fails whenever evaluated (a list of
                    // `None`) is rightly refused statically.
                } else if facts.unbound_return
                    && matches!(e.kind(), ExpressionErrorKind::TypeError { .. })
                {
                    known_issue!(
                        "expr-unbound-return-typevar",
                        false,
                        "well-typed {src:?} evaluates to {cv:?} but static checking rejects it: {e}"
                    );
                } else {
                    oracle!(
                        !static_reject,
                        "well-typed {src:?} evaluates to {cv:?} but static checking rejects it: {e}"
                    );
                }
            }
            _ => {}
        }

        if let Ok(v) = &result {
            check_value(v, fmt);
        }
    }

    // Metrics stay within the limits that produced them.
    let host_table = env.concrete_in(PathFormat::host());
    let tabs = [&host_table];
    if let Ok(m) = parsed
        .with_library(&runtime)
        .with_memory_limit(MEMORY_LIMIT)
        .with_operation_limit(OP_LIMIT)
        .evaluate_with_metrics(&tabs)
    {
        oracle!(
            m.peak_memory <= MEMORY_LIMIT && m.operation_count <= OP_LIMIT,
            "{src:?}: metrics {}/{} exceed limits {MEMORY_LIMIT}/{OP_LIMIT}",
            m.peak_memory,
            m.operation_count
        );
    }

    // Target-type coercion: a value returned for target T has type T.
    let target = if one_in(u, 2) {
        concrete_type(u)
    } else {
        any_type(u, 2)
    };
    let host_table = env.concrete_in(PathFormat::host());
    let tabs = [&host_table];
    if let Ok(v) = parsed
        .with_library(&runtime)
        .with_memory_limit(MEMORY_LIMIT)
        .with_operation_limit(OP_LIMIT)
        .with_target_type(&target)
        .evaluate(&tabs)
    {
        if target.is_concrete() {
            oracle!(
                fits(&v, &target) || matches!(v, ExprValue::Null),
                "{src:?} with target type {target} returned {v:?} of type {}",
                v.expr_type()
            );
        }
    }

    // The convenience shortcuts agree with the builder they abbreviate.
    let host_table2 = env.concrete_in(PathFormat::host());
    let short = parsed
        .with_memory_limit(MEMORY_LIMIT)
        .with_operation_limit(OP_LIMIT)
        .with_library(&runtime)
        .evaluate(&[&host_table2]);
    let _ = parsed
        .with_operation_limit(OP_LIMIT)
        .with_memory_limit(MEMORY_LIMIT);
    if !parsed
        .called_functions()
        .iter()
        .any(|f| f == "apply_path_mapping")
    {
        let tabs = [&host_table2];
        let plain = parsed
            .with_library(&runtime)
            .with_memory_limit(MEMORY_LIMIT)
            .with_operation_limit(OP_LIMIT)
            .evaluate_with_metrics(&tabs)
            .map(|m| m.value);
        oracle!(
            short.is_ok() == plain.is_ok(),
            "{src:?}: builder order changes the outcome: {short:?} vs {plain:?}"
        );
    }
    if src.len() < 64 {
        // The default-configuration shortcuts (library defaults, host format).
        let _ = parsed.evaluate(&host_table2);
        let _ = parsed.evaluate_with_metrics(&[&host_table2]);
        let _ = parsed
            .with_target_type(&ExprType::STRING)
            .evaluate(&[&host_table2]);
    }

    // Parse metadata: a bare name lookup reads exactly that symbol.
    if let Some(name) = parsed.as_name_lookup() {
        oracle!(
            parsed.accessed_symbols().contains(name),
            "{src:?}: name lookup {name:?} not in accessed_symbols {:?}",
            parsed.accessed_symbols()
        );
    }
}

/// Oracles on any concrete value.
pub fn check_value(v: &ExprValue, fmt: PathFormat) {
    let _ = v.repr_python();
    let _ = v.memory_size();
    let display = v.to_display_string();
    oracle!(v.equals(v), "value not equal to itself: {v:?}");
    oracle!(
        hash_of(v) == hash_of(&v.clone()),
        "hash not stable across clone: {v:?}"
    );

    // The `From` conversions build the same value from its parts.
    let rebuilt = match v {
        ExprValue::Bool(b) => Some(ExprValue::from(*b)),
        ExprValue::Int(i) => {
            if let Ok(n) = i32::try_from(*i) {
                oracle!(ExprValue::from(n).equals(v), "From<i32> of {n} != {v:?}");
            }
            Some(ExprValue::from(*i))
        }
        ExprValue::String(x) => {
            oracle!(
                ExprValue::from(x.as_str()).equals(v),
                "From<&str> of {x:?} != {v:?}"
            );
            Some(ExprValue::from(x.clone()))
        }
        ExprValue::RangeExpr(r) => Some(ExprValue::from(r.clone())),
        ExprValue::Unresolved(t) => Some(ExprValue::from(t.clone())),
        _ => None,
    };
    if let Some(r) = rebuilt {
        oracle!(r.equals(v), "From conversion of {v:?}'s parts gives {r:?}");
    }
    // `Float64`: hashing follows the bits, ordering against `f64` follows
    // the value.
    if let ExprValue::Float(f) = v {
        // Hash agrees with Eq: the same value without its original spelling
        // is equal, so it must hash equal.
        if let Ok(g) = openjd_expr::value::Float64::new(**f) {
            if *f == g {
                oracle!(
                    hash_of(f) == hash_of(&g),
                    "equal Float64s {f:?} and {g:?} hash differently"
                );
            }
        }
        for x in [**f, 0.0, -1.5, f64::INFINITY] {
            oracle!(
                f.partial_cmp(&x) == (**f).partial_cmp(&x),
                "Float64 {f:?} vs {x}: ordering disagrees with the value"
            );
        }
    }

    // Transport round trip — the template-scope → session-scope boundary.
    let wire = v.to_json_transport();
    let has_range = matches!(v, ExprValue::RangeExpr(_))
        || matches!(v, ExprValue::ListList(_, t, _) if *t == ExprType::RANGE_EXPR);
    match ExprValue::from_json_transport(&wire, fmt) {
        Ok(back) => {
            if has_range && eq_by_range_values(&back, v) {
                known_issue!(
                    "expr-range-structural-eq",
                    back.equals(v),
                    "transport round trip changed {v:?} -> {wire} -> {back:?}"
                );
            } else {
                oracle!(
                    back.equals(v),
                    "transport round trip changed {v:?} -> {wire} -> {back:?}"
                );
                oracle!(
                    hash_of(&back) == hash_of(v),
                    "equal values hash differently: {v:?} vs {back:?}"
                );
            }
            if !has_range {
                oracle!(
                    back.to_display_string() == display,
                    "transport round trip changed display {display:?} -> {:?}",
                    back.to_display_string()
                );
            }
        }
        Err(e) if has_range && (display.is_empty() || wire.to_string().contains(r#""""#)) => {
            known_issue!(
                "expr-empty-range-transport",
                false,
                "transport encoding of {v:?} does not decode: {wire} -> {e}"
            );
        }
        Err(e) => {
            oracle!(
                false,
                "transport encoding of {v:?} does not decode: {wire} -> {e}"
            );
        }
    }

    // Lists: indexed access agrees with iteration.
    if let (Some(n), Some(it)) = (v.list_len(), v.list_iter()) {
        let items: Vec<ExprValue> = it.collect();
        oracle!(items.len() == n, "list_len {n} != iterated {}", items.len());
        for (i, item) in items.iter().enumerate().take(8) {
            let got = v.list_get(i as i64);
            oracle!(
                got.as_ref().is_some_and(|g| g.equals(item)),
                "list_get({i}) = {got:?}, iteration gives {item:?}"
            );
        }
        oracle!(
            v.list_get(n as i64).is_none(),
            "list_get past the end returned a value"
        );
    }
}

/// Bind symbols referenced by `parsed` to types chosen by the control bytes
/// (or, with no control bytes, by a hash of the name, so text seeds still
/// exercise varied types).
fn env_for_text(parsed: &ParsedExpression, u: &mut Unstructured<'_>, fmt: PathFormat) -> Env {
    let mut names: Vec<&String> = parsed.accessed_symbols().iter().collect();
    names.sort();
    let symbols = names
        .into_iter()
        .map(|n| {
            let ty = if u.is_empty() {
                let types = crate::gen::value::SCALAR_TYPES;
                types[(digest(n) % types.len() as u64) as usize].clone()
            } else {
                concrete_type(u)
            };
            Symbol {
                name: n.clone(),
                ty,
            }
        })
        .collect();
    Env::new(u, symbols, fmt)
}

/// `ParsedExpression::new` / `with_profile` on arbitrary text.
///
/// Oracles: parsing is profile-monotonic (anything `current()` accepts,
/// `latest()` accepts — extensions only add syntax) and deterministic, and
/// parse metadata is consistent.
pub fn parse(data: &[u8]) {
    let Some((text, _)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_unclosed_brackets(text) {
        return;
    }
    let current = ParsedExpression::with_profile(text, &ExprProfile::current());
    let latest = ParsedExpression::new(text);
    if let Ok(c) = &current {
        oracle!(
            latest.is_ok(),
            "{text:?} parses under current() but not latest(): {:?}",
            latest.err()
        );
        let _ = c.called_functions();
        let _ = c.local_bindings();
    }
    let again = ParsedExpression::new(text);
    oracle!(
        latest.is_ok() == again.is_ok(),
        "parse of {text:?} is nondeterministic"
    );
}

/// Parse arbitrary text, then evaluate it statically and concretely
/// against symbols bound to the names it references.
pub fn eval_text(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_unclosed_brackets(text) {
        return;
    }
    let Ok(parsed) = ParsedExpression::new(text) else {
        return;
    };
    let mut u = Unstructured::new(ctl);
    let fmt = if u.is_empty() {
        PathFormat::Posix
    } else {
        path_format(&mut u)
    };
    let env = env_for_text(&parsed, &mut u, fmt);
    check_expression(&parsed, &env, &mut u, None);
}

/// Generate well-typed expressions from the library's signatures, over a
/// generated environment, and run every expression oracle.
pub fn eval_gen(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let n = upto(&mut u, 4);
    let mut symbols = Vec::new();
    for i in 0..n {
        let prefix = pick(
            &mut u,
            &["Param", "Task.Param", "Session", "Env.File", "RawParam"],
        );
        symbols.push(Symbol {
            name: format!("{prefix}.{}{i}", gen::ident(&mut u)),
            ty: concrete_type(&mut u),
        });
    }
    let fmt = path_format(&mut u);
    let env = Env::new(&mut u, symbols, fmt);
    let mut g = ExprGen::new(&mut u, &env.symbols);
    let text = g.any();
    let facts = Facts::from_gen(&g);
    let Ok(parsed) = ParsedExpression::new(&text) else {
        return;
    };
    let (depth, max) = (text.len(), text.len() / 2);
    let deep = ExpressionError::expression_too_deep(depth, max).to_string();
    oracle!(
        deep.contains(&depth.to_string()) && deep.contains(&max.to_string()),
        "expression_too_deep({depth}, {max}) message {deep:?} omits a number"
    );
    check_expression(&parsed, &env, &mut u, Some(facts));
}

/// `FormatString` parse, resolution, and static validation.
///
/// Oracles: static resolution is sound for the concrete resolution
/// (`min_resolved_string_len` is a lower bound, `resolved_value` is exact,
/// `resolved_type` is satisfied); literal segments appear in order;
/// `copy_used_symtab_values` keeps exactly what resolution needs; escaping
/// round-trips; budgets hold.
pub fn format_string(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_interpolations(text) {
        return;
    }
    let mut u = Unstructured::new(ctl);

    // Escaping: any text escapes to a literal that resolves to itself.
    let escaped = escape_format_string(text);
    match FormatString::new(&escaped) {
        Ok(fs) => {
            let r = fs.resolve_string_with(&SymbolTable::new(), &FormatStringOptions::new());
            oracle!(
                r.as_ref().ok().map(String::as_str) == Some(text),
                "escaped {text:?} -> {escaped:?} resolves to {r:?}"
            );
        }
        Err(e) => {
            if text.len() < 64 * 1024 {
                oracle!(false, "escaped {text:?} -> {escaped:?} does not parse: {e}");
            }
        }
    }

    let Ok(fs) = FormatString::new(text) else {
        return;
    };
    // Display and Serialize both give the raw text, and the serialized form
    // deserializes back to an equal format string.
    oracle!(
        fs.to_string() == text,
        "FormatString display {fs} != {text:?}"
    );
    match serde_json::to_value(&fs) {
        Ok(serde_json::Value::String(s)) => {
            oracle!(s == text, "FormatString serializes to {s:?}, not {text:?}");
            match serde_json::from_value::<FormatString>(serde_json::Value::String(s)) {
                Ok(back) => oracle!(back == fs, "FormatString serde round trip changed it"),
                Err(e) => oracle!(false, "FormatString does not deserialize its own form: {e}"),
            }
        }
        other => oracle!(false, "FormatString serializes to {other:?}"),
    }
    let mut names: Vec<String> = fs.accessed_symbols().into_iter().collect();
    names.sort();
    for n in fs.expression_names() {
        oracle!(
            names.iter().any(|a| a == n),
            "{text:?}: expression name {n:?} not among accessed symbols {names:?}"
        );
    }
    let symbols = names
        .iter()
        .map(|n| Symbol {
            name: n.clone(),
            ty: if u.is_empty() {
                let types = crate::gen::value::SCALAR_TYPES;
                types[(digest(n) % types.len() as u64) as usize].clone()
            } else {
                concrete_type(&mut u)
            },
        })
        .collect();
    let fmt = if u.is_empty() {
        PathFormat::Posix
    } else {
        path_format(&mut u)
    };
    let env = Env::new(&mut u, symbols, fmt);
    let runtime = runtime_library(&env.rules);
    let validation = validation_library();
    let target = if one_in(&mut u, 3) {
        Some(concrete_type(&mut u))
    } else {
        None
    };
    let mut ropts = FormatStringOptions::new()
        .with_library(&*runtime)
        .with_path_format(fmt)
        .with_memory_limit(MEMORY_LIMIT)
        .with_operation_limit(OP_LIMIT);
    let mut vopts = FormatStringOptions::new()
        .with_library(&*validation)
        .with_path_format(fmt)
        .with_memory_limit(MEMORY_LIMIT)
        .with_operation_limit(OP_LIMIT);
    if let Some(t) = &target {
        ropts = ropts.with_target_type(t);
        vopts = vopts.with_target_type(t);
    }

    let (resolved, peak) = measure(|| fs.resolve_with(&env.concrete, &ropts));
    oracle!(
        peak <= PEAK_ALLOWANCE,
        "resolving {text:?} peaked at {peak} bytes"
    );
    let (as_string, peak) = measure(|| fs.resolve_string_with(&env.concrete, &ropts));
    oracle!(
        peak <= 2 * PEAK_ALLOWANCE + text.len() * 2,
        "resolve_string_with {text:?} peaked at {peak} bytes"
    );

    if let Ok(s) = &as_string {
        let mut rest = s.as_str();
        for lit in fs.literal_segments() {
            match rest.find(lit) {
                Some(i) => rest = &rest[i + lit.len()..],
                None => oracle!(
                    false,
                    "{text:?}: literal {lit:?} missing from resolution {s:?}"
                ),
            }
        }
    }

    // copy_used_symtab_values keeps everything resolution reads.
    let mut filtered = SymbolTable::new();
    fs.copy_used_symtab_values(&env.concrete, &mut filtered);
    let from_filtered = fs.resolve_string_with(&filtered, &ropts);
    if let Ok(s) = &as_string {
        oracle!(
            from_filtered.as_ref().ok() == Some(s),
            "{text:?}: resolution against copy_used_symtab_values differs: {from_filtered:?} vs {as_string:?}"
        );
    }

    match fs.validate_expressions(&env.unresolved, &vopts) {
        Ok(sr) => {
            if let Ok(s) = &as_string {
                let n = s.chars().count();
                if target.is_none() || fs.segment_count() != 1 {
                    oracle!(
                        n >= sr.min_resolved_string_len,
                        "{text:?}: static lower bound {} > actual length {n} ({s:?})",
                        sr.min_resolved_string_len
                    );
                }
            }
            if let (Some(sv), Ok(rv)) = (&sr.resolved_value, &resolved) {
                if fmt == PathFormat::host() {
                    oracle!(
                        sv.equals(rv),
                        "{text:?}: static resolved_value {sv:?} but resolution gives {rv:?}"
                    );
                }
            }
            match &resolved {
                Ok(rv) => oracle!(
                    fits(rv, &sr.resolved_type) || matches!(rv, ExprValue::Null),
                    "{text:?}: static resolved_type {} but resolution gives {rv:?}",
                    sr.resolved_type
                ),
                Err(e) => check_static_miss(e, Facts::from_text(text), || {
                    format!("{text:?}: validates but resolution fails with {e}")
                }),
            }
        }
        Err(e) => {
            let _ = e.to_string();
        }
    }
}

/// `RangeExpr::from_str` and everything derived from a parsed range,
/// checked against a plain `Vec<i64>` reference model.
pub fn range(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    let r = match RangeExpr::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let _ = (
                openjd_expr::RangeExprError::new(text, e.message()).to_string(),
                openjd_expr::RangeExprError::at(text, e.message(), 0).to_string(),
            );
            // The conversion to ExpressionError keeps the message.
            let msg = e.message().to_string();
            let ee = ExpressionError::from(openjd_expr::RangeExprError::new(text, &msg));
            oracle!(
                ee.to_string().contains(&msg),
                "ExpressionError from range error {msg:?} loses it: {ee}"
            );
            return;
        }
    };
    check_range(&r, ctl);
}

fn check_range(r: &RangeExpr, ctl: &[u8]) {
    let len = r.len();
    oracle!(
        r.is_empty() == (len == 0),
        "is_empty disagrees with len {len}"
    );
    let _ = r.heap_size();
    let _ = r.cumulative_lengths();

    // Display/parse round trip by value; equality and hash agree.
    let shown = r.to_string();
    match RangeExpr::from_str(&shown) {
        Ok(back) => {
            oracle!(
                back.len() == len,
                "display {shown:?} reparses with len {}",
                back.len()
            );
            if len <= 4096 {
                oracle!(
                    back.to_vec() == r.to_vec(),
                    "display {shown:?} reparses to different values"
                );
            }
            if back == *r {
                oracle!(
                    hash_of(&back) == hash_of(r),
                    "equal ranges hash differently: {shown}"
                );
            }
        }
        Err(e) => oracle!(false, "display {shown:?} does not reparse: {e}"),
    }

    if len > 4096 {
        // Large ranges: spot-check the ends without materializing.
        let first = r.get(0);
        let last = r.get(len as i64 - 1);
        oracle!(
            first.is_some() && last.is_some(),
            "get at the ends of a {len}-element range"
        );
        oracle!(r.get(-1) == last, "get(-1) != get(len-1)");
        oracle!(r.get(len as i64).is_none(), "get(len) returned a value");
        return;
    }

    let values: Vec<i64> = r.iter().collect();
    oracle!(
        values.len() == len,
        "len {len} != iterated {}",
        values.len()
    );
    oracle!(r.to_vec() == values, "to_vec differs from iter");
    oracle!(
        values.windows(2).all(|w| w[0] < w[1]),
        "values not strictly increasing: {values:?}"
    );
    for (i, v) in values.iter().enumerate() {
        oracle!(r.get(i as i64) == Some(*v), "get({i}) != iter[{i}]");
        oracle!(r.contains(*v), "contains({v}) false for a member");
        if i + 1 < values.len() && values[i + 1] > v + 1 {
            oracle!(
                !r.contains(v + 1),
                "contains({}) true for a non-member",
                v + 1
            );
        }
    }
    if len > 0 {
        oracle!(r.get(-1) == values.last().copied(), "get(-1) != last");
        oracle!(
            r.get(-(len as i64)) == Some(values[0]),
            "get(-len) != first"
        );
    }
    oracle!(r.get(len as i64).is_none(), "get(len) returned a value");
    oracle!(
        r.get(-(len as i64) - 1).is_none(),
        "get(-len-1) returned a value"
    );

    match RangeExpr::from_values(values.clone()) {
        Ok(fv) => oracle!(
            fv.to_vec() == values,
            "from_values(to_vec) changed the values"
        ),
        Err(e) => oracle!(len == 0, "from_values(to_vec) failed: {e}"),
    }
    match RangeExpr::from_ranges(r.ranges().to_vec()) {
        Ok(fr) => oracle!(
            fr.to_vec() == values,
            "from_ranges(ranges) changed the values"
        ),
        Err(e) => oracle!(len == 0, "from_ranges(ranges) failed: {e}"),
    }

    // slice: clamp start/stop to [0, len], positive step (documented).
    let mut u = Unstructured::new(ctl);
    for _ in 0..4 {
        let pick_idx = |u: &mut Unstructured<'_>| {
            if one_in(u, 3) {
                gen::int(u)
            } else {
                u.int_in_range(-2..=len as i64 + 2).unwrap_or(0)
            }
        };
        let (a, b) = (pick_idx(&mut u), pick_idx(&mut u));
        let step = if one_in(&mut u, 4) {
            gen::int(&mut u)
        } else {
            u.int_in_range(-1..=4).unwrap_or(1)
        };
        let got = r.slice(a, b, step);
        if step <= 0 {
            oracle!(got.is_err(), "slice with step {step} succeeded");
            continue;
        }
        let lo = a.clamp(0, len as i64) as usize;
        let hi = b.clamp(0, len as i64) as usize;
        let expected: Vec<i64> = if lo >= hi {
            vec![]
        } else {
            values[lo..hi]
                .iter()
                .step_by(step.min(len.max(1) as i64) as usize)
                .copied()
                .collect()
        };
        match got {
            Ok(s) => oracle!(
                s.to_vec() == expected,
                "slice({a}, {b}, {step}) of {r} = {:?}, expected {expected:?}",
                s.to_vec()
            ),
            Err(e) => oracle!(expected.is_empty(), "slice({a}, {b}, {step}) failed: {e}"),
        }
    }
}

/// `IntRange::new` over the whole `i64` domain, then `RangeExpr` built from
/// the result.
pub fn int_range(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let (start, end, step) = (gen::int(&mut u), gen::int(&mut u), gen::int(&mut u));
    let Ok(ir) = openjd_expr::range_expr::IntRange::new(start, end, step) else {
        return;
    };
    oracle!(
        ir.step() > 0 && ir.start() <= ir.end(),
        "IntRange invariant broken: {ir:?}"
    );
    let len = ir.len();
    oracle!(ir.is_empty() == (len == 0), "is_empty disagrees with len");
    for (i, v) in ir.iter().take(64).enumerate() {
        oracle!(ir.get(i) == Some(v), "get({i}) != iter");
        oracle!(ir.contains(v), "contains({v}) false for a member");
    }
    if len > 0 {
        let last = ir.get(len - 1);
        oracle!(last.is_some(), "get(len-1) is None");
        oracle!(ir.get(len).is_none(), "get(len) returned a value");
    }
    if let Ok(r) = RangeExpr::from_ranges(vec![ir.clone()]) {
        oracle!(
            r.len() == len,
            "RangeExpr len {} != IntRange len {len}",
            r.len()
        );
        check_range(&r, u.take_rest());
    }
}

/// `ExprType::parse` on text, and display/parse of generated types.
///
/// Oracles: display/parse round trip, normalization idempotence,
/// reflexive satisfaction, union idempotence.
pub fn types(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    let mut candidates = Vec::new();
    if let Ok(t) = ExprType::parse(text) {
        candidates.push(t);
    }
    let mut u = Unstructured::new(ctl);
    candidates.push(any_type(&mut u, 3));
    for t in candidates {
        let shown = t.to_string();
        // A signature only exists at the top level of a type string; one
        // nested inside a list or union is a generator artifact.
        if shown.contains("->")
            && (t.code() != TypeCode::Signature || shown.matches("->").count() > 1)
        {
            continue;
        }
        match ExprType::parse(&shown) {
            Ok(back) => oracle!(
                back == t,
                "type {t:?} displays as {shown:?} which parses to {back:?}"
            ),
            Err(e) => oracle!(
                false,
                "type {t:?} displays as {shown:?} which does not parse: {e}"
            ),
        }
        if t.code() == TypeCode::Signature {
            // resolve_call agrees with match_call + substitute.
            let args: Vec<ExprType> = t.sig_params().to_vec();
            let resolved = t.resolve_call(&args);
            let matched = t.match_call(&args).map(|b| t.sig_return().substitute(&b));
            oracle!(
                resolved == matched,
                "{shown}: resolve_call {resolved:?} != match_call+substitute {matched:?}"
            );
        }
        let lib = validation_library();
        for prop in [
            "name", "stem", "suffix", "parent", "parts", "suffixes", "nope",
        ] {
            let _ = lib.get_property_type(&t, prop);
        }
        let rebuilt = ExprType::new(t.code(), t.params().to_vec());
        oracle!(
            rebuilt == t,
            "re-normalizing {shown} changed it to {rebuilt}"
        );
        oracle!(
            ExprType::union(vec![t.clone(), t.clone()]) == t || t.code() == TypeCode::Signature,
            "union[{shown}, {shown}] != {shown}"
        );
        // A type containing `noreturn` denotes no values, so nothing (not even
        // itself) satisfies it.
        if t.is_concrete() && !shown.contains("noreturn") {
            oracle!(t.satisfies(&t), "{shown} does not satisfy itself");
            oracle!(t.satisfies(&ExprType::ANY), "{shown} does not satisfy any");
        }
        let _ = t.is_symbolic();
        oracle!(
            hash_of(&t) == hash_of(&rebuilt),
            "equal types hash differently: {shown}"
        );
    }
}

/// Structured `ExprValue`s: transport, coercion, comparison, serde.
pub fn values(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let fmt = path_format(&mut u);
    let a = value(&mut u, fmt);
    let b = value(&mut u, fmt);
    check_value(&a, fmt);

    // Equality/hash/ordering consistency.
    if a.equals(&b) {
        oracle!(
            hash_of(&a) == hash_of(&b),
            "{a:?} == {b:?} but hashes differ"
        );
        oracle!(b.equals(&a), "equality not symmetric: {a:?} {b:?}");
    }
    if let (Ok(ab), Ok(ba)) = (a.compare(&b), b.compare(&a)) {
        oracle!(
            ab == ba.reverse(),
            "compare not antisymmetric: {a:?} {b:?} {ab:?} {ba:?}"
        );
    }

    // Coercion: the result satisfies the target, and coercion is idempotent.
    let target = if one_in(&mut u, 2) {
        concrete_type(&mut u)
    } else {
        any_type(&mut u, 2)
    };
    if let Ok(c) = a.clone().coerce(&target, fmt) {
        oracle!(
            fits(&c, &target) || matches!(c, ExprValue::Null),
            "coerce({a:?}, {target}) returned {c:?} of type {}",
            c.expr_type()
        );
        match c.clone().coerce(&target, fmt) {
            Ok(cc) => oracle!(
                cc.equals(&c),
                "coerce to {target} not idempotent: {c:?} -> {cc:?}"
            ),
            Err(e) => oracle!(
                false,
                "coerce to {target} accepted {a:?} but rejects its own output {c:?}: {e}"
            ),
        }
    }

    // from_str_coerce of a scalar's display form gives the scalar back.
    let t = a.expr_type();
    if !a.is_list() && !matches!(a, ExprValue::Null) {
        let s = a.to_display_string();
        match ExprValue::from_str_coerce(&s, &t, fmt) {
            Ok(back) if matches!(a, ExprValue::RangeExpr(_)) => known_issue!(
                "expr-range-structural-eq",
                back.equals(&a),
                "from_str_coerce({s:?}, {t}) = {back:?}, expected {a:?}"
            ),
            Ok(back) => oracle!(
                back.equals(&a),
                "from_str_coerce({s:?}, {t}) = {back:?}, expected {a:?}"
            ),
            Err(e) => oracle!(
                false,
                "from_str_coerce({s:?}, {t}) failed for a value's own display: {e}"
            ),
        }
    }
    let raw: &str = u.arbitrary().unwrap_or("");
    let _ = ExprValue::from_str_coerce(raw, &target, fmt);

    // Float64 serde round trip.
    if let ExprValue::Float(f) = &a {
        let json = serde_json::to_string(f).unwrap_or_default();
        match serde_json::from_str::<Float64>(&json) {
            Ok(back) => known_issue!(
                "expr-float64-serde-precision",
                back.value() == f.value() && back.to_display_string() == f.to_display_string(),
                "Float64 serde changed {f:?} -> {json} -> {back:?}"
            ),
            Err(e) => oracle!(
                false,
                "Float64 {f:?} serializes to {json} which does not deserialize: {e}"
            ),
        }
    }

    // make_list over generated elements: the result's elements equal inputs.
    let elems: Vec<ExprValue> = (0..upto(&mut u, 4)).map(|_| value(&mut u, fmt)).collect();
    if let Ok(l) = ExprValue::make_list(elems.clone(), ExprType::INT) {
        let got = l.list_elements().unwrap_or_default();
        oracle!(got.len() == elems.len(), "make_list changed the length");
        for (g, e) in got.iter().zip(&elems) {
            // int → float promotion is documented and may round.
            let promoted =
                e.expr_type().to_string().replace("int", "float") == g.expr_type().to_string();
            oracle!(
                g.equals(e) || promoted,
                "make_list element {e:?} became {g:?}"
            );
        }
        check_value(&l, fmt);
    }
}

/// An `EvalContext` with real budgets, for direct library calls.
struct Ctx {
    fmt: PathFormat,
    ops: usize,
}

impl EvalContext for Ctx {
    fn path_format(&self) -> PathFormat {
        self.fmt
    }
    fn count_op(&mut self) -> Result<(), ExpressionError> {
        self.count_ops(1)
    }
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError> {
        self.ops = self.ops.saturating_add(n);
        if self.ops > OP_LIMIT {
            Err(ExpressionError::from_kind(
                ExpressionErrorKind::OperationLimitExceeded {
                    count: self.ops,
                    limit: OP_LIMIT,
                },
            ))
        } else {
            Ok(())
        }
    }
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError> {
        self.count_ops(len.div_ceil(256))
    }
    fn check_memory(&self, bytes: usize) -> Result<(), ExpressionError> {
        if bytes > MEMORY_LIMIT {
            Err(ExpressionError::from_kind(
                ExpressionErrorKind::MemoryLimitExceeded {
                    used: bytes,
                    limit: MEMORY_LIMIT,
                },
            ))
        } else {
            Ok(())
        }
    }
}

/// Call library functions directly with signature-shaped (or arbitrary)
/// arguments, bypassing the parser.
///
/// Oracles: a successful call has a derivable return type that the value
/// satisfies; results respect the memory budget; peak heap stays bounded.
pub fn function_call(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let fmt = path_format(&mut u);
    let rules = path_rules(&mut u);
    let lib = runtime_library(&rules);
    let mut names: Vec<&str> = lib.function_names().collect();
    names.sort_unstable();
    let name = *pick(&mut u, &names);
    let sigs = lib.get_signatures(name);
    let args: Vec<ExprValue> = if !sigs.is_empty() && !one_in(&mut u, 4) {
        let sig = &pick(&mut u, sigs).signature;
        let mut bindings = std::collections::HashMap::new();
        for code in [
            TypeCode::TypeVarT,
            TypeCode::TypeVarT1,
            TypeCode::TypeVarT2,
            TypeCode::TypeVarT3,
        ] {
            bindings.insert(code, concrete_type(&mut u));
        }
        sig.sig_params()
            .iter()
            .map(|p| value_of(&mut u, &p.substitute(&bindings), fmt))
            .collect()
    } else {
        (0..upto(&mut u, 3)).map(|_| value(&mut u, fmt)).collect()
    };
    let method = one_in(&mut u, 2);
    let (result, peak) = measure(|| {
        let mut ctx = Ctx { fmt, ops: 0 };
        if method {
            lib.call_method(name, &args, &mut ctx)
        } else {
            lib.call(name, &args, &mut ctx)
        }
    });
    oracle!(
        peak <= PEAK_ALLOWANCE,
        "{name}({args:?}) peaked at {peak} bytes with a {MEMORY_LIMIT}-byte limit"
    );
    if let Ok(v) = result {
        let _ = v.to_display_string();
        oracle!(
            v.memory_size() <= 2 * MEMORY_LIMIT,
            "{name}({args:?}) returned {} bytes, past the {MEMORY_LIMIT}-byte budget",
            v.memory_size()
        );
        let arg_types: Vec<ExprType> = args.iter().map(ExprValue::expr_type).collect();
        match lib.derive_return_type(name, &arg_types) {
            Some(rt) if name == "sum" && matches!(v, ExprValue::Int(0)) => known_issue!(
                "expr-sum-empty-float",
                fits(&v, &rt),
                "{name}({arg_types:?}) derives {rt} but returned {v:?}"
            ),
            // Static derivation coerces (path → string) before trying the
            // overload run-time dispatch picks first.
            Some(rt)
                if !fits(&v, &rt)
                    && (arg_types.iter().any(|t| t.to_string().contains("path"))
                        || name == "re_findall") =>
            {
                known_issue!(
                    "expr-dispatch-derive-mismatch",
                    false,
                    "{name}({arg_types:?}) derives {rt} but returned {v:?} of type {}",
                    v.expr_type()
                )
            }
            Some(rt) => oracle!(
                fits(&v, &rt),
                "{name}({arg_types:?}) derives {rt} but returned {v:?} of type {}",
                v.expr_type()
            ),
            None => oracle!(
                false,
                "{name}({arg_types:?}) succeeded with {v:?} but derive_return_type finds no overload"
            ),
        }
    }
}

/// Path mapping and URI helpers.
///
/// Oracles: `apply_rules_with_format` is first-match-wins over the rules'
/// own `apply_with_format`; a rule maps its own source path; URI helpers
/// agree with `is_uri`/`parse` and satisfy `name == stem + suffix`.
pub fn paths(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let rules = path_rules(&mut u);
    let path = gen::path_string(&mut u);
    let out = path_format(&mut u);
    let expected = rules.iter().find_map(|r| r.apply_with_format(&path, out));
    let got = apply_rules_with_format(&rules, &path, out);
    let host = openjd_expr::path_mapping::apply_rules(&rules, &path);
    let host_expected = rules.iter().find_map(|r| r.apply(&path));
    if let Some(e) = &host_expected {
        oracle!(
            &host == e,
            "apply_rules({path:?}) = {host:?}, first matching rule gives {e:?}"
        );
    }
    for hc in [
        HostContext::None,
        HostContext::Unresolved,
        HostContext::with_rules(rules.clone()),
    ] {
        let _ = (hc.is_enabled(), hc.is_unresolved());
        let profile = ExprProfile::current().with_host_context(hc);
        let _ = (
            profile.revision(),
            profile.extensions().len(),
            profile.host_context(),
        );
        let _ = FunctionLibrary::for_profile(&profile)
            .function_names()
            .count();
    }
    if let Some(e) = &expected {
        oracle!(
            &got == e,
            "apply_rules({path:?}) = {got:?}, first matching rule gives {e:?}"
        );
    }
    for r in &rules {
        if !r.source_path.is_empty() && !is_uri(&r.source_path) {
            let _ = r.apply_with_format(&r.source_path, out);
        }
        let _ = r.apply(&path);
    }

    use openjd_expr::uri_path;
    for p in [&path, &format!("s3://b/{path}")] {
        let uri = uri_path::is_uri(p);
        oracle!(
            uri == uri_path::parse(p).is_some(),
            "is_uri({p:?}) = {uri} disagrees with parse"
        );
        oracle!(
            uri == is_uri(p),
            "uri_path::is_uri and path_mapping::is_uri disagree on {p:?}"
        );
        if uri {
            let name = uri_path::name(p);
            let stem = uri_path::stem(p);
            let suffix = uri_path::suffix(p);
            oracle!(
                name == format!("{stem}{suffix}"),
                "{p:?}: name {name:?} != stem {stem:?} + suffix {suffix:?}"
            );
            let _ = uri_path::suffixes(p);
            let _ = uri_path::parent(p);
            let parts = uri_path::parts(p);
            let _ = uri_path::from_parts(&parts);
            let child = gen::string(&mut u);
            let _ = uri_path::join(p, &child);
        }
    }

    // The same invariants through the expression language, per format.
    for fmt in [PathFormat::Posix, PathFormat::Windows] {
        let mut st = SymbolTable::new();
        let _ = st.set("P", ExprValue::new_path(path.clone(), fmt));
        let lib = runtime_library(&rules);
        let eval = |src: &str| {
            ParsedExpression::new(src).and_then(|p| {
                let tabs = [&st];
                p.with_library(&lib).with_path_format(fmt).evaluate(&tabs)
            })
        };
        if let (Ok(n), Ok(s), Ok(x)) = (eval("P.name"), eval("P.stem"), eval("P.suffix")) {
            oracle!(
                n.to_display_string()
                    == format!("{}{}", s.to_display_string(), x.to_display_string()),
                "{path:?} ({fmt:?}): name {n:?} != stem {s:?} + suffix {x:?}"
            );
        }
        if let Ok(v) = eval("P.parent") {
            let _ = v.to_display_string();
        }
        let _ = eval("apply_path_mapping(string(P))");
        let _ = eval("P.parts");
        let _ = eval("P.is_absolute()");
        let _ = eval("P.as_posix()");
        let _ = eval("bool(P)");
    }

    if let Ok(text) = std::str::from_utf8(u.take_rest()) {
        let _ = serde_json::from_str::<PathMappingRule>(text);
        let _ = serde_json::from_str::<Vec<PathMappingRule>>(text);
    }
}

/// Symbol tables: dotted-key operations against a map model, plus
/// serialization round trips and transport decoding of arbitrary JSON.
pub fn symbol_table(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let fmt = path_format(&mut u);
    let mut st = SymbolTable::new();
    let mut model: Vec<(String, ExprValue)> = Vec::new();
    for _ in 0..upto(&mut u, 8) {
        let depth = upto(&mut u, 2) + 1;
        let key: Vec<String> = (0..depth).map(|_| gen::ident(&mut u)).collect();
        let key = key.join(".");
        let v = value(&mut u, fmt);
        if st.set(&key, v.clone()).is_ok() {
            model.retain(|(k, _)| k != &key && !k.starts_with(&format!("{key}.")));
            model.push((key, v));
        }
    }
    for (k, v) in &model {
        let got = st.get_value(k);
        oracle!(
            got.is_some_and(|g| g.equals(v)),
            "after set({k:?}, {v:?}) get_value returns {got:?}"
        );
        oracle!(st.contains(k), "contains({k:?}) false after set");
    }
    let paths = st.all_paths("");
    oracle!(
        paths.windows(2).all(|w| w[0] <= w[1]),
        "all_paths not sorted: {paths:?}"
    );
    for p in &paths {
        oracle!(
            st.get_value(p).is_some(),
            "all_paths lists {p:?} but get_value finds nothing"
        );
    }

    // Serde round trip of the whole table.
    let ser = SerializedSymbolTable::from_symtab(&st);
    match serde_json::to_string(&ser).map(|j| serde_json::from_str::<SerializedSymbolTable>(&j)) {
        Ok(Ok(back)) => oracle!(
            back == ser,
            "SerializedSymbolTable serde round trip changed it"
        ),
        Ok(Err(e)) => oracle!(
            false,
            "SerializedSymbolTable does not deserialize its JSON: {e}"
        ),
        Err(e) => oracle!(false, "SerializedSymbolTable does not serialize: {e}"),
    }
    match ser.to_symtab(fmt) {
        Ok(back) => {
            oracle!(back.all_paths("") == paths, "transport changed the key set");
            for p in &paths {
                let (a, b) = (st.get_value(p), back.get_value(p));
                let same = matches!((a, b), (Some(a), Some(b)) if a.equals(b));
                let range_only = matches!((a, b), (Some(a), Some(b)) if eq_by_range_values(a, b));
                if range_only {
                    known_issue!(
                        "expr-range-structural-eq",
                        same,
                        "transport changed {p:?}: {a:?} -> {b:?}"
                    );
                } else {
                    oracle!(same, "transport changed {p:?}: {a:?} -> {b:?}");
                }
            }
            let has_ranges = paths.iter().any(|p| {
                st.get_value(p)
                    .is_some_and(|v| v.expr_type().to_string().contains("range_expr"))
            });
            if has_ranges {
                known_issue!(
                    "expr-range-structural-eq",
                    SerializedSymbolTable::from_symtab(&back) == ser,
                    "transport encoding not canonical"
                );
            } else {
                oracle!(
                    SerializedSymbolTable::from_symtab(&back) == ser,
                    "transport encoding not canonical"
                );
            }
        }
        Err(e) if e.contains("range_expr") => known_issue!(
            "expr-empty-range-transport",
            false,
            "symbol table transport does not decode: {e}"
        ),
        Err(e) => oracle!(false, "symbol table transport does not decode: {e}"),
    }

    // from_pairs / set_string / get_string / set_table agree with set / get.
    let pairs: Vec<(String, ExprValue)> = paths
        .iter()
        .filter_map(|p| st.get_value(p).map(|v| (p.clone(), v.clone())))
        .collect();
    match SymbolTable::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.clone()))) {
        Ok(fp) => {
            oracle!(
                fp.all_paths("") == paths,
                "from_pairs(all_paths) changed the key set"
            );
            // `collect()` (FromIterator) agrees with from_pairs; the entries
            // are a table's own, so it does not panic on a conflict.
            let collected: SymbolTable =
                pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            oracle!(
                collected.all_paths("") == paths,
                "FromIterator changed the key set"
            );
        }
        Err(e) => oracle!(false, "from_pairs rejects a table's own entries: {e:?}"),
    }
    if let Some(p) = paths.first() {
        let below = format!("{p}.{}", gen::ident(&mut u));
        if st.get_table(p).is_none() {
            match st.set(&below, ExprValue::Int(1)) {
                Err(e) => {
                    let msg = e.to_string();
                    oracle!(
                        msg.contains(&e.key) && msg.contains(&e.conflict),
                        "SymbolTableError display {msg:?} omits its key or conflict"
                    );
                    let ee = ExpressionError::from(e.clone());
                    oracle!(
                        ee.to_string().contains(&msg),
                        "ExpressionError from {msg:?} loses the message: {ee}"
                    );
                    let me = openjd_model::ModelError::from(e);
                    oracle!(
                        me.to_string().contains(&msg),
                        "ModelError from {msg:?} loses the message: {me}"
                    );
                }
                Ok(()) => oracle!(false, "set({below:?}) under the scalar {p:?} succeeded"),
            }
        }
    }
    let sk = format!("S.{}", gen::ident(&mut u));
    let sv = gen::string(&mut u);
    let mut st2 = SymbolTable::new();
    if st2.set_string(&sk, &sv).is_ok() {
        oracle!(
            st2.get_string(&sk) == Some(sv.as_str()),
            "set_string/get_string disagree on {sk:?}"
        );
    }
    let mut parent = SymbolTable::new();
    let tk = gen::ident(&mut u);
    parent.set_table(&tk, st2.clone());
    if let Some(t) = parent.get_table(&tk) {
        oracle!(
            t.all_paths("") == st2.all_paths(""),
            "set_table/get_table changed the table"
        );
    }
    let ser2 = SerializedSymbolTable::from_value(ser.as_value().clone());
    oracle!(
        ser2 == ser,
        "SerializedSymbolTable from_value(as_value) changed it"
    );

    // merge_from: leaves from `other` win.
    let mut other = SymbolTable::new();
    let k = gen::ident(&mut u);
    let v = value(&mut u, fmt);
    if other.set(&k, v.clone()).is_ok() {
        let mut merged = st.clone();
        merged.merge_from(&other);
        let got = merged.get_value(&k);
        oracle!(
            got.is_some_and(|g| g.equals(&v)),
            "merge_from lost {k:?}: {got:?}"
        );
    }

    if let Ok(text) = std::str::from_utf8(u.take_rest()) {
        if let Ok(s) = SerializedSymbolTable::from_json_str(text) {
            let _ = s.to_symtab(fmt);
        }
        let _ = serde_json::from_str::<SymbolTable>(text);
        let _ = serde_json::from_str::<RangeExpr>(text);
        let _ = serde_json::from_str::<FormatString>(text);
    }
}
