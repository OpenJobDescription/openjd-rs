// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Function library: signature-based multiple dispatch for expression evaluation.

use crate::error::{ExpressionError, ExpressionErrorKind};

fn friendly_op_name(name: &str) -> Option<&'static str> {
    match name {
        "__add__" => Some("+"),
        "__sub__" => Some("-"),
        "__mul__" => Some("*"),
        "__truediv__" => Some("/"),
        "__floordiv__" => Some("//"),
        "__mod__" => Some("%"),
        "__pow__" => Some("**"),
        "__neg__" => Some("-"),
        "__pos__" => Some("+"),
        "__eq__" => Some("=="),
        "__ne__" => Some("!="),
        "__lt__" => Some("<"),
        "__le__" => Some("<="),
        "__gt__" => Some(">"),
        "__ge__" => Some(">="),
        "__contains__" => Some("in"),
        "__not_contains__" => Some("not in"),
        _ => None,
    }
}

use crate::types::ExprType;
use crate::value::ExprValue;
use std::collections::HashMap;
use std::sync::Arc;

/// Type alias for a boxed function implementation.
///
/// Uses `Arc<dyn Fn>` rather than a bare `fn` pointer so that closures
/// (capturing environment) can be registered alongside plain functions.
/// `Arc` (not `Box`) keeps `FunctionLibrary` `Clone`.
pub type FunctionImpl = Arc<
    dyn Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError> + Send + Sync,
>;

/// A registered function overload.
#[derive(Clone)]
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub struct FunctionEntry {
    pub signature: ExprType, // TypeCode::Signature
    pub implementation: FunctionImpl,
}

/// Trait for the evaluator context that function implementations can access.
#[allow(
    missing_docs,
    reason = "missing-docs burn-down: predates the lint; document these items, then delete this attribute"
)]
pub trait EvalContext {
    fn path_format(&self) -> crate::path_mapping::PathFormat;
    fn count_op(&mut self) -> Result<(), ExpressionError>;
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError>;
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError>;
    /// Pre-check that an allocation of `bytes` would not exceed the memory limit.
    /// Call before large allocations to avoid temporarily exceeding the limit.
    fn check_memory(&self, bytes: usize) -> Result<(), ExpressionError>;
    fn get_or_compile_regex(&mut self, pattern: &str) -> Result<regex::Regex, ExpressionError> {
        regex::RegexBuilder::new(pattern)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| ExpressionError::new(format!("Invalid regex: {e}")))
    }
}

/// Registry of functions available in expressions.
#[derive(Clone, Default)]
pub struct FunctionLibrary {
    functions: HashMap<String, Vec<FunctionEntry>>,
}

impl FunctionLibrary {
    /// Create an empty library with no functions registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a function overload with an `ExprType::Signature`.
    ///
    /// Accepts either a bare `fn` pointer or a closure — anything implementing
    /// `Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>`
    /// with `Send + Sync + 'static`. Closures enable wrapping host state
    /// (e.g., AWS clients, config) without plumbing it through `EvalContext`.
    pub fn register<F>(&mut self, name: &str, signature: ExprType, implementation: F)
    where
        F: Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>
            + Send
            + Sync
            + 'static,
    {
        self.functions
            .entry(name.to_string())
            .or_default()
            .push(FunctionEntry {
                signature,
                implementation: Arc::new(implementation),
            });
    }

    /// Register from spec notation: `lib.register_sig("len", "(list[T1]) -> int", len_list)`
    ///
    /// Returns `Err` if `sig_str` cannot be parsed as a valid type
    /// signature. The function is **not** registered on failure.
    pub fn register_sig<F>(
        &mut self,
        name: &str,
        sig_str: &str,
        implementation: F,
    ) -> Result<(), String>
    where
        F: Fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>
            + Send
            + Sync
            + 'static,
    {
        let signature = ExprType::parse(sig_str)?;
        self.register(name, signature, implementation);
        Ok(())
    }

    /// Get all entries for a function name.
    pub fn get_signatures(&self, name: &str) -> &[FunctionEntry] {
        self.functions
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Merge another library's registrations into this one.
    #[must_use]
    pub fn merge(mut self, other: FunctionLibrary) -> Self {
        for (name, entries) in other.functions {
            self.functions.entry(name).or_default().extend(entries);
        }
        self
    }

    /// Get all registered function names.
    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(|s| s.as_str())
    }

