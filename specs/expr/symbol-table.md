# Symbol Table

## Overview

`SymbolTable` provides hierarchical variable bindings for expression evaluation. It
supports dotted key paths (e.g., `Param.Frame`) by nesting `HashMap`s, matching the
Python implementation's design.

Defined in `symbol_table.rs`.

## Structure

```rust
pub struct SymbolTable {
    table: HashMap<String, SymbolTableEntry>,
}

enum SymbolTableEntry {
    Value(ExprValue),
    Table(SymbolTable),
}
```

A dotted path like `Param.Frame` creates a nested structure: the key `"Param"` maps to
a child `SymbolTable` containing `"Frame"` → `ExprValue::Int(42)`.

## Construction

```rust
// Empty
let symtab = SymbolTable::new();

// From pairs — dotted keys auto-nest
let symtab = SymbolTable::from_pairs(vec![
    ("Param.Frame", ExprValue::Int(42)),
    ("Param.Name", ExprValue::String("shot_01".into())),
]);

// Macro for concise construction
let symtab = symtab! {
    "Param.Frame" => 42,
    "Param.Name" => "shot_01",
    "Param.OutputDir" => ExprValue::Path { value: "/out".into(), format: PathFormat::Posix },
};
```

The `symtab!` macro accepts `impl Into<ExprValue>`, so bare integers, strings, and bools
are automatically converted.

## Dotted Path Operations

```rust
symtab.set("Param.Frame", ExprValue::Int(42));
// Creates: Param → SymbolTable { Frame → Int(42) }

symtab.get("Param.Frame")       // → Some(&ExprValue::Int(42))
symtab.get("Param")             // → None (it's a table, not a value)
symtab.get_table("Param")       // → Some(&SymbolTable)
symtab.contains("Param.Frame")  // → true
symtab.contains("Param")        // → true (table exists)
```

## Path Conflict Detection

Setting a value at a path that conflicts with an existing entry returns an error:

```rust
symtab.set("A.B", ExprValue::Int(1));
symtab.set("A.B.C", ExprValue::Int(2));  // → Err: "A.B" is a value, not a table
symtab.set("A", ExprValue::Int(3));       // → Err: "A" is a table, not a value
```

This prevents ambiguous lookups where a path could be both a value and a table prefix.

## Evaluator Lookup

The evaluator receives an array of symbol table references (`&[&SymbolTable]`) and
searches them in order. This supports stacked scopes — e.g., job parameters in one
table and let-binding variables in another:

```rust
let parsed = ParsedExpression::new("Param.Frame + offset")?;
let result = parsed.evaluator(&[&job_params, &let_bindings]).evaluate()?;
```

The evaluator's `eval_name` and `eval_attribute` methods walk the symbol tables for
simple names and dotted paths respectively. For dotted attribute access like `Param.Frame`,
the evaluator first tries the full dotted path as a variable lookup, then progressively
shorter prefixes with property access on the remainder.

## Unresolved Type Entries

When an `ExprType` is set as a value, it's automatically wrapped in
`ExprValue::unresolved()`:

```rust
symtab.set("Param.Frame", ExprValue::unresolved(ExprType::INT));
// Equivalent to the Python: SymbolTable({"Param.Frame": ExprType.INT})
```

This is used during template validation to build type-only symbol tables from parameter
definitions.

## SerializedSymbolTable

`SerializedSymbolTable` is the cross-process serialization boundary between template
scope and session scope. It wraps a `serde_json::Value` and provides conversion to
a live `SymbolTable` with path format awareness.

Defined in `symbol_table.rs`.

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SerializedSymbolTable(serde_json::Value);
```

### Wire Format

A `SymbolTable` serializes as a JSON array of entries:

```json
[
  { "name": "Param.Frame", "type": "int", "value": "42" },
  { "name": "Param.Name", "type": "string", "value": "shot_01" },
  { "name": "Param.OutputDir", "type": "path", "value": "/out" }
]
```

Each entry has `name` (dotted path), `type` (`ExprType` display string), and `value`
(string representation). `Unresolved` entries are skipped during serialization.

### Deserialization with Path Format

```rust
let serialized: SerializedSymbolTable = serde_json::from_str(json)?;
let symtab = serialized.to_symtab(PathFormat::Posix)?;
```

`to_symtab(path_format)` converts the serialized form back to a live `SymbolTable`.
The `path_format` parameter determines how `PATH`-typed values are constructed —
template scope stores paths in Posix format, while session scope may need host-native
format. This is the key architectural role: it bridges the serialization boundary
between processes that may run on different operating systems.

### JSON Transport for Values

Individual `ExprValue` instances also support JSON transport via `to_json_transport()`
and `from_json_transport()`:

```json
{ "type": "int", "value": "42" }
{ "type": "string", "value": "hello" }
{ "type": "list[int]", "value": ["1", "2", "3"] }
{ "type": "path", "value": "/tmp/out" }
```

This uses the same `{"type", "value"}` encoding as the symbol table entries (minus the
`"name"` field). Both share the underlying `transport_value()` / `from_transport_value()`
methods, so the serialization format is always consistent. See the "Shared Format with
SerializedSymbolTable" subsection in values.md for details.

## Divergence from Python

The Python `SymbolTable` accepts raw Python values and `ExprType` objects in its
constructor, auto-converting them. The Rust version requires `ExprValue` (or types
convertible via `Into<ExprValue>`), making the conversion explicit. The `symtab!` macro
provides equivalent ergonomics.
