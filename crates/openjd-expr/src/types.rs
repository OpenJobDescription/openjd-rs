// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Expression type system.
//!
//! Mirrors Python `openjd.expr._types` and the spec's ExprType definition.
//! Supports primitive types, list types, union types, type variables,
//! unresolved types, and normalization rules.

use std::collections::HashMap;
use std::fmt;

/// Type codes for expression values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub enum TypeCode {
    Null,
    Bool,
    Int,
    Float,
    String,
    Path,
    List,
    RangeExpr,
    Any,
    Union,
    NoReturn,
    Unresolved,
    TypeVarT,
    TypeVarT1,
    TypeVarT2,
    TypeVarT3,
    /// Function signature: params are [param_type_0, ..., param_type_n, return_type].
    Signature,
}

/// Represents a type in the expression language.
///
/// Fields are crate-private to enforce normalization via constructors.
/// Use [`code()`](ExprType::code) and [`params()`](ExprType::params) for read access.
#[derive(Debug, Clone, Eq, serde::Serialize)]
pub struct ExprType {
    code: TypeCode,
    params: Vec<ExprType>,
}

// ── Constants ──

impl ExprType {
    pub const BOOL: ExprType = ExprType {
        code: TypeCode::Bool,
        params: Vec::new(),
    };
    pub const INT: ExprType = ExprType {
        code: TypeCode::Int,
        params: Vec::new(),
    };
    pub const FLOAT: ExprType = ExprType {
        code: TypeCode::Float,
        params: Vec::new(),
    };
    pub const STRING: ExprType = ExprType {
        code: TypeCode::String,
        params: Vec::new(),
    };
    pub const PATH: ExprType = ExprType {
        code: TypeCode::Path,
        params: Vec::new(),
    };
    pub const RANGE_EXPR: ExprType = ExprType {
        code: TypeCode::RangeExpr,
        params: Vec::new(),
    };
    pub const NULLTYPE: ExprType = ExprType {
        code: TypeCode::Null,
        params: Vec::new(),
    };
    pub const ANY: ExprType = ExprType {
        code: TypeCode::Any,
        params: Vec::new(),
    };
    pub const NORETURN: ExprType = ExprType {
        code: TypeCode::NoReturn,
        params: Vec::new(),
    };
    pub const T: ExprType = ExprType {
        code: TypeCode::TypeVarT,
        params: Vec::new(),
    };
    pub const T1: ExprType = ExprType {
        code: TypeCode::TypeVarT1,
        params: Vec::new(),
    };
    pub const T2: ExprType = ExprType {
        code: TypeCode::TypeVarT2,
        params: Vec::new(),
    };
    pub const T3: ExprType = ExprType {
        code: TypeCode::TypeVarT3,
        params: Vec::new(),
    };

    /// The type code for this type.
    pub fn code(&self) -> TypeCode {
        self.code
    }

    /// The type parameters (e.g. element type for lists, member types for unions).
    pub fn params(&self) -> &[ExprType] {
        &self.params
    }

    pub fn list(elem: ExprType) -> Self {
        // Normalize: list[unresolved[T]] -> unresolved[list[T]]
        if elem.code == TypeCode::Unresolved && elem.params.len() == 1 {
            let inner_list = ExprType {
                code: TypeCode::List,
                params: vec![elem.params[0].clone()],
            };
            return ExprType {
                code: TypeCode::Unresolved,
                params: vec![inner_list],
            };
        }
        ExprType {
            code: TypeCode::List,
            params: vec![elem],
        }
    }

    pub fn union(types: Vec<ExprType>) -> Self {
        normalize_union(types)
    }

    pub fn unresolved(constraint: ExprType) -> Self {
        // Normalize: unresolved[unresolved[T]] -> unresolved[T]
        if constraint.code == TypeCode::Unresolved {
            return constraint;
        }
        ExprType {
            code: TypeCode::Unresolved,
            params: vec![constraint],
        }
    }

    /// Create a function signature type: `(param_types) -> return_type`.
    /// Stored as params = [param0, param1, ..., return_type].
    pub fn signature(param_types: Vec<ExprType>, return_type: ExprType) -> Self {
        let mut params = param_types;
        params.push(return_type);
        ExprType {
            code: TypeCode::Signature,
            params,
        }
    }