    /// Dispatch a function call: exact → coerced → generic. With an
    /// unresolved argument the call is type-only: a list argument whose
    /// element type is a union is typed by each of its forms.
    pub fn call(
        &self,
        name: &str,
        args: &[ExprValue],
        ctx: &mut dyn EvalContext,
    ) -> Result<ExprValue, ExpressionError> {
        self.call_inner(name, args, ctx, false)
    }

    /// Dispatch a method call (skip receiver coercion on first arg).
    pub fn call_method(
        &self,
        name: &str,
        args: &[ExprValue],
        ctx: &mut dyn EvalContext,
    ) -> Result<ExprValue, ExpressionError> {
        self.call_inner(name, args, ctx, true)
    }

    fn call_inner(
        &self,
        name: &str,
        args: &[ExprValue],
        ctx: &mut dyn EvalContext,
        skip_receiver_coercion: bool,
    ) -> Result<ExprValue, ExpressionError> {
        let entries = self.get_signatures(name);
        if entries.is_empty() {
            return Err(ExpressionError::from_kind(
                ExpressionErrorKind::UnknownFunction {
                    name: name.to_string(),
                },
            ));
        }
        let arg_types: Vec<ExprType> = args
            .iter()
            .map(|a| {
                let t = a.expr_type();
                if t.code() == crate::types::TypeCode::Unresolved && !t.params().is_empty() {
                    t.params()[0].clone() // unwrap unresolved for matching
                } else {
                    t
                }
            })
            .collect();
        let any_unresolved = args.iter().any(|a| a.is_unresolved());

        // An unresolved argument makes the call type-only: its result is
        // the signature's return type. The phases are applied to the
        // types, with list arguments whose element type is a union
        // matched by their forms (`unresolved_return_type`).
        if any_unresolved {
            if let Some(ret) = unresolved_return_type(entries, &arg_types, skip_receiver_coercion) {
                let ret = concat_return_type(name, &arg_types, ret);
                return Ok(ExprValue::unresolved(ret));
            }
        }

        // Phase 1: exact match (non-generic)
        for entry in entries {
            if entry.signature.is_symbolic() {
                continue;
            }
            if !any_unresolved && entry.signature.match_call(&arg_types).is_some() {
                return (entry.implementation)(ctx, args);
            }
        }

        // Phase 2: coerced match (non-generic)
        for entry in entries {
            if entry.signature.is_symbolic() || any_unresolved {
                continue;
            }
            if let Some(coerced) = try_coerce_args(
                args,
                &arg_types,
                entry.signature.sig_params(),
                skip_receiver_coercion,
            ) {
                return (entry.implementation)(ctx, &coerced);
            }
        }

        // Phase 3: generic match
        for entry in entries {
            if !entry.signature.is_symbolic() || any_unresolved {
                continue;
            }
            if entry.signature.match_call(&arg_types).is_some() {
                return (entry.implementation)(ctx, args);
            }
            // Try with coercion for generics too
            if let Some(coerced_types) = try_coerce_types(
                &arg_types,
                entry.signature.sig_params(),
                skip_receiver_coercion,
            ) {
                if entry.signature.match_call(&coerced_types).is_some() {
                    let coerced = coerce_values(args, &coerced_types);
                    return (entry.implementation)(ctx, &coerced);
                }
            }
        }

        // Build helpful error message
        let n_args = arg_types.len();
        let valid_arities: Vec<usize> = entries
            .iter()
            .map(|e| e.signature.sig_params().len())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        // Check if it's a wrong arg count issue
        if !valid_arities.contains(&n_args) {
            let arity_str = if valid_arities.len() == 1 {
                format!("{}", valid_arities[0])
            } else {
                valid_arities
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let plural = if valid_arities.len() > 1 {
                "arguments"
            } else {
                "argument(s)"
            };
            return Err(ExpressionError::new(format!(
                "{name}() takes {arity_str} {plural}, but {n_args} were given"
            )));
        }

        // Check if it's a receiver type mismatch (method on wrong type)
        if skip_receiver_coercion && !arg_types.is_empty() {
            let receiver_type = &arg_types[0];
            let valid_types: Vec<String> = entries
                .iter()
                .filter(|e| e.signature.sig_params().len() == n_args)
                .filter_map(|e| {
                    let first = e.signature.sig_params().first()?;
                    if first.is_symbolic() {
                        None
                    } else {
                        Some(first.to_string())
                    }
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            if !valid_types.is_empty() {
                return Err(ExpressionError::new(format!(
                    "{name}() is not available for {receiver_type}. Available for: {}",
                    valid_types.join(", ")
                )));
            }
        }

        let type_strs: Vec<String> = arg_types.iter().map(|t| t.to_string()).collect();
        let display_name = friendly_op_name(name);
        // Membership is dispatched container-first, the reverse of the source
        // order, and a list-vs-list mismatch prints two identical types
        // ("list[int] and list[int]"). Name the roles instead.
        if let (Some(op @ ("in" | "not in")), [container, item]) =
            (display_name, arg_types.as_slice())
        {
            if let Some(msg) = membership_mismatch_message(op, container, item) {
                return Err(ExpressionError::new(msg));
            }
        }
        if let Some(op) = display_name {
            Err(ExpressionError::new(format!(
                "Cannot use '{}' operator with {}",
                op,
                type_strs.join(" and ")
            )))
        } else {
            Err(ExpressionError::new(format!(
                "No matching signature for {}({})",
                name,
                type_strs.join(", ")
            )))
        }
    }

    /// Derive the return type without evaluation (for static type checking).
    /// Accepts union types in arg_types and returns a union of all possible return types.
    /// A list argument whose element type is a union is typed by its
    /// forms (`list[int | nulltype]` as `list[int]` and `list[nulltype]`).
    /// A list concatenation of known element types (a `range_expr` counting
    /// as a `list[int]`) has their common element type, as concatenating
    /// values gives, of unknown ones (or an operand of unknown type)
    /// `list[any]`, and of no common type the signature's type (`list[T3]`,
    /// or `list[T2]` for the range overloads).
    pub fn derive_return_type(&self, name: &str, arg_types: &[ExprType]) -> Option<ExprType> {
        self.derive_return_type_by_signature(name, arg_types)
            .map(|ret| concat_return_type(name, arg_types, ret))
    }

    fn derive_return_type_by_signature(
        &self,
        name: &str,
        arg_types: &[ExprType],
    ) -> Option<ExprType> {
        // A list argument whose element type is a union is typed by its
        // forms, as `call` types an unresolved one (2026-02 Expression
        // Language §1.2.1). Past `MAX_FORM_COMBINATIONS` they are typed as
        // they are, below.
        if let Some(combinations) = form_combinations(arg_types).filter(|c| c.len() > 1) {
            let returns: Vec<ExprType> = combinations
                .iter()
                .filter_map(|c| self.derive_return_type_by_signature(name, c))
                .collect();
            if returns.is_empty() {
                return None;
            }
            return Some(crate::types::merge_list_union(ExprType::union(returns)));
        }
        // Fast path: if no union args, try exact match first (matches Python's singleton optimization)
        let has_unions = arg_types
            .iter()
            .any(|t| t.code() == crate::types::TypeCode::Union);
        if !has_unions {
            for entry in self.get_signatures(name) {
                if empty_list_overload_mismatch(entry, arg_types) {
                    continue;
                }
                if let Some(bindings) = entry.signature.match_call(arg_types) {
                    return Some(entry.signature.sig_return().substitute(&bindings));
                }
            }
            // Try coercion
            for entry in self.get_signatures(name) {
                if empty_list_overload_mismatch(entry, arg_types) {
                    continue;
                }
                if let Some(coerced) =
                    try_coerce_types(arg_types, entry.signature.sig_params(), false)
                {
                    if let Some(bindings) = entry.signature.match_call(&coerced) {
                        return Some(entry.signature.sig_return().substitute(&bindings));
                    }
                }
            }
            return None;
        }

        // Union path: flatten each arg's types into sets, then recurse per-signature.
        // This prunes early: if arg 0 doesn't match a signature's param 0, we skip
        // all combinations involving that arg type without expanding further args.
        let arg_type_sets: Vec<Vec<ExprType>> = arg_types
            .iter()
            .map(|t| {
                if t.code() == crate::types::TypeCode::Union {
                    let mut members = t.params().to_vec();
                    // Apply implicit coercions: int→float, path→string
                    let mut coerced = Vec::new();
                    for m in &members {
                        if m.code() == crate::types::TypeCode::Int {
                            coerced.push(ExprType::FLOAT);
                        } else if m.code() == crate::types::TypeCode::Path {
                            coerced.push(ExprType::STRING);
                        }
                    }
                    for c in coerced {
                        if !members.contains(&c) {
                            members.push(c);
                        }
                    }
                    members
                } else {
                    let mut members = vec![t.clone()];
                    if t.code() == crate::types::TypeCode::Int {
                        members.push(ExprType::FLOAT);
                    } else if t.code() == crate::types::TypeCode::Path {
                        members.push(ExprType::STRING);
                    }
                    members
                }
            })
            .collect();

        let mut result_types = Vec::new();
        for entry in self.get_signatures(name) {
            let sig_params = entry.signature.sig_params();
            if sig_params.len() != arg_type_sets.len() {
                continue;
            }
            Self::match_signature_recursive(
                &entry.signature,
                sig_params,
                &arg_type_sets,
                0,
                HashMap::new(),
                HashMap::new(),
                &mut result_types,
            );
        }

        if result_types.is_empty() {
            return None;
        }
        result_types.sort_by_key(|a| a.to_string());
        result_types.dedup();
        if result_types.len() == 1 {
            Some(result_types.into_iter().next().unwrap())
        } else {
            Some(ExprType::union(result_types))
        }
    }

    /// Recursively match a signature against argument type sets, pruning
    /// non-matching branches early instead of expanding the full Cartesian product.
    fn match_signature_recursive(
        sig: &ExprType,
        sig_params: &[ExprType],
        arg_type_sets: &[Vec<ExprType>],
        idx: usize,
        bindings: HashMap<crate::types::TypeCode, ExprType>,
        weak: HashMap<crate::types::TypeCode, ExprType>,
        result_types: &mut Vec<ExprType>,
    ) {
        if idx == arg_type_sets.len() {
            let mut all = bindings;
            crate::types::apply_weak_bindings(&mut all, weak);
            result_types.push(sig.sig_return().substitute(&all));
            return;
        }
        let param = &sig_params[idx];
        for arg_type in &arg_type_sets[idx] {
            if let Some(new_binds) = param.match_type(arg_type) {
                let mut merged = bindings.clone();
                let mut merged_weak = weak.clone();
                // The empty list binds a `list[<var>]` parameter only weakly,
                // exactly as `ExprType::match_call` does.
                let target = if crate::types::is_empty_list_against_list_var(param, arg_type) {
                    &mut merged_weak
                } else {
                    &mut merged
                };
                let mut conflict = false;
                for (k, v) in new_binds {
                    if crate::types::merge_binding(target, k, v).is_none() {
                        conflict = true;
                        break;
                    }
                }
                if !conflict {
                    Self::match_signature_recursive(
                        sig,
                        sig_params,
                        arg_type_sets,
                        idx + 1,
                        merged,
                        merged_weak,
                        result_types,
                    );
                }
            }
        }
    }

    /// Get the type of a property access.
    pub fn get_property_type(&self, base_type: &ExprType, property_name: &str) -> Option<ExprType> {
        self.derive_return_type(
            &format!("__property_{property_name}__"),
            std::slice::from_ref(base_type),
        )
    }
}

/// The diagnostic for `item in container` when no `__contains__` signature
/// matches and the container is a list or a range expression. Returns `None`
/// for other containers (a string haystack keeps the generic message).
fn membership_mismatch_message(op: &str, container: &ExprType, item: &ExprType) -> Option<String> {
    match container.code() {
        crate::types::TypeCode::List => {
            let elem = container.params().first()?;
            Some(format!(
                "Cannot use '{op}' operator: item of type {item} is not compatible with the element type {elem} of {container}"
            ))
        }
        crate::types::TypeCode::RangeExpr => Some(format!(
            "Cannot use '{op}' operator: item of type {item} is not compatible with a range_expr container, which holds int values"
        )),
        _ => None,
    }
}

/// Implicit coercion rules: int→float, path→string.
fn can_coerce(from: &ExprType, to: &ExprType) -> bool {
    (from.code() == crate::types::TypeCode::Int && to.code() == crate::types::TypeCode::Float)
        || (from.code() == crate::types::TypeCode::Path
            && to.code() == crate::types::TypeCode::String)
}

/// Most combinations of list forms a call is typed by; past this the
/// call is typed from the argument types as they are, without expanding
/// their forms.
const MAX_FORM_COMBINATIONS: usize = 64;

/// The type of a list concatenation (`__add__` of two lists, or of a list
/// and a `range_expr`, which concatenates as a `list[int]`) whose signature
/// leaves its result's element type unbound (`list[T3]`, `list[T2]`): the
/// common element type (2026-02 §2.1.3), by the join a list literal
/// holding both lists uses, as concatenating the values does
/// (`list[list[int]] + list[list[float]]` is `list[list[float]]`). An
/// operand whose element type is not known (it holds `any`, `unresolved`,
/// or a type variable) makes it `list[any]`: the values decide. Any other
/// call, or operands with no common type, keep `ret`.
fn concat_return_type(name: &str, arg_types: &[ExprType], ret: ExprType) -> ExprType {
    let as_list = |t: &ExprType| {
        let t = match (t.code(), t.params()) {
            (crate::types::TypeCode::Unresolved, [inner]) => inner.clone(),
            _ => t.clone(),
        };
        match t.code() {
            crate::types::TypeCode::RangeExpr => ExprType::list(ExprType::INT),
            // An operand of unknown type, which a run that succeeds has as
            // a list of unknown elements.
            crate::types::TypeCode::Any
            | crate::types::TypeCode::TypeVarT
            | crate::types::TypeCode::TypeVarT1
            | crate::types::TypeCode::TypeVarT2
            | crate::types::TypeCode::TypeVarT3 => ExprType::list(ExprType::ANY),
            _ => t,
        }
    };
    let [left, right] = arg_types else {
        return ret;
    };
    let (left, right) = (as_list(left), as_list(right));
    if name != "__add__" || !ret.is_symbolic() || !left.is_list() || !right.is_list() {
        return ret;
    }
    // `any` joins as the other type, which a value of it need not be
    // (`list[any] + list[int]` may be a `list[string]`), and a type
    // variable joins with nothing.
    fn unknown(t: &ExprType) -> bool {
        matches!(
            t.code(),
            crate::types::TypeCode::Any | crate::types::TypeCode::Unresolved
        ) || t.is_symbolic()
            || t.params().iter().any(unknown)
    }
    if unknown(&left) || unknown(&right) {
        return ExprType::list(ExprType::ANY);
    }
    match crate::types::join_list_element(&left, &right) {
        Some(joined) => crate::types::simplify_joined_list_element(joined),
        None => ret,
    }
}

/// The combinations of the arguments' list forms
/// ([`crate::types::list_forms`]): one argument list per way of picking a
/// form for each argument. `None` past [`MAX_FORM_COMBINATIONS`].
fn form_combinations(arg_types: &[ExprType]) -> Option<Vec<Vec<ExprType>>> {
    let forms: Vec<Vec<ExprType>> = arg_types.iter().map(crate::types::list_forms).collect();
    let count = forms
        .iter()
        .try_fold(1usize, |n, f| n.checked_mul(f.len()))
        .filter(|n| *n <= MAX_FORM_COMBINATIONS)?;
    Some(
        (0..count)
            .map(|i| {
                let mut rest = i;
                forms
                    .iter()
                    .map(|f| {
                        let t = f[rest % f.len()].clone();
                        rest /= f.len();
                        t
                    })
                    .collect()
            })
            .collect(),
    )
}

/// The return type of a call with an unresolved argument, typed from the
/// argument types by the same phases [`FunctionLibrary::call`] applies
/// to values (exact, coerced, then generic), or `None` when no signature
/// matches.
///
/// A list argument whose element type is a union stands for one list
/// type per member (2026-02 Expression Language §1.2.1:
/// `list[int | nulltype]` is a `list[int]` or the empty `list[nulltype]`),
/// so the call is typed for each combination of forms: the result is the
/// union of the return types of the combinations that match, a union of
/// list types read as one list type (`list[int] | list[nulltype]` is
/// `list[int | nulltype]`). It is `None` when no combination matches.
/// Past [`MAX_FORM_COMBINATIONS`] the arguments are typed as they are.
fn unresolved_return_type(
    entries: &[FunctionEntry],
    arg_types: &[ExprType],
    skip_receiver_coercion: bool,
) -> Option<ExprType> {
    let Some(combinations) = form_combinations(arg_types) else {
        return phase_return_type(entries, arg_types, skip_receiver_coercion);
    };
    if let [only] = combinations.as_slice() {
        return phase_return_type(entries, only, skip_receiver_coercion);
    }
    let returns: Vec<ExprType> = combinations
        .iter()
        .filter_map(|c| phase_return_type(entries, c, skip_receiver_coercion))
        .collect();
    if returns.is_empty() {
        return None;
    }
    Some(crate::types::merge_list_union(ExprType::union(returns)))
}

/// Whether `entry` has a `list[nulltype]` parameter whose argument is not
/// known to be the empty list.
///
/// Such an overload (`__getitem__(list[nulltype], int) -> noreturn`,
/// `sum(list[nulltype]) -> int`) specializes a general one for the empty
/// list. Signature matching lets an argument of unknown element type
/// (`list[T3]`, `list[any]`, `any`) match it, but only an argument that is
/// the empty list selects it at run time; any other may be non-empty and
/// takes the general overload. So at the type level it applies only to an
/// argument of type `list[nulltype]` — the empty form of a list with a
/// union element type, or an empty literal.
fn empty_list_overload_mismatch(entry: &FunctionEntry, arg_types: &[ExprType]) -> bool {
    let empty = ExprType::list(ExprType::NULLTYPE);
    entry
        .signature
        .sig_params()
        .iter()
        .zip(arg_types)
        .any(|(p, a)| *p == empty && *a != empty)
}

/// The return type of the first signature the dispatch phases match for
/// `arg_types`, applied to types.
fn phase_return_type(
    entries: &[FunctionEntry],
    arg_types: &[ExprType],
    skip_receiver_coercion: bool,
) -> Option<ExprType> {
    let entries: Vec<&FunctionEntry> = entries
        .iter()
        .filter(|e| !empty_list_overload_mismatch(e, arg_types))
        .collect();
    let entries = entries.as_slice();
    // Phase 1: exact match (non-generic)
    for entry in entries {
        if !entry.signature.is_symbolic() && entry.signature.match_call(arg_types).is_some() {
            return Some(entry.signature.sig_return().clone());
        }
    }
    // Phase 2: coerced match (non-generic)
    for entry in entries {
        if !entry.signature.is_symbolic()
            && try_coerce_types(
                arg_types,
                entry.signature.sig_params(),
                skip_receiver_coercion,
            )
            .is_some()
        {
            return Some(entry.signature.sig_return().clone());
        }
    }
    // Phase 3: generic match, then generic with coercion
    for entry in entries {
        if !entry.signature.is_symbolic() {
            continue;
        }
        if let Some(bindings) = entry.signature.match_call(arg_types) {
            return Some(entry.signature.sig_return().substitute(&bindings));
        }
        if let Some(coerced) = try_coerce_types(
            arg_types,
            entry.signature.sig_params(),
            skip_receiver_coercion,
        ) {
            if let Some(bindings) = entry.signature.match_call(&coerced) {
                return Some(entry.signature.sig_return().substitute(&bindings));
            }
        }
    }
    None
}

/// Try to coerce argument types to match parameter types.
fn try_coerce_types(
    arg_types: &[ExprType],
    param_types: &[ExprType],
    skip_first: bool,
) -> Option<Vec<ExprType>> {
    if arg_types.len() != param_types.len() {
        return None;
    }
    let mut coerced = Vec::with_capacity(arg_types.len());
    for (i, (at, pt)) in arg_types.iter().zip(param_types.iter()).enumerate() {
        if at == pt || pt.code() == crate::types::TypeCode::Any {
            coerced.push(at.clone());
        } else if can_coerce(at, pt) && !(skip_first && i == 0) {
            coerced.push(pt.clone());
        } else if pt.is_symbolic() {
            coerced.push(at.clone());
        } else {
            return None;
        }
    }
    Some(coerced)
}

/// Try to coerce argument values to match parameter types.
fn try_coerce_args(
    args: &[ExprValue],
    arg_types: &[ExprType],
    param_types: &[ExprType],
    skip_first: bool,
) -> Option<Vec<ExprValue>> {
    if args.len() != param_types.len() {
        return None;
    }
    let mut coerced = Vec::with_capacity(args.len());
    let mut any_changed = false;
    for (i, (at, pt)) in arg_types.iter().zip(param_types.iter()).enumerate() {
        if at == pt {
            coerced.push(args[i].clone());
        } else if can_coerce(at, pt) && !(skip_first && i == 0) {
            any_changed = true;
            match (&args[i], pt.code()) {
                (ExprValue::Int(v), crate::types::TypeCode::Float) => {
                    coerced.push(ExprValue::Float(
                        crate::value::Float64::new(*v as f64).unwrap(),
                    ));
                }
                (ExprValue::Path { value, .. }, crate::types::TypeCode::String) => {
                    coerced.push(ExprValue::String(value.clone()));
                }
                _ => return None,
            }
        } else {
            return None;
        }
    }
    if any_changed {
        Some(coerced)
    } else {
        None
    }
}

/// Coerce values to match target types (for generic dispatch after type matching).
fn coerce_values(args: &[ExprValue], target_types: &[ExprType]) -> Vec<ExprValue> {
    args.iter()
        .zip(target_types.iter())
        .map(|(a, t)| {
            if can_coerce(&a.expr_type(), t) {
                match (a, t.code()) {
                    (ExprValue::Int(v), crate::types::TypeCode::Float) => {
                        ExprValue::Float(crate::value::Float64::new(*v as f64).unwrap())
                    }
                    (ExprValue::Path { value, .. }, crate::types::TypeCode::String) => {
                        ExprValue::String(value.clone())
                    }
                    _ => a.clone(),
                }
            } else {
                a.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ExprType;

    fn dummy_impl(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        Ok(args.first().cloned().unwrap_or(ExprValue::Null))
    }

    #[test]
    fn register_and_get() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("len", "(string) -> int", dummy_impl)
            .unwrap();
        lib.register_sig("len", "(list[T1]) -> int", dummy_impl)
            .unwrap();
        assert_eq!(lib.get_signatures("len").len(), 2);
        assert_eq!(lib.get_signatures("missing").len(), 0);
    }

    #[test]
    fn merge_libraries() {
        let mut a = FunctionLibrary::new();
        a.register_sig("foo", "(int) -> int", dummy_impl).unwrap();
        let mut b = FunctionLibrary::new();
        b.register_sig("bar", "(string) -> string", dummy_impl)
            .unwrap();
        b.register_sig("foo", "(float) -> float", dummy_impl)
            .unwrap();
        let merged = a.merge(b);
        assert_eq!(merged.get_signatures("foo").len(), 2);
        assert_eq!(merged.get_signatures("bar").len(), 1);
    }

    #[test]
    fn register_sig_parses() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__getitem__", "(list[T1], int) -> T1", dummy_impl)
            .unwrap();
        let sigs = lib.get_signatures("__getitem__");
        assert_eq!(sigs.len(), 1);
        assert!(sigs[0].signature.is_symbolic());
    }

    #[test]
    fn function_names() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("alpha", "(int) -> int", dummy_impl)
            .unwrap();
        lib.register_sig("beta", "(int) -> int", dummy_impl)
            .unwrap();
        let names: Vec<&str> = lib.function_names().collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    // -- Dispatch tests --

    struct MockCtx;
    impl EvalContext for MockCtx {
        fn path_format(&self) -> crate::path_mapping::PathFormat {
            crate::path_mapping::PathFormat::Posix
        }
        fn count_op(&mut self) -> Result<(), ExpressionError> {
            Ok(())
        }
        fn count_ops(&mut self, _n: usize) -> Result<(), ExpressionError> {
            Ok(())
        }
        fn count_string_ops(&mut self, _len: usize) -> Result<(), ExpressionError> {
            Ok(())
        }
        fn check_memory(&self, _bytes: usize) -> Result<(), ExpressionError> {
            Ok(())
        }
    }

    fn add_int(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        match (&args[0], &args[1]) {
            (ExprValue::Int(a), ExprValue::Int(b)) => Ok(ExprValue::Int(a + b)),
            _ => Err(ExpressionError::type_error("type error")),
        }
    }

    fn add_float(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        match (&args[0], &args[1]) {
            (ExprValue::Float(a), ExprValue::Float(b)) => Ok(ExprValue::Float(
                crate::value::Float64::new(a.value() + b.value())?,
            )),
            _ => Err(ExpressionError::type_error("type error")),
        }
    }

    fn list_len(
        _ctx: &mut dyn EvalContext,
        args: &[ExprValue],
    ) -> Result<ExprValue, ExpressionError> {
        Ok(ExprValue::Int(args[0].list_len().unwrap_or(0) as i64))
    }

    #[test]
    fn dispatch_exact_match() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int)
            .unwrap();
        lib.register_sig("__add__", "(float, float) -> float", add_float)
            .unwrap();
        let mut ctx = MockCtx;
        let r = lib
            .call("__add__", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx)
            .unwrap();
        assert_eq!(r, ExprValue::Int(5));
    }

    #[test]
    fn dispatch_coerced_match() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(float, float) -> float", add_float)
            .unwrap();
        let mut ctx = MockCtx;
        // int + float → coerce int to float
        let r = lib
            .call(
                "__add__",
                &[
                    ExprValue::Int(2),
                    ExprValue::Float(crate::value::Float64::new(3.0).unwrap()),
                ],
                &mut ctx,
            )
            .unwrap();
        assert!(matches!(r, ExprValue::Float(_)));
    }

    #[test]
    fn dispatch_generic_match() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("len", "(list[T1]) -> int", list_len)
            .unwrap();
        let mut ctx = MockCtx;
        let list = ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT)
            .unwrap();
        let r = lib.call("len", &[list], &mut ctx).unwrap();
        assert_eq!(r, ExprValue::Int(2));
    }

    #[test]
    fn dispatch_unresolved_returns_unresolved() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int)
            .unwrap();
        let mut ctx = MockCtx;
        let r = lib
            .call(
                "__add__",
                &[ExprValue::unresolved(ExprType::INT), ExprValue::Int(1)],
                &mut ctx,
            )
            .unwrap();
        assert!(r.is_unresolved());
        assert_eq!(r.expr_type(), ExprType::unresolved(ExprType::INT));
    }

    #[test]
    fn dispatch_no_match_errors() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int)
            .unwrap();
        let mut ctx = MockCtx;
        let r = lib.call(
            "__add__",
            &[ExprValue::String("a".into()), ExprValue::Int(1)],
            &mut ctx,
        );
        assert!(r.is_err());
    }

    #[test]
    fn dispatch_unknown_function_errors() {
        let lib = FunctionLibrary::new();
        let mut ctx = MockCtx;
        let r = lib.call("nonexistent", &[ExprValue::Int(1)], &mut ctx);
        assert!(r.is_err());
    }

    #[test]
    fn derive_return_type_exact() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int)
            .unwrap();
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::INT]),
            Some(ExprType::INT)
        );
    }

    #[test]
    fn derive_return_type_generic() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__getitem__", "(list[T1], int) -> T1", dummy_impl)
            .unwrap();
        assert_eq!(
            lib.derive_return_type(
                "__getitem__",
                &[ExprType::list(ExprType::STRING), ExprType::INT]
            ),
            Some(ExprType::STRING)
        );
    }

    #[test]
    fn derive_return_type_coerced() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(float, float) -> float", add_float)
            .unwrap();
        // int + float → coerce → float
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::FLOAT]),
            Some(ExprType::FLOAT)
        );
    }

    #[test]
    fn derive_return_type_union_args() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int)
            .unwrap();
        lib.register_sig("__add__", "(float, float) -> float", add_float)
            .unwrap();
        // (int | float) + int → int (from int+int) | float (from float coerced)
        let union_arg = ExprType::union(vec![ExprType::INT, ExprType::FLOAT]);
        let result = lib
            .derive_return_type("__add__", &[union_arg, ExprType::INT])
            .unwrap();
        assert_eq!(
            result,
            ExprType::union(vec![ExprType::INT, ExprType::FLOAT])
        );
    }

    #[test]
    fn derive_return_type_union_collapses_to_single() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("len", "(string) -> int", dummy_impl)
            .unwrap();
        lib.register_sig("len", "(list[T1]) -> int", dummy_impl)
            .unwrap();
        // len(string | list[int]) → int (both return int)
        let union_arg = ExprType::union(vec![ExprType::STRING, ExprType::list(ExprType::INT)]);
        assert_eq!(
            lib.derive_return_type("len", &[union_arg]),
            Some(ExprType::INT)
        );
    }

    #[test]
    fn get_property_type_path() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__property_name__", "(path) -> string", dummy_impl)
            .unwrap();
        lib.register_sig("__property_parent__", "(path) -> path", dummy_impl)
            .unwrap();
        assert_eq!(
            lib.get_property_type(&ExprType::PATH, "name"),
            Some(ExprType::STRING)
        );
        assert_eq!(
            lib.get_property_type(&ExprType::PATH, "parent"),
            Some(ExprType::PATH)
        );
        assert_eq!(lib.get_property_type(&ExprType::INT, "name"), None);
    }
}
