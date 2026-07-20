// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Runtime values for expression evaluation.

use crate::path_mapping::PathFormat;
use crate::range_expr::RangeExpr;
use crate::types::{ExprType, TypeCode};

/// A float with optional original string representation for passthrough.
/// 16 bytes: 8 for f64, 8 for `Option<Box<str>>` (NULL or heap pointer).
///
/// Fields are private. Construction goes through [`Float64::new`] or
/// [`Float64::with_str`], which enforce the no-NaN / no-Inf / no-`-0.0`
/// invariants that the `Hash` and `PartialEq` impls on `ExprValue` depend on.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Float64 {
    value: f64,
    original: Option<Box<str>>,
}

impl std::hash::Hash for Float64 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.to_bits().hash(state);
    }
}

/// Return `true` if `v` is exactly representable in the i64 value range.
///
/// The valid f64 doubles convertible to i64 lie in `[-2^63, 2^63)`.
/// `i64::MIN as f64` is exactly `-2^63`; the upper bound is its negation.
/// Note that a `> i64::MAX as f64` check is wrong: `i64::MAX` rounds up
/// to `2^63` in f64, which would let `2^63` through to an `as i64` cast
/// that silently saturates to `i64::MAX`. NaN compares false and is
/// therefore rejected.
pub(crate) fn float_fits_i64(v: f64) -> bool {
    const LOWER: f64 = i64::MIN as f64; // exactly -2^63
    (LOWER..-LOWER).contains(&v)
}

/// Normalize -0.0 to 0.0 (matches Python's copysign normalization).
fn normalize_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

impl Float64 {
    /// Create a new `Float64`, rejecting NaN and infinity, normalizing -0.0 to 0.0.
    pub fn new(v: f64) -> Result<Self, crate::error::ExpressionError> {
        let v = normalize_zero(v);
        if v.is_nan() {
            return Err(crate::error::ExpressionError::float_error(
                "Float operation produced NaN",
            ));
        }
        if v.is_infinite() {
            return Err(crate::error::ExpressionError::float_error(
                "Float operation produced infinity",
            ));
        }
        Ok(Self {
            value: v,
            original: None,
        })
    }
    /// Create a `Float64` preserving the original string representation for lossless display.
    pub fn with_str(v: f64, s: String) -> Result<Self, crate::error::ExpressionError> {
        let v = normalize_zero(v);
        if v.is_nan() {
            return Err(crate::error::ExpressionError::float_error(
                "Float operation produced NaN",
            ));
        }
        if v.is_infinite() {
            return Err(crate::error::ExpressionError::float_error(
                "Float operation produced infinity",
            ));
        }
        Ok(Self {
            value: v,
            original: if v == 0.0 && s != "0.0" {
                None
            } else {
                Some(s.into_boxed_str())
            },
        })
    }
    /// The underlying `f64` value.
    pub fn value(&self) -> f64 {
        self.value
    }
    /// Display string: the original literal if preserved, otherwise formatted.
    pub fn to_display_string(&self) -> String {
        if let Some(s) = &self.original {
            s.to_string()
        } else {
            format_float(self.value)
        }
    }
}

impl std::fmt::Display for Float64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

impl std::ops::Deref for Float64 {
    type Target = f64;
    fn deref(&self) -> &f64 {
        &self.value
    }
}

impl PartialEq<f64> for Float64 {
    fn eq(&self, other: &f64) -> bool {
        self.value == *other
    }
}

impl PartialOrd<f64> for Float64 {
    fn partial_cmp(&self, other: &f64) -> Option<std::cmp::Ordering> {
        self.value.partial_cmp(other)
    }
}

/// A typed value during expression evaluation.
///
/// `#[non_exhaustive]` because future revisions or extensions may add
/// new primitive types (e.g., `Duration`, `Url`, `Decimal`). Adding a
/// variant must not be a breaking change for downstream crates that
/// match on this enum. The `Path` variant has its own `#[non_exhaustive]`
/// attribute, which serves a separate purpose (preventing direct
/// struct-literal construction so that `ExprValue::new_path` can
/// enforce the separator-normalization invariant).
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub enum ExprValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(Float64),
    String(String),
    /// A PATH value — a string path together with its format.
    ///
    /// `#[non_exhaustive]` prevents direct construction outside this crate;
    /// downstream callers must use [`ExprValue::new_path`], which enforces
    /// the separator-normalization invariant (`\` ↔ `/` per `PathFormat`,
    /// and no normalization for URI paths). The fields remain visible for
    /// pattern matching (using `..` is required from outside the crate).
    #[non_exhaustive]
    Path {
        value: String,
        format: PathFormat,
    },
    // Typed list variants (new)
    ListBool(Vec<bool>),
    ListInt(Vec<i64>),
    ListFloat(Vec<Float64>),
    ListString(Vec<String>, usize), // (elements, cached_memory_size)
    ListPath(Vec<String>, PathFormat, usize), // (elements, format, cached_memory_size)
    ListList(Vec<ExprValue>, ExprType, usize), // (elements, element_type_hint, cached_memory_size)
    RangeExpr(RangeExpr),
    Unresolved(ExprType),
}