    /// Get the parameter types of a signature (all params except the last).
    pub fn sig_params(&self) -> &[ExprType] {
        debug_assert_eq!(self.code, TypeCode::Signature);
        &self.params[..self.params.len() - 1]
    }

    /// Get the return type of a signature (the last param).
    pub fn sig_return(&self) -> &ExprType {
        debug_assert_eq!(self.code, TypeCode::Signature);
        self.params.last().unwrap()
    }

    /// Try to match a call's argument types against this signature.
    /// Returns type variable bindings if successful, None if no match.
    pub fn match_call(&self, arg_types: &[ExprType]) -> Option<HashMap<TypeCode, ExprType>> {
        let sig_params = self.sig_params();
        if sig_params.len() != arg_types.len() {
            return None;
        }
        let mut bindings = HashMap::new();
        for (sig_p, arg_t) in sig_params.iter().zip(arg_types.iter()) {
            let sub = sig_p.match_type(arg_t)?;
            for (k, v) in sub {
                if let Some(existing) = bindings.get(&k) {
                    if *existing != v {
                        return None;
                    }
                }
                bindings.insert(k, v);
            }
        }
        Some(bindings)
    }

    /// Resolve the return type of a signature given argument types.
    /// Matches args, substitutes bindings into return type.
    pub fn resolve_call(&self, arg_types: &[ExprType]) -> Option<ExprType> {
        let bindings = self.match_call(arg_types)?;
        Some(self.sig_return().substitute(&bindings))
    }
}

// ── Normalization ──

fn normalize_union(types: Vec<ExprType>) -> ExprType {
    let mut members = Vec::new();
    for t in types {
        match t.code {
            TypeCode::Any => return ExprType::ANY,
            TypeCode::NoReturn => continue,
            TypeCode::Union => members.extend(t.params),
            _ => members.push(t),
        }
    }
    // Hoist unresolved: T | unresolved[S] -> unresolved[T | S]
    let mut unresolved_constraints = Vec::new();
    let mut non_unresolved = Vec::new();
    for m in &members {
        if m.code == TypeCode::Unresolved {
            unresolved_constraints.push(m.params[0].clone());
        } else {
            non_unresolved.push(m.clone());
        }
    }
    if !unresolved_constraints.is_empty() {
        let mut all_parts = non_unresolved;
        all_parts.extend(unresolved_constraints);
        let inner = ExprType::union(all_parts);
        return ExprType::unresolved(inner);
    }
    // Deduplicate
    members.sort_by_key(|a| a.to_string());
    members.dedup();
    match members.len() {
        0 => ExprType::NORETURN,
        1 => members.into_iter().next().unwrap(),
        _ => ExprType {
            code: TypeCode::Union,
            params: members,
        },
    }
}

// ── Queries ──

impl ExprType {
    pub fn is_list(&self) -> bool {
        self.code == TypeCode::List
    }

    pub fn list_element_type(&self) -> Option<&ExprType> {
        if self.code == TypeCode::List {
            self.params.first()
        } else {
            None
        }
    }

    pub fn is_symbolic(&self) -> bool {
        matches!(
            self.code,
            TypeCode::TypeVarT | TypeCode::TypeVarT1 | TypeCode::TypeVarT2 | TypeCode::TypeVarT3
        ) || self.params.iter().any(|p| p.is_symbolic())
    }

    pub fn is_concrete(&self) -> bool {
        if matches!(
            self.code,
            TypeCode::Any
                | TypeCode::Union
                | TypeCode::Unresolved
                | TypeCode::TypeVarT
                | TypeCode::TypeVarT1
                | TypeCode::TypeVarT2
                | TypeCode::TypeVarT3
                | TypeCode::Signature
        ) {
            return false;
        }
        self.params.iter().all(|p| p.is_concrete())
    }

    /// Substitute type variables with concrete types.
    pub fn substitute(&self, bindings: &HashMap<TypeCode, ExprType>) -> ExprType {
        if let Some(bound) = bindings.get(&self.code) {
            return bound.clone();
        }
        if self.params.is_empty() {
            return self.clone();
        }
        let new_params: Vec<ExprType> =
            self.params.iter().map(|p| p.substitute(bindings)).collect();
        ExprType::new(self.code, new_params)
    }

