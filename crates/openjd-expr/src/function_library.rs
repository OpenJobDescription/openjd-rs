// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

/// A registered function overload.
#[derive(Clone)]
pub struct FunctionEntry {
    pub signature: ExprType, // TypeCode::Signature
    pub implementation:
        fn(&mut dyn EvalContext, &[ExprValue]) -> Result<ExprValue, ExpressionError>,
}

/// Trait for the evaluator context that function implementations can access.
pub trait EvalContext {
    fn path_format(&self) -> crate::path_mapping::PathFormat;
    fn path_mapping_rules(&self) -> &[crate::path_mapping::PathMappingRule];
    fn count_op(&mut self) -> Result<(), ExpressionError>;
    fn count_ops(&mut self, n: usize) -> Result<(), ExpressionError>;
    fn count_string_ops(&mut self, len: usize) -> Result<(), ExpressionError>;
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
    pub host_context_enabled: bool,
}

impl FunctionLibrary {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            host_context_enabled: false,
        }
    }

    /// Return a copy with host-only functions (like `apply_path_mapping`) enabled.
    #[must_use]
    pub fn with_host_context(mut self) -> Self {
        if !self.host_context_enabled {
            self.host_context_enabled = true;
            crate::default_library::register_host_context_functions(&mut self);
        }
        self
    }

    /// Return a copy with host-only function signatures available, but returning
    /// `Unresolved(PATH)` instead of actual values. Use at job creation time when
    /// path mapping rules aren't available yet.
    #[must_use]
    pub fn with_unresolved_host_context(mut self) -> Self {
        if !self.host_context_enabled {
            self.host_context_enabled = true;
            crate::default_library::register_unresolved_host_context_functions(&mut self);
        }
        self
    }

    /// Register a function overload with an ExprType::Signature.
    pub fn register(
        &mut self,
        name: &str,
        signature: ExprType,
        implementation: fn(
            &mut dyn EvalContext,
            &[ExprValue],
        ) -> Result<ExprValue, ExpressionError>,
    ) {
        self.functions
            .entry(name.to_string())
            .or_default()
            .push(FunctionEntry {
                signature,
                implementation,
            });
    }

    /// Register from spec notation: `lib.register_sig("len", "(list[T1]) -> int", len_list)`
    pub fn register_sig(
        &mut self,
        name: &str,
        sig_str: &str,
        implementation: fn(
            &mut dyn EvalContext,
            &[ExprValue],
        ) -> Result<ExprValue, ExpressionError>,
    ) {
        let signature =
            ExprType::parse(sig_str).unwrap_or_else(|e| panic!("Bad signature '{sig_str}': {e}"));
        self.register(name, signature, implementation);
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

    /// Dispatch a function call: exact → coerced → generic.
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
                if t.code == crate::types::TypeCode::Unresolved && !t.params.is_empty() {
                    t.params[0].clone() // unwrap unresolved for matching
                } else {
                    t
                }
            })
            .collect();
        let any_unresolved = args.iter().any(|a| a.is_unresolved());

        // Phase 1: exact match (non-generic)
        for entry in entries {
            if entry.signature.is_symbolic() {
                continue;
            }
            if entry.signature.match_call(&arg_types).is_some() {
                if any_unresolved {
                    let ret = entry.signature.sig_return().clone();
                    return Ok(ExprValue::unresolved(ret));
                }
                return (entry.implementation)(ctx, args);
            }
        }

        // Phase 2: coerced match (non-generic)
        for entry in entries {
            if entry.signature.is_symbolic() {
                continue;
            }
            if any_unresolved {
                if try_coerce_types(
                    &arg_types,
                    entry.signature.sig_params(),
                    skip_receiver_coercion,
                )
                .is_some()
                {
                    let ret = entry.signature.sig_return().clone();
                    return Ok(ExprValue::unresolved(ret));
                }
            } else if let Some(coerced) = try_coerce_args(
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
            if !entry.signature.is_symbolic() {
                continue;
            }
            if let Some(bindings) = entry.signature.match_call(&arg_types) {
                if any_unresolved {
                    let ret = entry.signature.sig_return().substitute(&bindings);
                    return Ok(ExprValue::unresolved(ret));
                }
                return (entry.implementation)(ctx, args);
            }
            // Try with coercion for generics too
            if let Some(coerced_types) = try_coerce_types(
                &arg_types,
                entry.signature.sig_params(),
                skip_receiver_coercion,
            ) {
                if let Some(bindings) = entry.signature.match_call(&coerced_types) {
                    let coerced = coerce_values(args, &coerced_types);
                    if any_unresolved {
                        let ret = entry.signature.sig_return().substitute(&bindings);
                        return Ok(ExprValue::unresolved(ret));
                    }
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
    pub fn derive_return_type(&self, name: &str, arg_types: &[ExprType]) -> Option<ExprType> {
        // Fast path: if no union args, try exact match first (matches Python's singleton optimization)
        let has_unions = arg_types
            .iter()
            .any(|t| t.code == crate::types::TypeCode::Union);
        if !has_unions {
            for entry in self.get_signatures(name) {
                if let Some(bindings) = entry.signature.match_call(arg_types) {
                    return Some(entry.signature.sig_return().substitute(&bindings));
                }
            }
            // Try coercion
            for entry in self.get_signatures(name) {
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

        // Union path: expand and collect all possible return types
        let expanded = expand_unions(arg_types);
        let mut result_types = Vec::new();

        for concrete_args in &expanded {
            // Try exact, then coerced for each combination
            let mut found = false;
            for entry in self.get_signatures(name) {
                if let Some(bindings) = entry.signature.match_call(concrete_args) {
                    result_types.push(entry.signature.sig_return().substitute(&bindings));
                    found = true;
                    break;
                }
            }
            if !found {
                for entry in self.get_signatures(name) {
                    if let Some(coerced) =
                        try_coerce_types(concrete_args, entry.signature.sig_params(), false)
                    {
                        if let Some(bindings) = entry.signature.match_call(&coerced) {
                            result_types.push(entry.signature.sig_return().substitute(&bindings));
                            break;
                        }
                    }
                }
            }
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

    /// Get the type of a property access.
    pub fn get_property_type(&self, base_type: &ExprType, property_name: &str) -> Option<ExprType> {
        self.derive_return_type(
            &format!("__property_{property_name}__"),
            std::slice::from_ref(base_type),
        )
    }
}

/// Expand union types into all concrete type combinations.
/// `[int | string, float]` → `[[int, float], [string, float]]`
fn expand_unions(arg_types: &[ExprType]) -> Vec<Vec<ExprType>> {
    let mut combos = vec![Vec::new()];
    for arg in arg_types {
        let members: Vec<&ExprType> = if arg.code == crate::types::TypeCode::Union {
            arg.params.iter().collect()
        } else {
            vec![arg]
        };
        let mut new_combos = Vec::new();
        for combo in &combos {
            for m in &members {
                let mut c = combo.clone();
                c.push((*m).clone());
                new_combos.push(c);
            }
        }
        combos = new_combos;
    }
    combos
}

/// Implicit coercion rules: int→float, path→string.
fn can_coerce(from: &ExprType, to: &ExprType) -> bool {
    (from.code == crate::types::TypeCode::Int && to.code == crate::types::TypeCode::Float)
        || (from.code == crate::types::TypeCode::Path && to.code == crate::types::TypeCode::String)
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
        if at == pt || pt.code == crate::types::TypeCode::Any {
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
            match (&args[i], pt.code) {
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
                match (a, t.code) {
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
        lib.register_sig("len", "(string) -> int", dummy_impl);
        lib.register_sig("len", "(list[T1]) -> int", dummy_impl);
        assert_eq!(lib.get_signatures("len").len(), 2);
        assert_eq!(lib.get_signatures("missing").len(), 0);
    }

    #[test]
    fn merge_libraries() {
        let mut a = FunctionLibrary::new();
        a.register_sig("foo", "(int) -> int", dummy_impl);
        let mut b = FunctionLibrary::new();
        b.register_sig("bar", "(string) -> string", dummy_impl);
        b.register_sig("foo", "(float) -> float", dummy_impl);
        let merged = a.merge(b);
        assert_eq!(merged.get_signatures("foo").len(), 2);
        assert_eq!(merged.get_signatures("bar").len(), 1);
    }

    #[test]
    fn register_sig_parses() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__getitem__", "(list[T1], int) -> T1", dummy_impl);
        let sigs = lib.get_signatures("__getitem__");
        assert_eq!(sigs.len(), 1);
        assert!(sigs[0].signature.is_symbolic());
    }

    #[test]
    fn function_names() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("alpha", "(int) -> int", dummy_impl);
        lib.register_sig("beta", "(int) -> int", dummy_impl);
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
        fn path_mapping_rules(&self) -> &[crate::path_mapping::PathMappingRule] {
            &[]
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
            (ExprValue::Float(a), ExprValue::Float(b)) => {
                Ok(ExprValue::Float(crate::value::Float64::new(a.0 + b.0)?))
            }
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
        lib.register_sig("__add__", "(int, int) -> int", add_int);
        lib.register_sig("__add__", "(float, float) -> float", add_float);
        let mut ctx = MockCtx;
        let r = lib
            .call("__add__", &[ExprValue::Int(2), ExprValue::Int(3)], &mut ctx)
            .unwrap();
        assert_eq!(r, ExprValue::Int(5));
    }

    #[test]
    fn dispatch_coerced_match() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(float, float) -> float", add_float);
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
        lib.register_sig("len", "(list[T1]) -> int", list_len);
        let mut ctx = MockCtx;
        let list = ExprValue::make_list(vec![ExprValue::Int(1), ExprValue::Int(2)], ExprType::INT)
            .unwrap();
        let r = lib.call("len", &[list], &mut ctx).unwrap();
        assert_eq!(r, ExprValue::Int(2));
    }

    #[test]
    fn dispatch_unresolved_returns_unresolved() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int);
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
        lib.register_sig("__add__", "(int, int) -> int", add_int);
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
        lib.register_sig("__add__", "(int, int) -> int", add_int);
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::INT]),
            Some(ExprType::INT)
        );
    }

    #[test]
    fn derive_return_type_generic() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__getitem__", "(list[T1], int) -> T1", dummy_impl);
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
        lib.register_sig("__add__", "(float, float) -> float", add_float);
        // int + float → coerce → float
        assert_eq!(
            lib.derive_return_type("__add__", &[ExprType::INT, ExprType::FLOAT]),
            Some(ExprType::FLOAT)
        );
    }

    #[test]
    fn derive_return_type_union_args() {
        let mut lib = FunctionLibrary::new();
        lib.register_sig("__add__", "(int, int) -> int", add_int);
        lib.register_sig("__add__", "(float, float) -> float", add_float);
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
        lib.register_sig("len", "(string) -> int", dummy_impl);
        lib.register_sig("len", "(list[T1]) -> int", dummy_impl);
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
        lib.register_sig("__property_name__", "(path) -> string", dummy_impl);
        lib.register_sig("__property_parent__", "(path) -> path", dummy_impl);
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