impl std::hash::Hash for ExprValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must be consistent with PartialEq (which uses equals()):
        // Int(1) == Float(1.0), String("x") == Path{value:"x",...}
        // Empty lists of any type are equal, so they must hash identically.
        match self {
            Self::Null => 0u8.hash(state),
            Self::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            // Int hashes with integer tag + raw i64 bits.
            Self::Int(i) => {
                2u8.hash(state);
                i.hash(state);
            }
            // Float hashes as Int when it's an exact integer in i64 range,
            // otherwise uses float tag + f64 bits.
            Self::Float(f) => {
                let v = f.value;
                if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
                    2u8.hash(state);
                    (v as i64).hash(state);
                } else {
                    12u8.hash(state);
                    v.to_bits().hash(state);
                }
            }
            // String and Path hash the same way so they match
            Self::String(s) => {
                3u8.hash(state);
                s.hash(state);
            }
            Self::Path { value, .. } => {
                3u8.hash(state);
                value.hash(state);
            }
            // All list types use discriminant 4 so empty lists hash equally.
            // Elements are hashed via their ExprValue-equivalent hash to maintain
            // consistency with cross-type equality (e.g. ListInt([1]) == ListFloat([1.0])).
            Self::ListBool(v) => {
                4u8.hash(state);
                for b in v {
                    1u8.hash(state);
                    b.hash(state);
                }
            }
            Self::ListInt(v) => {
                4u8.hash(state);
                for i in v {
                    2u8.hash(state);
                    i.hash(state);
                }
            }
            Self::ListFloat(v) => {
                4u8.hash(state);
                for f in v {
                    let fv = f.value;
                    if fv.fract() == 0.0 && fv >= i64::MIN as f64 && fv <= i64::MAX as f64 {
                        2u8.hash(state);
                        (fv as i64).hash(state);
                    } else {
                        12u8.hash(state);
                        fv.to_bits().hash(state);
                    }
                }
            }
            Self::ListString(v, _) => {
                4u8.hash(state);
                for s in v {
                    3u8.hash(state);
                    s.hash(state);
                }
            }
            Self::ListPath(v, _, _) => {
                4u8.hash(state);
                for s in v {
                    3u8.hash(state);
                    s.hash(state);
                }
            }
            Self::ListList(v, _, _) => {
                4u8.hash(state);
                for e in v {
                    e.hash(state);
                }
            }
            Self::RangeExpr(r) => {
                10u8.hash(state);
                r.hash(state);
            }
            Self::Unresolved(t) => {
                11u8.hash(state);
                t.hash(state);
            }
        }
    }
}

impl Eq for ExprValue {}

impl ExprValue {
    /// Create a list, promoting elements as needed. Produces old List variant for compatibility.
    fn make_list_string(v: Vec<String>) -> Self {
        let heap =
            v.len() * std::mem::size_of::<String>() + v.iter().map(|s| s.len()).sum::<usize>();
        Self::ListString(v, heap)
    }
    fn make_list_path(v: Vec<String>, fmt: PathFormat) -> Self {
        let heap =
            v.len() * std::mem::size_of::<String>() + v.iter().map(|s| s.len()).sum::<usize>();
        Self::ListPath(v, fmt, heap)
    }
    fn make_list_list(v: Vec<ExprValue>, elem_hint: ExprType) -> Self {
        // Vec buffer holds ExprValues inline; only count their additional heap allocations
        let heap = v.len() * std::mem::size_of::<ExprValue>()
            + v.iter().map(|e| e.heap_size()).sum::<usize>();
        let elem_type = v.first().map(|e| e.expr_type()).unwrap_or(elem_hint);
        Self::ListList(v, elem_type, heap)
    }

    /// Estimate the heap allocation required to build a list from `elements`.
    ///
    /// Upper bound on the `heap_size()` of the resulting list — ignores the
    /// type-promotion shortcuts in [`make_list`](Self::make_list) that can
    /// shrink the final footprint (e.g. collapsing `ListInt` elements into
    /// a single `ListFloat`). Treats the worst case of storing every
    /// element through a `ListList`, which is what a heterogeneous input
    /// ultimately materializes to.
    ///
    /// Used by [`make_list_checked`](Self::make_list_checked) to fail a
    /// memory-bounded evaluator cleanly before the list allocation
    /// happens, rather than after.
    fn estimate_list_heap_size(elements: &[ExprValue]) -> usize {
        let per_slot = std::mem::size_of::<ExprValue>();
        elements
            .iter()
            .fold(elements.len().saturating_mul(per_slot), |acc, e| {
                acc.saturating_add(e.heap_size())
            })
    }

    /// Memory-checked variant of [`make_list`](Self::make_list).
    ///
    /// Pre-checks the evaluator's memory budget against an upper-bound
    /// estimate of the list's heap footprint before any allocation occurs.
    /// This is the defense-in-depth path: call sites that have an
    /// [`EvalContext`](crate::function_library::EvalContext) available
    /// should prefer this over [`make_list`](Self::make_list) so that a
    /// memory-bounded evaluator fails cleanly on oversized intermediate
    /// lists — even from code paths that did not charge ops proportionally
    /// to the list size.
    ///
    /// Type promotion and nesting validation are otherwise identical to
    /// [`make_list`](Self::make_list); this function forwards to it after
    /// the memory check passes.
    pub fn make_list_checked(
        ctx: &mut dyn crate::function_library::EvalContext,
        elements: Vec<ExprValue>,
        hint_type: ExprType,
    ) -> Result<Self, crate::error::ExpressionError> {
        ctx.check_memory(Self::estimate_list_heap_size(&elements))?;
        Self::make_list(elements, hint_type)
    }