    /// Try to match this (possibly symbolic) type against another type.
    /// Returns bindings if successful, None if no match.
    pub fn match_type(&self, other: &ExprType) -> Option<HashMap<TypeCode, ExprType>> {
        // Type variables bind
        if matches!(
            self.code,
            TypeCode::TypeVarT | TypeCode::TypeVarT1 | TypeCode::TypeVarT2 | TypeCode::TypeVarT3
        ) {
            let mut m = HashMap::new();
            m.insert(self.code, other.clone());
            return Some(m);
        }
        if matches!(
            other.code,
            TypeCode::TypeVarT | TypeCode::TypeVarT1 | TypeCode::TypeVarT2 | TypeCode::TypeVarT3
        ) {
            let mut m = HashMap::new();
            m.insert(other.code, self.clone());
            return Some(m);
        }
        // ANY matches anything
        if self.code == TypeCode::Any || other.code == TypeCode::Any {
            return Some(HashMap::new());
        }
        // UNRESOLVED delegates to constraint
        if self.code == TypeCode::Unresolved {
            return self.params[0].match_type(other);
        }
        if other.code == TypeCode::Unresolved {
            return self.match_type(&other.params[0]);
        }
        // UNION: matches if any member matches
        if self.code == TypeCode::Union && other.code == TypeCode::Union {
            for s in &self.params {
                for c in &other.params {
                    if let Some(r) = s.match_type(c) {
                        return Some(r);
                    }
                }
            }
            return None;
        }
        if self.code == TypeCode::Union {
            for member in &self.params {
                if let Some(r) = member.match_type(other) {
                    return Some(r);
                }
            }
            return None;
        }
        if other.code == TypeCode::Union {
            for member in &other.params {
                if let Some(r) = self.match_type(member) {
                    return Some(r);
                }
            }
            return None;
        }
        // Same code, match params
        if self.code != other.code {
            return None;
        }
        if self.params.len() != other.params.len() {
            return None;
        }
        let mut bindings = HashMap::new();
        for (sp, cp) in self.params.iter().zip(other.params.iter()) {
            let sub = sp.match_type(cp)?;
            for (k, v) in sub {
                if let Some(existing) = bindings.get(&k) {
                    if *existing != v {
                        return None;
                    }
                }
                bindings.insert(k, v);
            }
        }
        Some(bindings)
    }

    /// Construct with normalization.
    pub fn new(code: TypeCode, params: Vec<ExprType>) -> Self {
        match code {
            TypeCode::Union => normalize_union(params),
            TypeCode::List if params.len() == 1 => {
                ExprType::list(params.into_iter().next().unwrap())
            }
            TypeCode::Unresolved if params.len() == 1 => {
                ExprType::unresolved(params.into_iter().next().unwrap())
            }
            _ => ExprType { code, params },
        }
    }
}

// ── Parsing ──

impl ExprType {
    const MAX_PARSE_DEPTH: usize = 10;

    /// Parse a type string like "int", "list\[string\]", "int?", "int | string", "unresolved\[int\]".
    pub fn parse(s: &str) -> Result<ExprType, String> {
        Self::parse_inner(s, 0)
    }

