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

## Divergence from Python

The Python `SymbolTable` accepts raw Python values and `ExprType` objects in its
constructor, auto-converting them. The Rust version requires `ExprValue` (or types
convertible via `Into<ExprValue>`), making the conversion explicit. The `symtab!` macro
provides equivalent ergonomics.
