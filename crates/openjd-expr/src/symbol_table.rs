// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Hierarchical symbol table for expression evaluation.
//!
//! Mirrors Python `openjd.expr._symbol_table.SymbolTable`.
//! Supports dotted key paths and nested tables.
//!
//! # Construction
//!
//! ```
//! use openjd_expr::{symtab, SymbolTable, ExprValue, ExprType};
//!
//! // Macro (most concise):
//! let st = symtab! {
//!     "Param.Frame" => 42,
//!     "Param.Name" => "test",
//!     "Session.Dir" => ExprType::PATH,  // auto-wraps as unresolved
//! };
//!
//! // Builder-style:
//! let mut st = SymbolTable::new();
//! st.set("Param.Frame", 42).unwrap();
//! st.set("Param.Name", "test").unwrap();
//!
//! // From iterator:
//! let st: SymbolTable = [
//!     ("Param.Frame", ExprValue::from(42)),
//!     ("Param.Name", "test".into()),
//! ].into_iter().collect();
//! ```

use std::collections::HashMap;
use crate::value::ExprValue;
use crate::types::ExprType;

/// Error returned when a [`SymbolTable::set`] call conflicts with an existing entry.
///
/// For example, setting `"A.B.C"` when `"A.B"` is already a scalar value.
#[derive(Debug, Clone)]
pub struct SymbolTableError {
    pub key: String,
    pub conflict: String,
}

impl std::fmt::Display for SymbolTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cannot set '{}': '{}' is not a table", self.key, self.conflict)
    }
}

impl std::error::Error for SymbolTableError {}

/// Entry in a symbol table: either a nested table or a value.
#[derive(Debug, Clone)]
pub enum SymbolTableEntry {
    Table(SymbolTable),
    Value(ExprValue),
}