    fn parse_inner(s: &str, depth: usize) -> Result<ExprType, String> {
        if depth > Self::MAX_PARSE_DEPTH {
            return Err("Type nesting depth exceeded".to_string());
        }
        // Signature: (T1, T2) -> T3  — check before union split since return type may contain |
        if s.starts_with('(') {
            if let Some(arrow_pos) = s.find(") -> ") {
                let params_str = &s[1..arrow_pos];
                let ret_str = &s[arrow_pos + 5..];
                let param_types = if params_str.is_empty() {
                    Vec::new()
                } else {
                    split_params(params_str)
                        .iter()
                        .map(|p| Self::parse_inner(p, depth + 1))
                        .collect::<Result<Vec<_>, _>>()?
                };
                let return_type = Self::parse_inner(ret_str, depth + 1)?;
                return Ok(ExprType::signature(param_types, return_type));
            }
        }
        // Union: split on " | " outside brackets/parens
        let parts = split_union(s);
        if parts.len() > 1 {
            let types: Result<Vec<_>, _> = parts
                .iter()
                .map(|p| Self::parse_inner(p, depth + 1))
                .collect();
            return Ok(ExprType::union(types?));
        }
        // Base types
        match s {
            "bool" => return Ok(ExprType::BOOL),
            "int" => return Ok(ExprType::INT),
            "float" => return Ok(ExprType::FLOAT),
            "string" => return Ok(ExprType::STRING),
            "path" => return Ok(ExprType::PATH),
            "range_expr" => return Ok(ExprType::RANGE_EXPR),
            "nulltype" => return Ok(ExprType::NULLTYPE),
            "noreturn" => return Ok(ExprType::NORETURN),
            "any" => return Ok(ExprType::ANY),
            "unresolved" => return Ok(ExprType::unresolved(ExprType::ANY)),
            _ => {}
        }
        // Optional: T?
        if let Some(inner) = s.strip_suffix('?') {
            let t = Self::parse_inner(inner, depth + 1)?;
            return Ok(ExprType::union(vec![t, ExprType::NULLTYPE]));
        }
        // list[T]
        if let Some(inner) = s.strip_prefix("list[").and_then(|s| s.strip_suffix(']')) {
            let elem = Self::parse_inner(inner, depth + 1)?;
            return Ok(ExprType::list(elem));
        }
        // unresolved[T]
        if let Some(inner) = s
            .strip_prefix("unresolved[")
            .and_then(|s| s.strip_suffix(']'))
        {
            let constraint = Self::parse_inner(inner, depth + 1)?;
            return Ok(ExprType::unresolved(constraint));
        }
        // Type variables
        match s {
            "T" => return Ok(ExprType::T),
            "T1" => return Ok(ExprType::T1),
            "T2" => return Ok(ExprType::T2),
            "T3" => return Ok(ExprType::T3),
            _ => {}
        }
        Err(format!("Unknown type string: {s}"))
    }
}

fn split_union(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth -= 1,
            b' ' if depth == 0
                && i + 2 < bytes.len()
                && bytes[i + 1] == b'|'
                && bytes[i + 2] == b' ' =>
            {
                parts.push(&s[start..i]);
                i += 3;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Split on ", " respecting brackets and parens.
fn split_params(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

// ── Display ──

impl fmt::Display for ExprType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            TypeCode::Null => write!(f, "nulltype"),
            TypeCode::Bool => write!(f, "bool"),
            TypeCode::Int => write!(f, "int"),
            TypeCode::Float => write!(f, "float"),
            TypeCode::String => write!(f, "string"),
            TypeCode::Path => write!(f, "path"),
            TypeCode::RangeExpr => write!(f, "range_expr"),
            TypeCode::Any => write!(f, "any"),
            TypeCode::NoReturn => write!(f, "noreturn"),
            TypeCode::TypeVarT => write!(f, "T"),
            TypeCode::TypeVarT1 => write!(f, "T1"),
            TypeCode::TypeVarT2 => write!(f, "T2"),
            TypeCode::TypeVarT3 => write!(f, "T3"),
            TypeCode::List => {
                if let Some(elem) = self.params.first() {
                    write!(f, "list[{elem}]")
                } else {
                    write!(f, "list")
                }
            }
            TypeCode::Unresolved => {
                if let Some(constraint) = self.params.first() {
                    if constraint.code == TypeCode::Any {
                        write!(f, "unresolved")
                    } else {
                        write!(f, "unresolved[{constraint}]")
                    }
                } else {
                    write!(f, "unresolved")
                }
            }
            TypeCode::Union => {
                let non_null: Vec<_> = self
                    .params
                    .iter()
                    .filter(|t| t.code != TypeCode::Null)
                    .collect();
                let has_null = non_null.len() < self.params.len();
                if has_null && non_null.len() == 1 {
                    return write!(f, "{}?", non_null[0]);
                }
                let mut parts: Vec<std::string::String> =
                    non_null.iter().map(|t| t.to_string()).collect();
                if has_null {
                    parts.push("nulltype".to_string());
                }
                write!(f, "{}", parts.join(" | "))
            }
            TypeCode::Signature => {
                let params = &self.params[..self.params.len() - 1];
                let ret = self.params.last().unwrap();
                let param_strs: Vec<String> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "({}) -> {}", param_strs.join(", "), ret)
            }
        }
    }
}

