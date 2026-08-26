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

/// The exact i64 value of `v`, or `None` if `v` is not an integer exactly
/// representable in i64. NaN and infinities fail the `fract()` test.
pub(crate) fn float_as_exact_i64(v: f64) -> Option<i64> {
    (v.fract() == 0.0 && float_fits_i64(v)).then_some(v as i64)
}

/// Budgeted equality for same-variant typed lists: O(1) length check,
/// then per-element comparison with one charge per comparison performed,
/// so an early mismatch costs the steps taken rather than the length.
fn primitive_lists_eq<T, E>(
    a: &[T],
    b: &[T],
    charge: &mut dyn FnMut(usize) -> Result<(), E>,
    eq: impl Fn(&T, &T) -> bool,
) -> Result<bool, E> {
    if a.len() != b.len() {
        return Ok(false);
    }
    for (x, y) in a.iter().zip(b.iter()) {
        charge(1)?;
        if !eq(x, y) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Exact int↔float equality, matching Python.
///
/// A plain `(i as f64) == v` comparison rounds `i` to the nearest f64
/// first, so near the 2^63 boundary distinct integers compare equal to
/// the same float: `i64::MAX as f64` is exactly 2^63, which is NOT the
/// value of `i64::MAX`, yet `(i64::MAX as f64) == 2^63 as f64` is true.
/// Python compares exactly (`float(2**63) == 2**63 - 1` is False). An
/// integral f64 in the i64 domain converts exactly, so comparing the
/// converted value in the integer domain gives the exact answer.
pub(crate) fn int_float_eq(i: i64, v: f64) -> bool {
    float_as_exact_i64(v) == Some(i)
}

/// Exact int↔float ordering (`i` vs `v`), matching Python; `None` for NaN.
///
/// Ordering must agree with [`int_float_eq`]: with the rounding
/// comparison, `i64::MAX < 2^63 as f64` was false (the int rounds up to
/// exactly 2^63) while equality correctly said they differ — an
/// inconsistent order. Compare in the widened i128/f64 domains instead:
/// out-of-i64-domain floats order by sign against every int, and
/// in-domain floats compare exactly via truncation plus a fractional
/// tie-break.
pub(crate) fn int_float_cmp(i: i64, v: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if v.is_nan() {
        return None;
    }
    // Every i64 lies in [-2^63, 2^63); a float at or beyond the bounds
    // orders strictly by sign. (-(i64::MIN as f64) is exactly 2^63.)
    if v >= -(i64::MIN as f64) {
        return Some(Ordering::Less); // i < v (covers +inf)
    }
    if v < i64::MIN as f64 {
        return Some(Ordering::Greater); // i > v (covers -inf)
    }
    // In-domain: truncation is exact (|v| < 2^63), so compare the integer
    // parts, breaking ties on the fractional remainder.
    let vt = v.trunc() as i64;
    Some(i.cmp(&vt).then_with(|| {
        let frac = v - v.trunc();
        // i == trunc(v): a positive remainder means v is bigger, a
        // negative remainder means v is smaller.
        if frac > 0.0 {
            Ordering::Less
        } else if frac < 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }))
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
    /// Byte length of the preserved display representation without allocating.
    pub(crate) fn display_len(&self) -> usize {
        self.original.as_ref().map_or_else(
            || {
                // `format_float` uses Rust's shortest finite-f64 rendering;
                // 32 bytes is a conservative upper bound including sign,
                // decimal point, and exponent.
                32
            },
            |s| s.len(),
        )
    }

    /// Borrow the preserved display representation, formatting only when needed.
    pub(crate) fn display_cow(&self) -> std::borrow::Cow<'_, str> {
        self.original.as_ref().map_or_else(
            || std::borrow::Cow::Owned(format_float(self.value)),
            |s| std::borrow::Cow::Borrowed(s.as_ref()),
        )
    }

    /// Display string: the original literal if preserved, otherwise formatted.
    pub fn to_display_string(&self) -> String {
        self.display_cow().into_owned()
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
            // Float hashes as Int when it's an exact integer in i64 range
            // (same rule as int↔float equality), otherwise uses float tag
            // + f64 bits.
            Self::Float(f) => {
                let v = f.value;
                if let Some(i) = float_as_exact_i64(v) {
                    2u8.hash(state);
                    i.hash(state);
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
                    // Same exact rule as the top-level Float arm: an
                    // integral in-i64-range float hashes as its integer.
                    if let Some(i) = float_as_exact_i64(fv) {
                        2u8.hash(state);
                        i.hash(state);
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
    /// Coercion answers two questions in order, and keeping them separate is
    /// what makes abstract targets (`any`, unions) well defined:
    ///
    /// 1. **Satisfaction** — does the value's type already satisfy the
    ///    target, per [`ExprType::satisfies`]? Then return it **unchanged**.
    ///    This covers `int` against `int | string`, `list[int]` against
    ///    `list[any]`, and every value against `any`. RFC 0005 §"Implicit
    ///    Type Coercion" calls for coercion only "where the intent is
    ///    obvious"; a value that is already acceptable needs none.
    /// 2. **Conversion** — otherwise apply the non-destructive conversion
    ///    table. A conversion needs a single concrete type to produce, so
    ///    the target is first decomposed into its candidate destinations;
    ///    the first destination that converts wins.
    ///
    /// Conversion is non-destructive: only conversions that don't lose
    /// information are attempted (`int → float`, `int → string`, etc).
    ///
    /// Because step 1 returns the value unchanged, the result's type is the
    /// *source* type rather than the target — coercing a `list[int]` to
    /// `list[any]` yields a `list[int]`, not a `list[any]`. In both steps the
    /// result is guaranteed to satisfy the target.
    ///
    /// Unresolved values have no payload to inspect, so they run the same two
    /// steps at the type level and return an unresolved value constrained to
    /// the result type. Unbound type variables in the constraint read as
    /// wildcards — a concrete value can never have one in its type — so
    /// `unresolved[list[T1]]` coerces exactly as `unresolved[list[any]]`.
    /// Checks that need a
    /// concrete payload, such as parsing a string as an integer, are deferred
    /// until resolution. The type-level table may therefore accept a pair
    /// that the concrete value later rejects, but it must never reject one
    /// the concrete value would accept — that would fail a template during
    /// validation that would have run correctly. This invariant is enforced
    /// mechanically by `tests/integration/test_coercion_drift.rs`, which
    /// sweeps sample values against target types and checks the two paths
    /// agree.
    pub fn coerce(self, target: &ExprType, path_format: PathFormat) -> Result<Self, String> {
        // Unresolved values carry a type constraint and nothing else, so the
        // decision has to be made entirely at the type level.
        if let ExprValue::Unresolved(constraint) = &self {
            return match Self::coerce_type(constraint, target) {
                Some(result_type) => Ok(ExprValue::unresolved(result_type)),
                None => Err(format!("Cannot coerce {constraint} to {target}")),
            };
        }
        // Step 1: already acceptable, so pass through untouched.
        let source_type = self.expr_type();
        if source_type.satisfies(target) {
            return Ok(self);
        }
        // Step 2: convert toward a candidate destination.
        let destinations = Self::conversion_destinations(&source_type, target);
        // A destination equal to the whole target — every non-union target —
        // is the only rule the caller could have meant, so convert directly
        // without a clone and keep the rule's specific diagnostic (for
        // example the range_expr and integer-overflow messages).
        if let [only] = destinations[..] {
            if only == target {
                return self.convert_to(only, path_format);
            }
        }
        // Union target: the first destination that converts wins, and a
        // failure along the way is not an error as long as a later
        // destination succeeds. No individual diagnostic survives, since
        // naming one member would misreport the target the caller asked for.
        for dest in destinations {
            if let Ok(converted) = self.clone().convert_to(dest, path_format) {
                return Ok(converted);
            }
        }
        Err(format!("Cannot coerce {source_type} to {target}"))
    }

    /// The types a value of type `source` may be converted *to* in order to
    /// satisfy `target`, in the order they should be tried.
    ///
    /// A union contributes each of its members, because converting to any one
    /// of them satisfies the union. Members are ordered **non-list before
    /// list** — converting to a scalar produces a single value whose cost does
    /// not depend on the source, while converting to a list materializes
    /// elements and can fail on size limits — so a `range_expr` against
    /// `list[int] | string` becomes the `string` its canonical form already
    /// is, rather than expanding the range. Within each group the order is the
    /// source type's preference, per [`Self::destination_rank`], with the
    /// union's normalized order as the tie-break.
    ///
    /// Types that denote no runtime values — type variables,
    /// `noreturn`, `unresolved[T]`, signatures, and lists parameterized by
    /// any of them — contribute nothing, so a target made only of those
    /// yields no destinations and coercion fails. `nulltype` likewise
    /// contributes nothing: nothing coerces *into* `null`.
    fn conversion_destinations<'t>(source: &ExprType, target: &'t ExprType) -> Vec<&'t ExprType> {
        if target.code() != TypeCode::Union {
            return if Self::is_conversion_destination(target) {
                vec![target]
            } else {
                Vec::new()
            };
        }
        let (mut scalars, mut lists): (Vec<&ExprType>, Vec<&ExprType>) = target
            .params()
            .iter()
            .filter(|m| Self::is_conversion_destination(m))
            .partition(|m| m.code() != TypeCode::List);
        // Stable sorts, so equally ranked destinations keep the union's
        // normalized order.
        scalars.sort_by_key(|d| Self::destination_rank(source, d));
        lists.sort_by_key(|d| Self::destination_rank(source, d));
        scalars.extend(lists);
        scalars
    }

    /// The scalar conversion rules, in one table with the source type's
    /// preference among them: `Some(rank)` when a `source → dest` conversion
    /// rule exists (lower ranks are tried first), `None` when no rule does
    /// (RFC 0005 §"Implicit Type Coercion").
    ///
    /// This is the single source of truth for *which* scalar pairs convert
    /// and *in what order* — [`Self::destination_rank`] orders destinations
    /// by it and [`Self::convert_type`] tests rule existence with it, so a
    /// new rule cannot gain one without the other. Only [`Self::convert_to`]
    /// repeats the pairs, in its match arms, because it needs the payloads;
    /// `tests/integration/test_coercion_drift.rs` checks that copy against
    /// this one.
    ///
    /// The order is the destination table in RFC 0005 §"Implicit Type
    /// Coercion", restated in `specs/expr/values.md` § Destinations: a value
    /// prefers to stay within its own kind, and a conversion that can fail
    /// is tried before one that always succeeds.
    ///
    /// There is deliberately no `string → nulltype` rule: turning the text
    /// `"null"` into `null` is a transport-decode rule belonging to
    /// [`Self::from_str_coerce`], not an implicit coercion (RFC 0005).
    ///
    /// # Example
    ///
    /// Coercing the string `"7"` to the union target `path | float | int`
    /// ranks the members
    ///
    /// ```text
    /// scalar_conversion_rank(String, Path)  == Some(4)
    /// scalar_conversion_rank(String, Float) == Some(1)
    /// scalar_conversion_rank(String, Int)   == Some(0)
    /// ```
    ///
    /// so conversion tries `int`, then `float`, then `path`, and `"7"`
    /// becomes the int `7`. The same target given `"7.5"` fails the `int`
    /// parse and becomes the float `7.5`; given `"out/"` it falls through
    /// to the path `out/`.
    fn scalar_conversion_rank(source: TypeCode, dest: TypeCode) -> Option<u8> {
        match (source, dest) {
            (TypeCode::Int, TypeCode::Float)
            | (TypeCode::Float, TypeCode::Int)
            | (TypeCode::String, TypeCode::Int) => Some(0),
            // `bool`, `path`, and `range_expr` each have `string` as their
            // only destination, so their rank is never compared against a
            // sibling; 1 groups them with the other to-string rules.
            (
                TypeCode::Bool
                | TypeCode::Int
                | TypeCode::Float
                | TypeCode::Path
                | TypeCode::RangeExpr,
                TypeCode::String,
            )
            | (TypeCode::String, TypeCode::Float) => Some(1),
            (TypeCode::String, TypeCode::Bool) => Some(2),
            (TypeCode::String, TypeCode::RangeExpr) => Some(3),
            (TypeCode::String, TypeCode::Path) => Some(4),
            _ => None,
        }
    }

    /// The source type's preference among conversion destinations of the
    /// same shape, per [`Self::scalar_conversion_rank`]: lower ranks are
    /// tried first.
    ///
    /// A list source ranks list destinations by its element type's preference,
    /// recursively, so `list[float]` tries `list[int]` before `list[string]`.
    /// Pairs with no conversion rule rank last; their relative order is
    /// irrelevant because conversion fails for them anyway.
    fn destination_rank(source: &ExprType, dest: &ExprType) -> u8 {
        if source.code() == TypeCode::List && dest.code() == TypeCode::List {
            if let (Some(s), Some(d)) = (source.params().first(), dest.params().first()) {
                return Self::destination_rank(s, d);
            }
        }
        Self::scalar_conversion_rank(source.code(), dest.code()).unwrap_or(u8::MAX)
    }

    /// Whether a conversion rule could produce a value of this exact type.
    ///
    /// `nulltype` is not among them. RFC 0005 lists no `X → nulltype`
    /// conversion, and discounts `nulltype` when counting a union target's
    /// candidate scalar types, so `null` is reached only by already being
    /// `null` — which satisfaction handles. Turning the text `"null"` into
    /// `null` is a transport-decode rule belonging to
    /// [`Self::from_str_coerce`], not an implicit coercion; keeping it out
    /// here is what stops it from firing inside an unrelated union such as
    /// `int?`.
    fn is_conversion_destination(t: &ExprType) -> bool {
        match t.code() {
            TypeCode::Bool
            | TypeCode::Int
            | TypeCode::Float
            | TypeCode::String
            | TypeCode::Path
            | TypeCode::RangeExpr => true,
            // A produced list carries its element type, so that type must be
            // fully bindable: one that denotes runtime values, with no type
            // variable anywhere in it — not even inside a union member,
            // which `denotes_runtime_values` alone would tolerate.
            TypeCode::List => matches!(t.params(), [elem]
                if elem.denotes_runtime_values() && !elem.is_symbolic()),
            _ => false,
        }
    }

    /// Apply the non-destructive conversion table toward a single concrete
    /// destination.
    fn convert_to(self, dest: &ExprType, path_format: PathFormat) -> Result<Self, String> {
        match (&self, dest.code()) {
            (ExprValue::Int(i), TypeCode::Float) => {
                Ok(ExprValue::Float(Float64::new(*i as f64).unwrap()))
            }
            (ExprValue::Float(f), TypeCode::Int) => Self::convert_float_to_int(f),
            (ExprValue::Bool(b), TypeCode::String) => Ok(ExprValue::String(
                if *b { "true" } else { "false" }.to_string(),
            )),
            (ExprValue::Int(i), TypeCode::String) => Ok(ExprValue::String(i.to_string())),
            (ExprValue::Float(f), TypeCode::String) => Ok(ExprValue::String(f.to_display_string())),
            (ExprValue::String(s), _) => ExprValue::from_str_coerce(s, dest, path_format),
            (ExprValue::Path { value, .. }, TypeCode::String) => {
                Ok(ExprValue::String(value.clone()))
            }
            (ExprValue::RangeExpr(r), TypeCode::String) => Ok(ExprValue::String(r.to_string())),
            (ExprValue::RangeExpr(r), TypeCode::List) => Self::convert_range_expr_to_list(r, dest),
            _ if dest.code() == TypeCode::List => self.convert_list_elementwise(dest, path_format),
            _ => Err(format!("Cannot coerce {} to {dest}", self.expr_type())),
        }
    }

    /// `float → int`, only for values that are exactly integral.
    fn convert_float_to_int(f: &Float64) -> Result<Self, String> {
        let v = f.value();
        if v.fract() != 0.0 || !v.is_finite() {
            return Err(format!(
                "Cannot coerce float to int: {} is not a whole number",
                f.to_display_string()
            ));
        }
        // Range check before the cast: `as i64` silently saturates for
        // out-of-range values (e.g. 1e30 would become i64::MAX), so reject
        // them with an integer overflow error instead.
        if !float_fits_i64(v) {
            return Err("Integer overflow: result is outside the 64-bit signed range".to_string());
        }
        Ok(ExprValue::Int(v as i64))
    }

    /// `range_expr → list[int]`, the only list type a range expression
    /// implicitly coerces to (RFC 0005).
    ///
    /// Implicit rules do not chain, so the materialized `list[int]` is never
    /// widened element-wise toward the destination; templates use the
    /// explicit `list()` conversion (RFC 0006) when they want that. The
    /// destination is therefore accepted only when a `list[int]` already
    /// satisfies it, which covers `list[int]`, `list[any]`, and
    /// `list[int | string]`, and rejects `list[float]` and `list[string]`.
    fn convert_range_expr_to_list(
        r: &crate::range_expr::RangeExpr,
        dest: &ExprType,
    ) -> Result<Self, String> {
        if !ExprType::list(ExprType::INT).satisfies(dest) {
            return Err(format!(
                "Cannot coerce range_expr to {dest}: a range expression only \
                 implicitly coerces to list[int] (use list() for an explicit \
                 conversion)"
            ));
        }
        // coerce() runs outside any EvalContext (post-evaluation target-type
        // hook, public API), so no operation or memory budget applies here.
        // Cap the materialization at the evaluator's default operation limit:
        // a range longer than that could not have been built as a list
        // through any budgeted path either.
        if r.len_u64() > crate::eval::DEFAULT_OPERATION_LIMIT as u64 {
            return Err(format!(
                "Cannot coerce range_expr of {} elements to list[int] (maximum {})",
                r.len_u64(),
                crate::eval::DEFAULT_OPERATION_LIMIT
            ));
        }
        Ok(ExprValue::ListInt(r.to_vec()))
    }

    /// `list[T] → list[U]`, coercing each element to `U`.
    ///
    /// An empty list converts to every list type, since there are no
    /// elements to check.
    fn convert_list_elementwise(
        self,
        dest: &ExprType,
        path_format: PathFormat,
    ) -> Result<Self, String> {
        let elem_type = &dest.params()[0];
        let Some(elements) = self.list_elements() else {
            return Err(format!("Cannot coerce {} to {dest}", self.expr_type()));
        };
        let coerced: Result<Vec<_>, _> = elements
            .into_iter()
            .map(|e| e.coerce(elem_type, path_format))
            .collect();
        ExprValue::make_list(coerced?, elem_type.clone()).map_err(|e| e.to_string())
    }

    /// The type-level counterpart of [`Self::coerce`], for unresolved values.
    ///
    /// Returns the type the coerced value would have, or `None` if no rule
    /// applies. Mirrors `coerce`'s two steps: satisfaction returns the
    /// **source** type (the value would pass through unchanged), conversion
    /// returns the union of the types every applicable destination rule
    /// produces — the payload decides which one wins at runtime, so the
    /// result is a sound over-approximation of the concrete result's type.
    /// The mirror is maintained by hand;
    /// `tests/integration/test_coercion_drift.rs` checks it stays consistent
    /// with the concrete path.
    fn coerce_type(source: &ExprType, target: &ExprType) -> Option<ExprType> {
        // An unresolved constraint may itself be unresolved; the innermost
        // constraint is what the resolved value's type will be.
        if source.code() == TypeCode::Unresolved {
            return source
                .params()
                .first()
                .and_then(|inner| Self::coerce_type(inner, target));
        }
        if !target.denotes_runtime_values() {
            return None;
        }
        // A concrete value can never have a type variable in its type, so an
        // unbound variable in a source constraint reads as a wildcard: the
        // resolved value will have *some* concrete type, which is exactly
        // the set of possibilities `any` denotes. Erasing variables to `any`
        // preserves the shape around them — `list[T1]` becomes `list[any]`,
        // still a list, so only list rules apply — and the rules below need
        // no case for variables at all. Such constraints arise from generic
        // return types whose variable no parameter binds, e.g. list
        // concatenation's `(list[T1], list[T2]) -> list[T3]`.
        let erased;
        let source = if source.is_symbolic() {
            erased = source.erase_type_vars();
            &erased
        } else {
            source
        };
        // A source of wholly unknown type could resolve to anything, so no
        // pair can be ruled out and nothing narrower than the target can be
        // promised. Promise only the members a value could land in, though:
        // an unusable union member (`T1` in `int | T1`) denotes no runtime
        // values, and copying it would produce a constraint that does not
        // satisfy the target.
        if source.code() == TypeCode::Any {
            if target.code() == TypeCode::Union {
                let usable: Vec<_> = target
                    .params()
                    .iter()
                    .filter(|m| m.denotes_runtime_values())
                    .cloned()
                    .collect();
                return Some(ExprType::union(usable));
            }
            return Some(target.clone());
        }
        // A union source resolves to exactly one of its members. Keep the
        // members that have a rule and union their results; dropping the
        // rest is what lets `unresolved[int | list[int]]` coerce to `int`.
        if source.code() == TypeCode::Union {
            let result_types: Vec<_> = source
                .params()
                .iter()
                .filter_map(|member| Self::coerce_type(member, target))
                .collect();
            return if result_types.is_empty() {
                None
            } else {
                Some(ExprType::union(result_types))
            };
        }
        // Step 1: satisfaction — the value passes through unchanged, so the
        // result keeps the source type rather than widening to the target.
        if source.satisfies(target) {
            return Some(source.clone());
        }
        // Step 2: conversion. The concrete path picks the first destination
        // its payload converts to; without the payload no single destination
        // can be promised, so the result is the union of every destination
        // with a type-level rule. Anything narrower would misdescribe the
        // resolved value: a `float` against `int | string` lands on `int` or
        // `string` depending on whether the payload is a whole number.
        let result_types: Vec<_> = Self::conversion_destinations(source, target)
            .into_iter()
            .filter_map(|dest| Self::convert_type(source, dest))
            .collect();
        if result_types.is_empty() {
            None
        } else {
            Some(ExprType::union(result_types))
        }
    }

    /// The type-level counterpart of [`Self::convert_to`]: the type that
    /// converting a `source` value toward `dest` would produce, or `None` if
    /// no rule applies.
    fn convert_type(source: &ExprType, dest: &ExprType) -> Option<ExprType> {
        if Self::scalar_conversion_rank(source.code(), dest.code()).is_some() {
            return Some(dest.clone());
        }
        // `range_expr → list[int]` on the same condition as the concrete
        // path, and producing the same `list[int]` it produces — not the
        // destination, which may be wider.
        if source.code() == TypeCode::RangeExpr && dest.code() == TypeCode::List {
            let list_int = ExprType::list(ExprType::INT);
            return list_int.satisfies(dest).then_some(list_int);
        }
        // Every well-formed list/list pair is accepted: an
        // `unresolved[list[S]]` could resolve to the empty list, which
        // converts to any `list[U]` element-wise over zero elements.
        // Rejecting here would fail a template during validation that could
        // have run correctly, so the element compatibility check waits until
        // the payload is known. The arity check guards against malformed
        // sources built via `ExprType::new` with zero or several parameters:
        // no concrete value has such a type, so the empty-list argument does
        // not apply to them and rejecting is safe. (The destination's arity
        // is already enforced by `is_conversion_destination`.)
        if source.code() == TypeCode::List
            && source.params().len() == 1
            && dest.code() == TypeCode::List
        {
            return Some(dest.clone());
        }
        None
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
    ///
    /// Lists render as JSON arrays: element strings and paths are quoted and
    /// JSON-escaped, with non-ASCII characters preserved verbatim. See
    /// `specs/expr/values.md` ("List display strings").
    pub fn to_display_string(&self) -> String {
        let mut buf = String::new();
        self.write_display(&mut buf);
        buf
    }

    /// Append [`Self::to_display_string`] to `buf` without an intermediate
    /// allocation. Single source of truth for the display form; `functions/repr.rs`
    /// uses it for the nested-list case of the shell reprs.
    pub(crate) fn write_display(&self, buf: &mut String) {
        use std::fmt::Write;
        match self {
            Self::Null => buf.push_str("null"),
            Self::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
            Self::Int(i) => {
                let _ = write!(buf, "{i}");
            }
            Self::Float(fv) => buf.push_str(&fv.display_cow()),
            Self::String(s) => buf.push_str(s),
            Self::Path { value, .. } => buf.push_str(value),
            Self::ListBool(v) => {
                write_display_list(buf, v.iter(), |buf, b| {
                    buf.push_str(if *b { "true" } else { "false" })
                });
            }
            Self::ListInt(v) => {
                write_display_list(buf, v.iter(), |buf, i| {
                    let _ = write!(buf, "{i}");
                });
            }
            Self::ListFloat(v) => {
                write_display_list(buf, v.iter(), |buf, f| buf.push_str(&f.display_cow()));
            }
            Self::ListString(v, _) | Self::ListPath(v, _, _) => {
                write_display_list(buf, v.iter(), |buf, s| {
                    buf.push('"');
                    crate::json_escape::write_escaped(s, buf);
                    buf.push('"');
                });
            }
            Self::ListList(v, _, _) => {
                write_display_list(buf, v.iter(), |buf, e| e.write_display(buf));
            }
            Self::RangeExpr(r) => {
                let _ = write!(buf, "{r}");
            }
            Self::Unresolved(t) => {
                let _ = write!(buf, "<unresolved[{t}]>");
            }
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
        match self.equals_charged(other, &mut |_| Ok::<(), std::convert::Infallible>(())) {
            Ok(eq) => eq,
            Err(e) => match e {},
        }
    }

    /// Core of [`equals`]: value equality with a caller-supplied `charge`
    /// callback invoked with the number of element comparisons about to be
    /// performed. The evaluator's `==`/`!=`/`in` operators pass the
    /// operation budget; `equals`/`PartialEq` pass an infallible no-op.
    ///
    /// Comparisons decidable by length alone are decided in O(1) with no
    /// charge — a symbolic range's length is pure arithmetic, so a length
    /// mismatch never requires expanding anything.
    pub(crate) fn equals_charged<E>(
        &self,
        other: &ExprValue,
        charge: &mut dyn FnMut(usize) -> Result<(), E>,
    ) -> Result<bool, E> {
        Ok(match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.value == b.value,
            (Self::Int(a), Self::Float(b)) => int_float_eq(*a, b.value),
            (Self::Float(a), Self::Int(b)) => int_float_eq(*b, a.value),
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Path { value: a, .. }, Self::Path { value: b, .. }) => a == b,
            (Self::String(a), Self::Path { value: b, .. })
            | (Self::Path { value: b, .. }, Self::String(a)) => a == b,
            // Same-variant typed lists: primitive element comparison (no
            // per-element ExprValue construction), charged per comparison
            // actually performed after the O(1) length check — a mismatch
            // at element zero costs one op, not the list length.
            (Self::ListBool(a), Self::ListBool(b)) => {
                return primitive_lists_eq(a, b, charge, |x, y| x == y);
            }
            (Self::ListInt(a), Self::ListInt(b)) => {
                return primitive_lists_eq(a, b, charge, |x, y| x == y);
            }
            (Self::ListFloat(a), Self::ListFloat(b)) => {
                return primitive_lists_eq(a, b, charge, |x, y| x.value == y.value);
            }
            (Self::ListString(a, _), Self::ListString(b, _)) => {
                return primitive_lists_eq(a, b, charge, |x, y| x == y);
            }
            _ if self.is_list() && other.is_list() => {
                let (a_iter, b_iter) = match (self.list_iter(), other.list_iter()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return Ok(false),
                };
                if a_iter.len() != b_iter.len() {
                    return Ok(false);
                }
                // Charge per comparison performed (like the typed-list
                // arms): a first-element mismatch costs one op, not the
                // list length.
                for (x, y) in a_iter.zip(b_iter) {
                    charge(1)?;
                    if !x.equals_charged(&y, charge)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Self::ListInt(elems), Self::RangeExpr(r))
            | (Self::RangeExpr(r), Self::ListInt(elems)) => {
                // O(1) uncharged length check first — len_u64() is exact
                // arithmetic under the bounded value domain — matching
                // the list arms. Then walk the range lazily (never
                // materializing it), charged per comparison performed;
                // equal lengths guarantee the zip covers both sides.
                if elems.len() as u64 != r.len_u64() {
                    return Ok(false);
                }
                for (e, rv) in elems.iter().zip(r.iter()) {
                    charge(1)?;
                    if *e != rv {
                        return Ok(false);
                    }
                }
                true
            }
            (Self::RangeExpr(a), Self::RangeExpr(b)) => a == b,
            (Self::Unresolved(a), Self::Unresolved(b)) => a == b,
            _ => false,
        })
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
            (Self::Int(a), Self::Float(b)) => int_float_cmp(*a, b.value)
                .ok_or_else(|| crate::error::ExpressionError::new("Cannot compare NaN")),
            (Self::Float(a), Self::Int(b)) => int_float_cmp(*b, a.value)
                .map(std::cmp::Ordering::reverse)
                .ok_or_else(|| crate::error::ExpressionError::new("Cannot compare NaN")),
            (Self::Bool(a), Self::Bool(b)) => Ok(a.cmp(b)),
            (Self::String(a), Self::String(b)) => Ok(a.cmp(b)),
            (Self::Path { value: a, .. }, Self::Path { value: b, .. }) => Ok(a.cmp(b)),
            (Self::String(a), Self::Path { value: b, .. }) => Ok(a.cmp(b)),
            (Self::Path { value: a, .. }, Self::String(b)) => Ok(a.cmp(b)),
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
        let rendered = format!("{f:e}");
        if let Some((mantissa, exponent)) = rendered.split_once('e') {
            if let Ok(exponent) = exponent.parse::<i32>() {
                return format!("{mantissa}e{exponent:+03}");
            }
        }
        rendered
    } else if f.fract() == 0.0 {
        format!("{}.0", f as i64)
    } else {
        f.to_string()
    }
}

/// Write `items` into `buf` as a JSON array, rendering each element with
/// `write_item`.
fn write_display_list<T>(
    buf: &mut String,
    items: impl Iterator<Item = T>,
    write_item: impl Fn(&mut String, T),
) {
    buf.push('[');
    for (i, item) in items.enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        write_item(buf, item);
    }
    buf.push(']');
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