/// Hierarchical symbol table mapping names to values or nested tables.
///
/// Supports dotted paths: `table.set("Param.Frame", 42)`
/// creates a nested structure `Param -> Frame -> 42`.
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    pub(crate) table: HashMap<String, SymbolTableEntry>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self { table: HashMap::new() }
    }

    /// Construct from a list of (dotted_key, value) pairs.
    pub fn from_pairs(pairs: Vec<(&str, ExprValue)>) -> Result<Self, SymbolTableError> {
        let mut st = Self::new();
        for (k, v) in pairs { st.set(k, v)?; }
        Ok(st)
    }

    /// Set a nested SymbolTable at a key (for dict-like nesting).
    pub fn set_table(&mut self, key: &str, subtable: SymbolTable) {
        self.table.insert(key.to_string(), SymbolTableEntry::Table(subtable));
    }

    /// Get a subtable at a key, or None.
    pub fn get_table(&self, key: &str) -> Option<&SymbolTable> {
        match self.get(key) {
            Some(SymbolTableEntry::Table(t)) => Some(t),
            _ => None,
        }
    }

    /// Set a value at a dotted path, creating intermediate tables as needed.
    ///
    /// Accepts anything convertible to `ExprValue` via `Into`:
    /// - `i32`, `i64` → `ExprValue::Int`
    /// - `bool` → `ExprValue::Bool`
    /// - `&str`, `String` → `ExprValue::String`
    /// - `ExprType` → `ExprValue::Unresolved` (for type-checking symbol tables)
    /// - `ExprValue` → used directly
    ///
    /// For floats, construct `ExprValue::Float(Float64::new(v)?)` explicitly.
    ///
    /// Returns an error if an intermediate path component is already set to a
    /// value (not a table). For example, setting `"A.B.C"` fails if `"A.B"` is
    /// already a scalar value.
    pub fn set(&mut self, key: &str, value: impl Into<ExprValue>) -> Result<(), SymbolTableError> {
        self.set_value(key, value.into())
    }

    fn set_value(&mut self, key: &str, value: ExprValue) -> Result<(), SymbolTableError> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() == 1 {
            if matches!(self.table.get(key), Some(SymbolTableEntry::Table(_))) {
                return Err(SymbolTableError { key: key.to_string(), conflict: key.to_string() });
            }
            self.table.insert(key.to_string(), SymbolTableEntry::Value(value));
            return Ok(());
        }
        let mut current = self;
        for &part in &parts[..parts.len() - 1] {
            let entry = current.table
                .entry(part.to_string())
                .or_insert_with(|| SymbolTableEntry::Table(SymbolTable::new()));
            current = match entry {
                SymbolTableEntry::Table(t) => t,
                _ => return Err(SymbolTableError {
                    key: key.to_string(),
                    conflict: part.to_string(),
                }),
            };
        }
        let last = parts.last().unwrap().to_string();
        if matches!(current.table.get(&last), Some(SymbolTableEntry::Table(_))) {
            return Err(SymbolTableError { key: key.to_string(), conflict: last });
        }
        current.table.insert(last, SymbolTableEntry::Value(value));
        Ok(())
    }

    /// Set a string value at a dotted path (convenience).
    pub fn set_string(&mut self, key: &str, value: &str) -> Result<(), SymbolTableError> {
        self.set(key, ExprValue::String(value.to_string()))
    }

    /// Get an entry at a dotted path.
    pub fn get(&self, key: &str) -> Option<&SymbolTableEntry> {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = self;
        for (i, &part) in parts.iter().enumerate() {
            match current.table.get(part) {
                Some(SymbolTableEntry::Table(t)) if i < parts.len() - 1 => current = t,
                Some(entry) if i == parts.len() - 1 => return Some(entry),
                _ => return None,
            }
        }
        None
    }

    /// Get a value at a dotted path, returning None if not found or if it's a table.
    pub fn get_value(&self, key: &str) -> Option<&ExprValue> {
        match self.get(key) {
            Some(SymbolTableEntry::Value(v)) => Some(v),
            _ => None,
        }
    }

    /// Get a string value at a dotted path.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.get_value(key) {
            Some(ExprValue::String(s)) => Some(s),
            Some(ExprValue::Path { value, .. }) => Some(value),
            _ => None,
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Top-level keys.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.table.keys().map(|s| s.as_str())
    }

    /// Collect all leaf symbol paths (dotted names) in this table.
    pub fn all_paths(&self, prefix: &str, out: &mut Vec<String>) {
        for (key, entry) in &self.table {
            let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
            match entry {
                SymbolTableEntry::Value(_) => out.push(path),
                SymbolTableEntry::Table(sub) => sub.all_paths(&path, out),
            }
        }
    }

    /// Merge all entries from `other` into this table, overwriting on conflict.
    pub fn merge_from(&mut self, other: &SymbolTable) {
        for (key, entry) in &other.table {
            match entry {
                SymbolTableEntry::Value(v) => {
                    self.table.insert(key.clone(), SymbolTableEntry::Value(v.clone()));
                }
                SymbolTableEntry::Table(sub) => {
                    match self.table.get_mut(key) {
                        Some(SymbolTableEntry::Table(existing)) => existing.merge_from(sub),
                        _ => { self.table.insert(key.clone(), SymbolTableEntry::Table(sub.clone())); }
                    }
                }
            }
        }
    }
}

impl serde::Serialize for SymbolTable {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut paths = Vec::new();
        self.all_paths("", &mut paths);
        // Collect only resolved values — skip Unresolved entries
        let entries: Vec<_> = paths.iter()
            .filter_map(|p| self.get_value(p).and_then(|v| {
                if matches!(v, ExprValue::Unresolved(_)) { None } else { Some((p, v)) }
            }))
            .collect();
        let mut seq = s.serialize_seq(Some(entries.len()))?;
        for (path, value) in entries {
            seq.serialize_element(&serde_json::json!({
                "name": path,
                "value": value.transport_value(),
                "type": value.expr_type().to_string(),
            }))?;
        }
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for SymbolTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let arr: Vec<serde_json::Value> = serde::Deserialize::deserialize(d)?;
        let mut st = SymbolTable::new();
        for entry in &arr {
            let name = entry.get("name").and_then(|n| n.as_str())
                .ok_or_else(|| serde::de::Error::missing_field("name"))?;
            let type_str = entry.get("type").and_then(|t| t.as_str())
                .ok_or_else(|| serde::de::Error::missing_field("type"))?;
            let binding_type = ExprType::parse(type_str).map_err(serde::de::Error::custom)?;
            let raw_value = entry.get("value")
                .ok_or_else(|| serde::de::Error::missing_field("value"))?;
            let value = ExprValue::from_transport_value(
                raw_value, &binding_type, crate::path_mapping::PathFormat::Posix,
            ).map_err(serde::de::Error::custom)?;
            st.set(name, value).map_err(serde::de::Error::custom)?;
        }
        Ok(st)
    }
}