// ── Eq / Hash ──

impl PartialEq for ExprType {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.params == other.params
    }
}

impl std::hash::Hash for ExprType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.code.hash(state);
        self.params.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic types ──
    #[test]
    fn basic_types() {
        assert_eq!(ExprType::BOOL.code, TypeCode::Bool);
        assert_eq!(ExprType::INT.code, TypeCode::Int);
        assert_eq!(ExprType::FLOAT.code, TypeCode::Float);
        assert_eq!(ExprType::STRING.code, TypeCode::String);
        assert_eq!(ExprType::PATH.code, TypeCode::Path);
    }

    // ── Display ──
    #[test]
    fn display_int() {
        assert_eq!(ExprType::INT.to_string(), "int");
    }
    #[test]
    fn display_list_int() {
        assert_eq!(ExprType::list(ExprType::INT).to_string(), "list[int]");
    }
    #[test]
    fn display_any() {
        assert_eq!(ExprType::ANY.to_string(), "any");
    }
    #[test]
    fn display_noreturn() {
        assert_eq!(ExprType::NORETURN.to_string(), "noreturn");
    }
    #[test]
    fn display_typevar() {
        assert_eq!(ExprType::T1.to_string(), "T1");
    }
    #[test]
    fn display_union() {
        assert_eq!(
            ExprType::union(vec![ExprType::INT, ExprType::STRING]).to_string(),
            "int | string"
        );
    }
    #[test]
    fn display_nullable() {
        assert_eq!(
            ExprType::union(vec![ExprType::INT, ExprType::NULLTYPE]).to_string(),
            "int?"
        );
    }
    #[test]
    fn display_unresolved_bare() {
        assert_eq!(
            ExprType::unresolved(ExprType::ANY).to_string(),
            "unresolved"
        );
    }
    #[test]
    fn display_unresolved_int() {
        assert_eq!(
            ExprType::unresolved(ExprType::INT).to_string(),
            "unresolved[int]"
        );
    }

    // ── Parsing ──
    #[test]
    fn parse_int() {
        assert_eq!(ExprType::parse("int").unwrap(), ExprType::INT);
    }
    #[test]
    fn parse_float() {
        assert_eq!(ExprType::parse("float").unwrap(), ExprType::FLOAT);
    }
    #[test]
    fn parse_string() {
        assert_eq!(ExprType::parse("string").unwrap(), ExprType::STRING);
    }
    #[test]
    fn parse_bool() {
        assert_eq!(ExprType::parse("bool").unwrap(), ExprType::BOOL);
    }
    #[test]
    fn parse_path() {
        assert_eq!(ExprType::parse("path").unwrap(), ExprType::PATH);
    }
    #[test]
    fn parse_range_expr() {
        assert_eq!(ExprType::parse("range_expr").unwrap(), ExprType::RANGE_EXPR);
    }
    #[test]
    fn parse_nulltype() {
        assert_eq!(ExprType::parse("nulltype").unwrap(), ExprType::NULLTYPE);
    }
    #[test]
    fn parse_noreturn() {
        assert_eq!(ExprType::parse("noreturn").unwrap(), ExprType::NORETURN);
    }
    #[test]
    fn parse_any() {
        assert_eq!(ExprType::parse("any").unwrap(), ExprType::ANY);
    }
    #[test]
    fn parse_list_int() {
        assert_eq!(
            ExprType::parse("list[int]").unwrap(),
            ExprType::list(ExprType::INT)
        );
    }
    #[test]
    fn parse_list_string() {
        assert_eq!(
            ExprType::parse("list[string]").unwrap(),
            ExprType::list(ExprType::STRING)
        );
    }
    #[test]
    fn parse_nested_list() {
        assert_eq!(
            ExprType::parse("list[list[int]]").unwrap(),
            ExprType::list(ExprType::list(ExprType::INT))
        );
    }
    #[test]
    fn parse_optional() {
        let t = ExprType::parse("int?").unwrap();
        assert_eq!(t.code, TypeCode::Union);
        assert_eq!(t.to_string(), "int?");
    }
    #[test]
    fn parse_union() {
        let t = ExprType::parse("int | string").unwrap();
        assert_eq!(t.code, TypeCode::Union);
        assert_eq!(t.to_string(), "int | string");
    }
    #[test]
    fn parse_unresolved_bare() {
        assert_eq!(
            ExprType::parse("unresolved").unwrap(),
            ExprType::unresolved(ExprType::ANY)
        );
    }
    #[test]
    fn parse_unresolved_int() {
        assert_eq!(
            ExprType::parse("unresolved[int]").unwrap(),
            ExprType::unresolved(ExprType::INT)
        );
    }
    #[test]
    fn parse_unresolved_list() {
        assert_eq!(
            ExprType::parse("unresolved[list[string]]").unwrap(),
            ExprType::unresolved(ExprType::list(ExprType::STRING))
        );
    }
    #[test]
    fn parse_unknown_rejects() {
        assert!(ExprType::parse("notavalidtype").is_err());
    }
    #[test]
    fn parse_case_sensitive() {
        assert!(ExprType::parse("INT").is_err());
    }
    #[test]
    fn parse_whitespace_rejected() {
        assert!(ExprType::parse(" int").is_err());
    }

    // ── Roundtrip ──
    #[test]
    fn roundtrip_bare() {
        let t = ExprType::parse("unresolved").unwrap();
        assert_eq!(ExprType::parse(&t.to_string()).unwrap(), t);
    }
    #[test]
    fn roundtrip_constrained() {
        for s in &[
            "unresolved[int]",
            "unresolved[list[string]]",
            "unresolved[float | int]",
        ] {
            let t = ExprType::parse(s).unwrap();
            assert_eq!(
                ExprType::parse(&t.to_string()).unwrap(),
                t,
                "roundtrip failed for {s}"
            );
        }
    }

    // ── Equality / Hash ──
    #[test]
    fn eq_same() {
        assert_eq!(ExprType::INT, ExprType::INT);
    }
    #[test]
    fn eq_diff() {
        assert_ne!(ExprType::INT, ExprType::FLOAT);
    }
    #[test]
    fn eq_list() {
        assert_eq!(ExprType::list(ExprType::INT), ExprType::list(ExprType::INT));
    }
    #[test]
    fn ne_list() {
        assert_ne!(
            ExprType::list(ExprType::INT),
            ExprType::list(ExprType::STRING)
        );
    }
    #[test]
    fn hash_consistent() {
        use std::collections::HashSet;
        let mut s = HashSet::new();
        s.insert(ExprType::INT);
        s.insert(ExprType::FLOAT);
        s.insert(ExprType::INT);
        assert_eq!(s.len(), 2);
    }

    // ── Union normalization ──
    #[test]
    fn union_dedup() {
        assert_eq!(
            ExprType::union(vec![ExprType::INT, ExprType::INT]).code,
            TypeCode::Int
        );
    }
    #[test]
    fn union_single_unwrap() {
        assert_eq!(
            ExprType::union(vec![ExprType::STRING]).code,
            TypeCode::String
        );
    }
    #[test]
    fn union_flatten() {
        let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
        let u2 = ExprType::union(vec![ExprType::FLOAT, ExprType::BOOL]);
        let combined = ExprType::union(vec![u1, u2]);
        assert_eq!(combined.code, TypeCode::Union);
        assert_eq!(combined.params.len(), 4);
        assert_eq!(combined.to_string(), "bool | float | int | string");
    }
    #[test]
    fn union_any_absorbs() {
        assert_eq!(
            ExprType::union(vec![ExprType::INT, ExprType::ANY]).code,
            TypeCode::Any
        );
    }
    #[test]
    fn union_noreturn_collapses() {
        assert_eq!(
            ExprType::union(vec![ExprType::INT, ExprType::NORETURN]),
            ExprType::INT
        );
    }
    #[test]
    fn union_all_noreturn() {
        assert_eq!(
            ExprType::union(vec![ExprType::NORETURN, ExprType::NORETURN]),
            ExprType::NORETURN
        );
    }
    #[test]
    fn union_order_independent() {
        let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
        let u2 = ExprType::union(vec![ExprType::STRING, ExprType::INT]);
        assert_eq!(u1, u2);
    }
    #[test]
    fn union_hash_consistent() {
        use std::collections::HashSet;
        let u1 = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
        let u2 = ExprType::parse("int | string").unwrap();
        let mut s = HashSet::new();
        s.insert(u1);
        s.insert(u2);
        assert_eq!(s.len(), 1);
    }

    // ── Unresolved normalization ──
    #[test]
    fn list_of_unresolved_hoists() {
        let t = ExprType::list(ExprType::unresolved(ExprType::INT));
        assert_eq!(t.code, TypeCode::Unresolved);
        assert_eq!(t, ExprType::unresolved(ExprType::list(ExprType::INT)));
    }
    #[test]
    fn union_with_unresolved_hoists() {
        let t = ExprType::union(vec![ExprType::STRING, ExprType::unresolved(ExprType::INT)]);
        assert_eq!(t.code, TypeCode::Unresolved);
        assert_eq!(
            t,
            ExprType::unresolved(ExprType::union(vec![ExprType::INT, ExprType::STRING]))
        );
    }
    #[test]
    fn nested_unresolved_flattens() {
        let t = ExprType::unresolved(ExprType::unresolved(ExprType::INT));
        assert_eq!(t.code, TypeCode::Unresolved);
        assert_eq!(t, ExprType::unresolved(ExprType::INT));
    }
    #[test]
    fn unresolved_never_inside_list() {
        let t = ExprType::list(ExprType::unresolved(ExprType::STRING));
        assert_eq!(t.code, TypeCode::Unresolved);
        assert_eq!(t.params[0], ExprType::list(ExprType::STRING));
    }

    // ── is_symbolic / is_concrete ──
    #[test]
    fn concrete_int() {
        assert!(ExprType::INT.is_concrete());
    }
    #[test]
    fn concrete_list_int() {
        assert!(ExprType::list(ExprType::INT).is_concrete());
    }
    #[test]
    fn not_concrete_any() {
        assert!(!ExprType::ANY.is_concrete());
    }
    #[test]
    fn not_concrete_union() {
        assert!(!ExprType::union(vec![ExprType::INT, ExprType::STRING]).is_concrete());
    }
    #[test]
    fn not_concrete_typevar() {
        assert!(!ExprType::T1.is_concrete());
    }
    #[test]
    fn symbolic_t1() {
        assert!(ExprType::T1.is_symbolic());
    }
    #[test]
    fn not_symbolic_int() {
        assert!(!ExprType::INT.is_symbolic());
    }
    #[test]
    fn symbolic_list_t1() {
        assert!(ExprType::list(ExprType::T1).is_symbolic());
    }

    // ── match_type ──
    #[test]
    fn match_simple_typevar() {
        let b = ExprType::T1.match_type(&ExprType::INT).unwrap();
        assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
    }
    #[test]
    fn match_nested_typevar() {
        let list_t1 = ExprType::list(ExprType::T1);
        let list_int = ExprType::list(ExprType::INT);
        let b = list_t1.match_type(&list_int).unwrap();
        assert_eq!(b[&TypeCode::TypeVarT1], ExprType::INT);
    }
    #[test]
    fn match_no_match() {
        let list_t1 = ExprType::list(ExprType::T1);
        assert!(list_t1.match_type(&ExprType::INT).is_none());
    }
    #[test]
    fn match_any() {
        assert!(ExprType::ANY.match_type(&ExprType::INT).is_some());
        assert!(ExprType::INT.match_type(&ExprType::ANY).is_some());
    }
    #[test]
    fn match_union_member() {
        let u = ExprType::union(vec![ExprType::INT, ExprType::STRING]);
        assert!(u.match_type(&ExprType::INT).is_some());
        assert!(u.match_type(&ExprType::STRING).is_some());
        assert!(u.match_type(&ExprType::FLOAT).is_none());
    }
    #[test]
    fn match_unresolved_delegates() {
        let t = ExprType::unresolved(ExprType::INT);
        assert!(t.match_type(&ExprType::INT).is_some());
        assert!(t.match_type(&ExprType::STRING).is_none());
    }

    // ── substitute ──
    #[test]
    fn substitute_typevar() {
        let list_t1 = ExprType::list(ExprType::T1);
        let mut bindings = HashMap::new();
        bindings.insert(TypeCode::TypeVarT1, ExprType::INT);
        assert_eq!(list_t1.substitute(&bindings), ExprType::list(ExprType::INT));
    }
}