    /// Construct a typed list from heterogeneous elements.
    ///
    /// Applies type promotion rules: int+float→float, path+string→string.
    /// Uses `hint_type` for empty lists to determine the element type.
    /// Returns an error if any element is a `ListList`, which would create 3+ nesting levels.
    ///
    /// When called from an evaluator or function implementation that has
    /// an [`EvalContext`](crate::function_library::EvalContext), prefer
    /// [`make_list_checked`](Self::make_list_checked) so that an oversized
    /// intermediate list fails the evaluator's memory limit before the
    /// allocation happens.
    pub fn make_list(
        mut elements: Vec<ExprValue>,
        hint_type: ExprType,
    ) -> Result<Self, crate::error::ExpressionError> {
        // Reject 3+ nesting levels: if any element is itself a ListList with a
        // non-nulltype element type, that's too deep. Empty lists (ListList with
        // NULLTYPE) represent `list[nulltype]` — a flat empty list, not a nested one.
        if elements
            .iter()
            .any(|e| matches!(e, Self::ListList(_, et, _) if *et != ExprType::NULLTYPE))
        {
            return Err(crate::error::ExpressionError::new(
                "Lists may be nested at most 2 levels deep",
            ));
        }
        // Convert empty ListList([], NULLTYPE) elements to match typed list siblings.
        // e.g. in [[], [1]], the empty [] should become ListInt([]) not ListList([], NULLTYPE).
        let has_empty_listlist = elements.iter().any(
            |e| matches!(e, Self::ListList(v, et, _) if v.is_empty() && *et == ExprType::NULLTYPE),
        );
        if has_empty_listlist {
            // Find the first typed list sibling to determine the target variant
            let sibling_code = elements.iter().find_map(|e| match e {
                Self::ListBool(v) if !v.is_empty() => Some(crate::types::TypeCode::Bool),
                Self::ListInt(v) if !v.is_empty() => Some(crate::types::TypeCode::Int),
                Self::ListFloat(_) => Some(crate::types::TypeCode::Float),
                Self::ListString(v, _) if !v.is_empty() => Some(crate::types::TypeCode::String),
                Self::ListPath(v, _, _) if !v.is_empty() => Some(crate::types::TypeCode::Path),
                _ => None,
            });
            if let Some(code) = sibling_code {
                for e in &mut elements {
                    if matches!(e, Self::ListList(v, et, _) if v.is_empty() && *et == ExprType::NULLTYPE)
                    {
                        *e = match code {
                            crate::types::TypeCode::Bool => Self::ListBool(Vec::new()),
                            crate::types::TypeCode::Int => Self::ListInt(Vec::new()),
                            crate::types::TypeCode::Float => Self::ListFloat(Vec::new()),
                            crate::types::TypeCode::String => Self::ListString(Vec::new(), 0),
                            crate::types::TypeCode::Path => {
                                Self::make_list_path(Vec::new(), PathFormat::host())
                            }
                            _ => continue,
                        };
                    }
                }
            }
        }
        if elements.is_empty() {
            // Empty lists are list[nulltype], compatible with any list type.
            // When a concrete hint is provided, use the matching typed variant
            // so that subsequent operations (e.g. append) preserve the type.
            // Otherwise (Null or unknown hint), use ListList with NULLTYPE as the
            // canonical empty list representation, compatible with any list type.
            return Ok(match hint_type.code() {
                crate::types::TypeCode::Bool => Self::ListBool(Vec::new()),
                crate::types::TypeCode::Int => Self::ListInt(Vec::new()),
                crate::types::TypeCode::Float => Self::ListFloat(Vec::new()),
                crate::types::TypeCode::Path => {
                    Self::make_list_path(Vec::new(), PathFormat::host())
                }
                crate::types::TypeCode::List => Self::make_list_list(Vec::new(), hint_type),
                crate::types::TypeCode::String => Self::ListString(Vec::new(), 0),
                crate::types::TypeCode::NullType => {
                    Self::ListList(Vec::new(), ExprType::NULLTYPE, 0)
                }
                _ => Self::ListList(Vec::new(), ExprType::NULLTYPE, 0),
            });
        }
        let has_int = elements.iter().any(|e| matches!(e, Self::Int(_)));
        let has_float = elements.iter().any(|e| matches!(e, Self::Float(_)));
        if has_int && has_float {
            for e in &mut elements {
                if let Self::Int(i) = e {
                    *e = Self::Float(Float64::new(*i as f64).unwrap());
                }
            }
            return Ok(Self::ListFloat(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::Float(f) => f,
                        _ => unreachable!("all elements promoted to Float above"),
                    })
                    .collect(),
            ));
        }
        let has_list_int = elements
            .iter()
            .any(|e| e.is_list() && e.list_elem_type() == Some(ExprType::INT));
        let has_list_float = elements
            .iter()
            .any(|e| e.is_list() && e.list_elem_type() == Some(ExprType::FLOAT));
        if has_list_int && has_list_float {
            for e in &mut elements {
                if let Self::ListInt(ints) = e {
                    *e = Self::ListFloat(
                        ints.iter()
                            .map(|i| Float64::new(*i as f64).unwrap())
                            .collect(),
                    );
                }
            }
            return Ok(Self::make_list_list(elements, ExprType::NULLTYPE));
        }
        // Nested list path/string promotion: list[path] + list[string] → list[string]
        let has_list_path = elements
            .iter()
            .any(|e| e.is_list() && e.list_elem_type() == Some(ExprType::PATH));
        let has_list_string = elements
            .iter()
            .any(|e| e.is_list() && e.list_elem_type() == Some(ExprType::STRING));
        if has_list_path && has_list_string {
            for e in &mut elements {
                if let Self::ListPath(paths, _, _) = e {
                    *e = Self::make_list_string(std::mem::take(paths));
                }
            }
            return Ok(Self::make_list_list(elements, ExprType::NULLTYPE));
        }
        // Path/string promotion: mix of path and string → string
        let has_path = elements.iter().any(|e| matches!(e, Self::Path { .. }));
        let has_string = elements.iter().any(|e| matches!(e, Self::String(_)));
        if has_path && has_string {
            return Ok(Self::make_list_string(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::String(s) | Self::Path { value: s, .. } => s,
                        _ => e.to_display_string(),
                    })
                    .collect(),
            ));
        }
        Ok(match &elements[0] {
            Self::Bool(_) => Self::ListBool(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::Bool(b) => Ok(b),
                        _ => Err(crate::error::ExpressionError::type_error(format!(
                            "make_list expected bool element, got {}",
                            e.type_name()
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Int(_) => Self::ListInt(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::Int(i) => Ok(i),
                        _ => Err(crate::error::ExpressionError::type_error(format!(
                            "make_list expected int element, got {}",
                            e.type_name()
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Float(_) => Self::ListFloat(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::Float(f) => Ok(f),
                        _ => Err(crate::error::ExpressionError::type_error(format!(
                            "make_list expected float element, got {}",
                            e.type_name()
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::String(_) => Self::make_list_string(
                elements
                    .into_iter()
                    .map(|e| match e {
                        Self::String(s) => Ok(s),
                        _ => Err(crate::error::ExpressionError::type_error(format!(
                            "make_list expected string element, got {}",
                            e.type_name()
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::Path { format, .. } => {
                let fmt = *format;
                Self::make_list_path(
                    elements
                        .into_iter()
                        .map(|e| match e {
                            Self::Path { value, .. } => Ok(value),
                            Self::String(value) => Ok(value),
                            _ => Err(crate::error::ExpressionError::type_error(format!(
                                "make_list expected path element, got {}",
                                e.type_name()
                            ))),
                        })
                        .collect::<Result<_, _>>()?,
                    fmt,
                )
            }
            _ if elements[0].is_list() => Self::make_list_list(elements, ExprType::NULLTYPE),
            Self::RangeExpr(_) => Self::make_list_list(elements, ExprType::RANGE_EXPR),
            _ => {
                return Err(crate::error::ExpressionError::type_error(format!(
                    "Cannot create list from {} elements",
                    elements[0].type_name()
                )))
            }
        })
    }

    /// Create an unresolved value with a type constraint (for validation-time type checking).
    pub fn unresolved(constraint: ExprType) -> Self {
        Self::Unresolved(constraint)
    }
    /// Returns `true` if this is an `Unresolved` value.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved(_))
    }

    /// Create a PATH value with separators normalized to the given format.
    ///
    /// This is the only public constructor for `ExprValue::Path`; the variant
    /// itself is `#[non_exhaustive]` so downstream crates cannot bypass the
    /// separator-normalization invariant by constructing the struct directly.
    ///
    /// - `Posix`: no normalization — backslash is a valid filename character
    /// - `Windows`: `/` → `\` (unless the value is a URI)
    /// - `Uri`: no normalization
    pub fn new_path(value: impl Into<String>, format: PathFormat) -> Self {
        let value = value.into();
        let normalized = normalize_path_separators(&value, format);
        Self::Path {
            value: normalized,
            format,
        }
    }

    /// Coerce a string value to the given type.
    pub fn from_str_coerce(
        s: &str,
        target: &ExprType,
        path_format: PathFormat,
    ) -> Result<Self, String> {
        match target.code() {
            TypeCode::Int => s
                .parse::<i64>()
                .map(ExprValue::Int)
                .map_err(|e| format!("Cannot convert '{s}' to int: {e}")),
            TypeCode::Float => {
                let v: f64 = s
                    .parse()
                    .map_err(|e| format!("Cannot convert '{s}' to float: {e}"))?;
                if v.is_infinite() || v.is_nan() {
                    return Err(format!("Cannot convert '{s}' to float"));
                }
                Ok(ExprValue::Float(
                    Float64::with_str(v, s.to_string()).map_err(|e| e.to_string())?,
                ))
            }
            TypeCode::Bool => match s.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(ExprValue::Bool(true)),
                "false" | "no" | "off" | "0" => Ok(ExprValue::Bool(false)),
                _ => Err(format!("Cannot convert '{s}' to bool")),
            },
            TypeCode::String => Ok(ExprValue::String(s.to_string())),
            TypeCode::Path => Ok(ExprValue::new_path(s, path_format)),
            TypeCode::RangeExpr => {
                let r: crate::range_expr::RangeExpr =
                    s.parse().map_err(|e: crate::error::ExpressionError| {
                        format!("Cannot convert '{s}' to range_expr: {e}")
                    })?;
                Ok(ExprValue::RangeExpr(r))
            }
            TypeCode::NullType if s == "null" => Ok(ExprValue::Null),
            _ => Err(format!("Cannot coerce string to {target}")),
        }
    }

    /// Coerce a value to the given type.
    ///
    /// Coercion is non-destructive: only conversions that don't lose
    /// information are attempted (`int → float`, `int → string`, etc).
    ///
    /// For union targets, the rules are:
    ///
    /// 1. **Match first** — if the value's type is already one of the
    ///    union members, return it unchanged.
    /// 2. **Per-member coercion** — otherwise try non-destructive
    ///    coercion to each scalar member (skipping `nulltype`, `list[T]`,
    ///    and nested unions). Return the first successful coercion.
    /// 3. **Error** — if neither step yields a result.
    ///
    /// This mirrors the behavior of the pure-Python reference
    /// implementation's `evaluate.try_coerce_nondestructive` loop and
    /// satisfies RFC 0005 §"Implicit Type Coercion": `int | string`
    /// accepts an `int` value as-is rather than rejecting it.
    pub fn coerce(self, target: &ExprType, path_format: PathFormat) -> Result<Self, String> {
        // Match-first: also accepts the case where the target is a union
        // and the value's type is one of its members. Falls back to the
        // existing strict-equality behavior for non-union targets.
        if self.expr_type() == *target {
            return Ok(self);
        }
        if target.code() == TypeCode::Union && target.match_type(&self.expr_type()).is_some() {
            return Ok(self);
        }
        // For union targets that don't match by type-membership: try
        // non-destructive coercion to each scalar member. Skip
        // `nulltype` (only matches `null`, which would have matched
        // above), `list[T]` (lists must satisfy by membership, not
        // coercion — matching the reference), and nested unions.
        if target.code() == TypeCode::Union {
            for member in target.params() {
                if matches!(
                    member.code(),
                    TypeCode::NullType | TypeCode::List | TypeCode::Union
                ) {
                    continue;
                }
                if let Ok(coerced) = self.clone().coerce(member, path_format) {
                    return Ok(coerced);
                }
            }
            return Err(format!("Cannot coerce {} to {target}", self.expr_type()));
        }
        match (&self, target.code()) {
            (ExprValue::Int(i), TypeCode::Float) => {
                Ok(ExprValue::Float(Float64::new(*i as f64).unwrap()))
            }
            (ExprValue::Float(f), TypeCode::Int) => {
                let v = f.value();
                if v.fract() == 0.0 && v.is_finite() {
                    // Range check before the cast: `as i64` silently
                    // saturates for out-of-range values (e.g. 1e30 would
                    // become i64::MAX), so reject them with an integer
                    // overflow error instead.
                    if !float_fits_i64(v) {
                        return Err(
                            "Integer overflow: result is outside the 64-bit signed range"
                                .to_string(),
                        );
                    }
                    Ok(ExprValue::Int(v as i64))
                } else {
                    Err(format!(
                        "Cannot coerce float to int: {} is not a whole number",
                        f.to_display_string()
                    ))
                }
            }
            (ExprValue::Bool(b), TypeCode::String) => Ok(ExprValue::String(
                if *b { "true" } else { "false" }.to_string(),
            )),
            (ExprValue::Int(i), TypeCode::String) => Ok(ExprValue::String(i.to_string())),
            (ExprValue::Float(f), TypeCode::String) => Ok(ExprValue::String(f.to_display_string())),
            (ExprValue::String(s), _) => ExprValue::from_str_coerce(s, target, path_format),
            (ExprValue::Path { value, .. }, TypeCode::String) => {
                Ok(ExprValue::String(value.clone()))
            }
            (ExprValue::RangeExpr(r), TypeCode::String) => Ok(ExprValue::String(r.to_string())),
            (ExprValue::RangeExpr(r), TypeCode::List) => Ok(ExprValue::ListInt(r.to_vec())),
            _ if target.code() == TypeCode::List && target.params().len() == 1 => {
                let elem_type = &target.params()[0];
                if let Some(elements) = self.list_elements() {
                    let coerced: Result<Vec<_>, _> = elements
                        .into_iter()
                        .map(|e| e.coerce(elem_type, path_format))
                        .collect();
                    Ok(ExprValue::make_list(coerced?, elem_type.clone())
                        .map_err(|e| e.to_string())?)
                } else {
                    Err(format!("Cannot coerce {} to {target}", self.expr_type()))
                }
            }
            _ => Err(format!("Cannot coerce {} to {target}", self.expr_type())),
        }
    }

    /// Python-style repr: `ExprValue(42)`, `ExprValue('hello')`, `ExprValue([1, 2], type='list[int]')`.
    pub fn repr_python(&self) -> String {
        match self {
            Self::Null => "ExprValue(None)".to_string(),
            Self::Bool(b) => format!("ExprValue({})", if *b { "True" } else { "False" }),
            Self::Int(i) => format!("ExprValue({i})"),
            Self::Float(f) => {
                if f.original.is_some() {
                    format!("ExprValue('{}', type='float')", f.to_display_string())
                } else {
                    format!("ExprValue({})", f.to_display_string())
                }
            }
            Self::String(s) => format!("ExprValue('{s}')"),
            Self::Path { value, format } => {
                format!(
                    "ExprValue('{value}', type='path', path_format=PathFormat.{})",
                    match format {
                        PathFormat::Posix => "POSIX",
                        PathFormat::Windows => "WINDOWS",
                        PathFormat::Uri => "URI",
                    }
                )
            }
            Self::RangeExpr(r) => format!("ExprValue('{}', type='range_expr')", r),
            Self::Unresolved(t) => format!("ExprValue.unresolved(ExprType(\"{t}\"))"),
            val if val.is_list() => {
                let type_str = val.expr_type().to_string();
                // Find path format if any
                let pf = val.find_path_format();
                let pf_str = pf
                    .map(|f| {
                        format!(
                            ", path_format=PathFormat.{}",
                            match f {
                                PathFormat::Posix => "POSIX",
                                PathFormat::Windows => "WINDOWS",
                                PathFormat::Uri => "URI",
                            }
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "ExprValue({}, type='{type_str}'{pf_str})",
                    val.repr_python_list()
                )
            }
            _ => format!("ExprValue('{}')", self.to_display_string()),
        }
    }

    fn repr_python_list(&self) -> String {
        let elements = self.list_elements().unwrap_or_default();
        let items: Vec<String> = elements
            .iter()
            .map(|e| {
                if e.is_list() {
                    e.repr_python_list()
                } else {
                    match e {
                        ExprValue::String(s) | ExprValue::Path { value: s, .. } => format!("'{s}'"),
                        ExprValue::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                        ExprValue::Int(i) => i.to_string(),
                        ExprValue::Float(f) => f.to_display_string(),
                        _ => e.to_display_string(),
                    }
                }
            })
            .collect();
        format!("[{}]", items.join(", "))
    }

    fn find_path_format(&self) -> Option<PathFormat> {
        match self {
            Self::ListPath(_, fmt, _) => Some(*fmt),
            Self::ListList(v, _, _) => v.first().and_then(|e| e.find_path_format()),
            _ => None,
        }
    }

    /// Serialize to JSON transport format: `{"type": "int", "value": "42"}`.
    /// Lists serialize value as nested JSON arrays of strings.
    /// The caller adds the `"name"` field.
    pub fn to_json_transport(&self) -> serde_json::Value {
        let type_str = self.expr_type().to_string();
        let value = self.transport_value();
        serde_json::json!({"type": type_str, "value": value})
    }

    pub fn transport_value(&self) -> serde_json::Value {
        match self {
            val if val.is_list() => {
                let elements = val.list_elements().unwrap_or_default();
                serde_json::Value::Array(elements.iter().map(|e| e.transport_value()).collect())
            }
            _ => serde_json::Value::String(self.to_display_string()),
        }
    }

    /// Deserialize from JSON transport format.
    /// `json` must have `"type"` and `"value"` fields.
    pub fn from_json_transport(
        json: &serde_json::Value,
        path_format: PathFormat,
    ) -> Result<Self, String> {
        let type_str = json
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'type' field")?;
        let value = json.get("value").ok_or("Missing 'value' field")?;
        let expr_type = ExprType::parse(type_str)?;
        Self::from_transport_value(value, &expr_type, path_format)
    }

    pub fn from_transport_value(
        value: &serde_json::Value,
        target: &ExprType,
        path_format: PathFormat,
    ) -> Result<Self, String> {
        Self::from_transport_value_inner(value, target, path_format, 0)
    }

    fn from_transport_value_inner(
        value: &serde_json::Value,
        target: &ExprType,
        path_format: PathFormat,
        depth: usize,
    ) -> Result<Self, String> {
        if depth > 10 {
            return Err("Transport value nesting depth exceeded".to_string());
        }
        if target.code() == TypeCode::List {
            let elem_type = target
                .params()
                .first()
                .ok_or("List type missing element type")?;
            let arr = value.as_array().ok_or("Expected array for list type")?;
            let elements: Result<Vec<_>, _> = arr
                .iter()
                .map(|v| Self::from_transport_value_inner(v, elem_type, path_format, depth + 1))
                .collect();
            return ExprValue::make_list(elements?, elem_type.clone()).map_err(|e| e.to_string());
        }
        let s = value
            .as_str()
            .ok_or_else(|| format!("Expected string value for {target}"))?;
        ExprValue::from_str_coerce(s, target, path_format)
    }

    /// Returns `true` if this value is a list variant.
    pub fn is_list(&self) -> bool {
        matches!(
            self,
            Self::ListBool(_)
                | Self::ListInt(_)
                | Self::ListFloat(_)
                | Self::ListString(_, _)
                | Self::ListPath(_, _, _)
                | Self::ListList(_, _, _)
        )
    }

    /// Number of elements if this is a list, `None` otherwise.
    pub fn list_len(&self) -> Option<usize> {
        match self {
            Self::ListBool(v) => Some(v.len()),
            Self::ListInt(v) => Some(v.len()),
            Self::ListFloat(v) => Some(v.len()),
            Self::ListString(v, _) => Some(v.len()),
            Self::ListPath(v, _, _) => Some(v.len()),
            Self::ListList(v, _, _) => Some(v.len()),
            _ => None,
        }
    }

    /// Collect all elements into a `Vec`. Prefer [`list_iter`](Self::list_iter) to avoid allocation.
    pub fn list_elements(&self) -> Option<Vec<ExprValue>> {
        match self {
            Self::ListBool(v) => Some(v.iter().map(|b| ExprValue::Bool(*b)).collect()),
            Self::ListInt(v) => Some(v.iter().map(|i| ExprValue::Int(*i)).collect()),
            Self::ListFloat(v) => Some(v.iter().map(|f| ExprValue::Float(f.clone())).collect()),
            Self::ListString(v, _) => {
                Some(v.iter().map(|s| ExprValue::String(s.clone())).collect())
            }
            Self::ListPath(v, fmt, _) => Some(
                v.iter()
                    .map(|s| ExprValue::new_path(s.clone(), *fmt))
                    .collect(),
            ),
            Self::ListList(v, _, _) => Some(v.clone()),
            _ => None,
        }
    }

    /// Iterate over list elements without allocating a Vec.
    /// Returns None for non-list values.
    pub fn list_iter(&self) -> Option<ListIter<'_>> {
        match self {
            Self::ListBool(v) => Some(ListIter::Bool(v.iter())),
            Self::ListInt(v) => Some(ListIter::Int(v.iter())),
            Self::ListFloat(v) => Some(ListIter::Float(v.iter())),
            Self::ListString(v, _) => Some(ListIter::String(v.iter())),
            Self::ListPath(v, fmt, _) => Some(ListIter::Path(v.iter(), *fmt)),
            Self::ListList(v, _, _) => Some(ListIter::List(v.iter())),
            _ => None,
        }
    }

    /// Get a single element by index without allocating.
    /// Supports negative indexing (Python-style).
    pub fn list_get(&self, index: i64) -> Option<ExprValue> {
        let len = self.list_len()? as i64;
        let i = if index < 0 { len + index } else { index };
        if i < 0 || i >= len {
            return None;
        }
        let i = i as usize;
        match self {
            Self::ListBool(v) => Some(ExprValue::Bool(v[i])),
            Self::ListInt(v) => Some(ExprValue::Int(v[i])),
            Self::ListFloat(v) => Some(ExprValue::Float(v[i].clone())),
            Self::ListString(v, _) => Some(ExprValue::String(v[i].clone())),
            Self::ListPath(v, fmt, _) => Some(ExprValue::new_path(v[i].clone(), *fmt)),
            Self::ListList(v, _, _) => Some(v[i].clone()),
            _ => None,
        }
    }

    /// Element type of a list, or `None` for non-list values.
    ///
    /// Returns the element type based on the list variant, even for empty
    /// lists. For example, an empty `ListString` returns `STRING`, not
    /// `NULLTYPE`. This ensures that operations on empty typed lists
    /// (e.g. `sorted([])` where `[]` was originally `list[string]`)
    /// preserve the element type through round-trips via `into_list` +
    /// `make_list`.
    pub fn list_elem_type(&self) -> Option<ExprType> {
        match self {
            Self::ListBool(_) => Some(ExprType::BOOL),
            Self::ListInt(_) => Some(ExprType::INT),
            Self::ListFloat(_) => Some(ExprType::FLOAT),
            Self::ListString(_, _) => Some(ExprType::STRING),
            Self::ListPath(_, _, _) => Some(ExprType::PATH),
            Self::ListList(_, elem_type, _) => Some(elem_type.clone()),
            _ => None,
        }
    }

    /// Destructure into (elements, elem_type) for migration compatibility.
    pub fn into_list(self) -> Option<(Vec<ExprValue>, ExprType)> {
        let et = self.list_elem_type()?;
        Some((self.list_elements()?, et))
    }

    /// The [`ExprType`] of this value.
    pub fn expr_type(&self) -> ExprType {
        match self {
            Self::Null => ExprType::NULLTYPE,
            Self::Bool(_) => ExprType::BOOL,
            Self::Int(_) => ExprType::INT,
            Self::Float(_) => ExprType::FLOAT,
            Self::String(_) => ExprType::STRING,
            Self::Path { .. } => ExprType::PATH,
            Self::ListBool(_) => ExprType::list(ExprType::BOOL),
            Self::ListInt(_) => ExprType::list(ExprType::INT),
            Self::ListFloat(_) => ExprType::list(ExprType::FLOAT),
            Self::ListString(_, _) => ExprType::list(ExprType::STRING),
            Self::ListPath(_, _, _) => ExprType::list(ExprType::PATH),
            Self::ListList(_, elem_type, _) => ExprType::list(elem_type.clone()),
            Self::RangeExpr(_) => ExprType::RANGE_EXPR,
            Self::Unresolved(t) => ExprType::unresolved(t.clone()),
        }
    }

    /// Get a string representation for use in path manipulation and constraint checking.
    /// Returns a `Cow` to avoid allocation when the value is already a string.
    pub fn as_str_repr(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::String(s) => std::borrow::Cow::Borrowed(s),
            Self::Path { value, .. } => std::borrow::Cow::Borrowed(value),
            _ => std::borrow::Cow::Owned(self.to_display_string()),
        }
    }

    /// Short type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Path { .. } => "path",
            Self::RangeExpr(_) => "range_expr",
            Self::Unresolved(_) => "unresolved",
            _ if self.is_list() => "list",
            _ => "unknown",
        }
    }

    /// Human-readable string for format string interpolation and display.
    pub fn to_display_string(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(fv) => fv.to_display_string(),
            Self::String(s) => s.clone(),
            Self::Path { value, .. } => value.clone(),
            Self::ListBool(v) => format!(
                "[{}]",
                v.iter()
                    .map(|b| if *b { "true" } else { "false" })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ListInt(v) => format!(
                "[{}]",
                v.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ListFloat(v) => format!(
                "[{}]",
                v.iter()
                    .map(|f| f.to_display_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ListString(v, _) => format!(
                "[{}]",
                v.iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ListPath(v, _, _) => format!(
                "[{}]",
                v.iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ListList(v, _, _) => format!(
                "[{}]",
                v.iter()
                    .map(|e| e.to_display_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::RangeExpr(r) => r.to_string(),
            Self::Unresolved(t) => format!("<unresolved[{t}]>"),
        }
    }

    /// Memory size: `size_of::<ExprValue>` (the enum itself) plus heap allocations.
    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<ExprValue>() + self.heap_size()
    }

    /// Heap-only allocation size (excludes the inline ExprValue struct).
    fn heap_size(&self) -> usize {
        use std::mem::size_of;
        match self {
            Self::Null | Self::Bool(_) | Self::Int(_) | Self::Unresolved(_) => 0,
            Self::Float(f) => f.original.as_ref().map_or(0, |s| s.len()),
            Self::String(s) | Self::Path { value: s, .. } => s.capacity(),
            Self::ListBool(v) => v.capacity(),
            Self::ListInt(v) => v.capacity() * size_of::<i64>(),
            Self::ListFloat(v) => v.capacity() * size_of::<Float64>(),
            Self::ListString(_, cached) | Self::ListPath(_, _, cached) => *cached,
            Self::ListList(_, _, cached) => *cached,
            Self::RangeExpr(r) => r.heap_size(),
        }
    }

    /// Value equality with cross-type support (Int↔Float, String↔Path).
    pub fn equals(&self, other: &ExprValue) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.value == b.value,
            (Self::Int(a), Self::Float(b)) => (*a as f64) == b.value,
            (Self::Float(a), Self::Int(b)) => a.value == (*b as f64),
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Path { value: a, .. }, Self::Path { value: b, .. }) => a == b,
            (Self::String(a), Self::Path { value: b, .. })
            | (Self::Path { value: b, .. }, Self::String(a)) => a == b,
            _ if self.is_list() && other.is_list() => {
                let (a_iter, b_iter) = match (self.list_iter(), other.list_iter()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return false,
                };
                let (a_len, b_len) = (a_iter.len(), b_iter.len());
                if a_len != b_len {
                    return false;
                }
                a_iter.zip(b_iter).all(|(x, y)| x.equals(&y))
            }
            (Self::ListInt(elems), Self::RangeExpr(r))
            | (Self::RangeExpr(r), Self::ListInt(elems)) => {
                let rv: Vec<i64> = r.iter().collect();
                elems.len() == rv.len() && elems.iter().zip(rv.iter()).all(|(a, b)| a == b)
            }
            (Self::RangeExpr(a), Self::RangeExpr(b)) => a == b,
            (Self::Unresolved(a), Self::Unresolved(b)) => a == b,
            _ => false,
        }
    }

    /// Ordering comparison. Returns `Err` for incomparable types.
    pub fn compare(
        &self,
        other: &ExprValue,
    ) -> Result<std::cmp::Ordering, crate::error::ExpressionError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(a.cmp(b)),
            (Self::Float(a), Self::Float(b)) => a
                .value
                .partial_cmp(&b.value)
                .ok_or_else(|| crate::error::ExpressionError::new("Cannot compare NaN")),
            (Self::Int(a), Self::Float(b)) => (*a as f64)
                .partial_cmp(&b.value)
                .ok_or_else(|| crate::error::ExpressionError::new("Cannot compare NaN")),
            (Self::Float(a), Self::Int(b)) => a
                .value
                .partial_cmp(&(*b as f64))
                .ok_or_else(|| crate::error::ExpressionError::new("Cannot compare NaN")),
            (Self::Bool(a), Self::Bool(b)) => Ok(a.cmp(b)),
            (Self::String(a), Self::String(b)) => Ok(a.cmp(b)),
            (Self::Path { value: a, .. }, Self::Path { value: b, .. }) => Ok(a.cmp(b)),
            (Self::String(a), Self::Path { value: b, .. })
            | (Self::Path { value: b, .. }, Self::String(a)) => Ok(a.cmp(b)),
            _ if self.is_list() && other.is_list() => {
                let (a_iter, b_iter) = match (self.list_iter(), other.list_iter()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => {
                        return Err(crate::error::ExpressionError::new(format!(
                            "Cannot compare {} and {}",
                            self.expr_type(),
                            other.expr_type()
                        )))
                    }
                };
                let (a_len, b_len) = (a_iter.len(), b_iter.len());
                for (x, y) in a_iter.zip(b_iter) {
                    match x.compare(&y) {
                        Ok(std::cmp::Ordering::Equal) => continue,
                        other => return other,
                    }
                }
                Ok(a_len.cmp(&b_len))
            }
            _ => Err(crate::error::ExpressionError::new(format!(
                "Cannot compare {} and {}",
                self.expr_type(),
                other.expr_type()
            ))),
        }
    }
}

impl PartialEq for ExprValue {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl From<bool> for ExprValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<i32> for ExprValue {
    fn from(v: i32) -> Self {
        Self::Int(v as i64)
    }
}
impl From<i64> for ExprValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<String> for ExprValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<&str> for ExprValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}
impl From<RangeExpr> for ExprValue {
    fn from(v: RangeExpr) -> Self {
        Self::RangeExpr(v)
    }
}
impl From<crate::types::ExprType> for ExprValue {
    fn from(t: crate::types::ExprType) -> Self {
        Self::Unresolved(t)
    }
}

/// Zero-allocation iterator over list elements.
pub enum ListIter<'a> {
    Bool(std::slice::Iter<'a, bool>),
    Int(std::slice::Iter<'a, i64>),
    Float(std::slice::Iter<'a, Float64>),
    String(std::slice::Iter<'a, String>),
    Path(std::slice::Iter<'a, String>, PathFormat),
    List(std::slice::Iter<'a, ExprValue>),
}

impl<'a> Iterator for ListIter<'a> {
    type Item = ExprValue;
    fn next(&mut self) -> Option<ExprValue> {
        match self {
            Self::Bool(it) => it.next().map(|b| ExprValue::Bool(*b)),
            Self::Int(it) => it.next().map(|i| ExprValue::Int(*i)),
            Self::Float(it) => it.next().map(|f| ExprValue::Float(f.clone())),
            Self::String(it) => it.next().map(|s| ExprValue::String(s.clone())),
            Self::Path(it, fmt) => it.next().map(|s| ExprValue::new_path(s.clone(), *fmt)),
            Self::List(it) => it.next().cloned(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Bool(it) => it.size_hint(),
            Self::Int(it) => it.size_hint(),
            Self::Float(it) => it.size_hint(),
            Self::String(it) => it.size_hint(),
            Self::Path(it, _) => it.size_hint(),
            Self::List(it) => it.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ListIter<'a> {}

pub fn format_float(f: f64) -> String {
    if f == 0.0 {
        return "0.0".to_string();
    }
    let abs = f.abs();
    if !(1e-4..1e16).contains(&abs) {
        format!("{:e}", f)
            .replace("e-0", "e-")
            .replace("e0", "e+0")
            .replace("e", "e+")
            .replace("e+-", "e-")
            .replace("e++", "e+")
    } else if f.fract() == 0.0 {
        format!("{}.0", f as i64)
    } else {
        f.to_string()
    }
}

/// Normalize path separators to match `format`.
///
/// - `Posix`: no normalization — backslashes are valid filename characters
/// - `Windows`: `/` → `\` (unless the value is a URI)
/// - `Uri`: no normalization
#[must_use]
pub fn normalize_path_separators(value: &str, format: PathFormat) -> String {
    if crate::uri_path::is_uri(value) {
        return value.to_string();
    }
    match format {
        PathFormat::Windows => value.replace('/', "\\"),
        PathFormat::Posix | PathFormat::Uri => value.to_string(),
    }
}