/// Collect `(&str, ExprValue)` pairs into a `SymbolTable`.
///
/// # Panics
///
/// Panics if a dotted path conflicts with an existing non-table entry.
/// Use [`SymbolTable::set`] directly if you need error handling.
impl<'a> FromIterator<(&'a str, ExprValue)> for SymbolTable {
    fn from_iter<I: IntoIterator<Item = (&'a str, ExprValue)>>(iter: I) -> Self {
        let mut st = Self::new();
        for (k, v) in iter { st.set(k, v).expect("SymbolTable path conflict in FromIterator"); }
        st
    }
}

// ═══════════════════════════════════════════════════════════════
// SerializedSymbolTable — boundary type between template and session scope
// ═══════════════════════════════════════════════════════════════

/// A symbol table in JSON transport format.
///
/// This is the boundary type between template scope (always Posix paths) and
/// session scope (host-native paths). The session deserializes it with
/// `PathFormat::host()`, ensuring path separators match the worker OS.
///
/// This mirrors the real-world flow where a scheduler serializes the symbol
/// table to JSON and sends it to a worker that may be on a different OS.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct SerializedSymbolTable(serde_json::Value);

impl SerializedSymbolTable {
    /// Create from a `serde_json::Value` (already-parsed JSON array).
    pub fn from_value(v: serde_json::Value) -> Self { Self(v) }

    /// Create from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        Ok(Self(serde_json::from_str(s)?))
    }

    /// Serialize a `SymbolTable` into transport format.
    pub fn from_symtab(st: &SymbolTable) -> Self {
        Self(serde_json::to_value(st).expect("SymbolTable serialization cannot fail"))
    }

    /// Deserialize to a `SymbolTable` with the given path format.
    ///
    /// Path values in the transport format are plain strings; this method
    /// reconstructs them as `ExprValue::Path` with separators normalized
    /// to the specified format.
    pub fn to_symtab(&self, path_format: crate::path_mapping::PathFormat) -> Result<SymbolTable, String> {
        let arr = self.0.as_array().ok_or("SerializedSymbolTable: expected JSON array")?;
        let mut st = SymbolTable::new();
        for entry in arr {
            let name = entry.get("name").and_then(|n| n.as_str())
                .ok_or("SerializedSymbolTable: missing 'name' field")?;
            let type_str = entry.get("type").and_then(|t| t.as_str())
                .ok_or("SerializedSymbolTable: missing 'type' field")?;
            let binding_type = ExprType::parse(type_str)
                .map_err(|e| format!("SerializedSymbolTable: bad type '{type_str}': {e}"))?;
            let raw_value = entry.get("value")
                .ok_or("SerializedSymbolTable: missing 'value' field")?;
            let value = ExprValue::from_transport_value(raw_value, &binding_type, path_format)
                .map_err(|e| format!("SerializedSymbolTable: {e}"))?;
            st.set(name, value).map_err(|e| format!("SerializedSymbolTable: {e}"))?;
        }
        Ok(st)
    }

    /// Get the underlying JSON value.
    pub fn as_value(&self) -> &serde_json::Value { &self.0 }
}

impl<'de> serde::Deserialize<'de> for SerializedSymbolTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: serde_json::Value = serde::Deserialize::deserialize(d)?;
        Ok(Self(v))
    }
}

/// Construct a [`SymbolTable`] from key-value pairs.
///
/// Values can be any type that implements `Into<ExprValue>`:
/// integers, floats, bools, string literals, `ExprValue`, or
/// `ExprType` (auto-wrapped as unresolved for type checking).
///
/// # Panics
///
/// Panics if a dotted path conflicts with an existing non-table entry.
///
/// ```
/// use openjd_expr::{symtab, ExprType};
///
/// let st = symtab! {
///     "Param.Frame" => 42,
///     "Param.Name" => "test",
///     "Session.Dir" => ExprType::PATH,
/// };
/// ```
#[macro_export]
macro_rules! symtab {
    ($($key:expr => $val:expr),* $(,)?) => {{
        let mut st = $crate::SymbolTable::new();
        $(st.set($key, $val).expect("symtab! path conflict");)*
        st
    }};
}
