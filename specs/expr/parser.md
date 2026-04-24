# Parser

## Overview

The expression language reuses a subset of Python's expression syntax (RFC 0005 §Design Constraints,
item 7). Rather than writing a custom parser, the crate uses `ruff_python_parser` to
parse expressions into a Python AST, then walks the AST with a custom evaluator.

Defined in `eval/parse.rs`.

## Parser Selection: ruff_python_parser

The `ruff_python_parser` from [astral-sh/ruff](https://github.com/astral-sh/ruff) is
used via the `rustpython-ruff_python_parser` crate on crates.io.

The `rustpython-ruff_python_parser` package on crates.io is a republished version of
the same code as `ruff_python_parser` in the `astral-sh/ruff` monorepo. The RustPython
project republishes it under this name. Users seeing `rustpython-ruff_python_parser` in
`Cargo.toml` or `Cargo.lock` should know it is the same parser, not a fork.

**Why ruff over rustpython-parser:**

- rustpython-parser's README states it is "superseded by" the ruff parser
- ruff is actively maintained (daily commits, backed by Astral)
- rustpython-parser has had no substantive code changes since early 2025

```toml
ruff_python_parser = { package = "rustpython-ruff_python_parser", version = "0.15.8" }
ruff_python_ast = { package = "rustpython-ruff_python_ast", version = "0.15.8" }
```

## Parsing Pipeline

```
Input string
    │
    ▼
ruff_python_parser::parse_expression()
    │
    ▼
AST validation (allowlist of allowed node types)
    │
    ▼
Keyword rename fixup (context-sensitive keywords)
    │
    ▼
JSON literal normalization (null→None, true→True, false→False)
    │
    ▼
Symbol reference collection
    │
    ▼
ParsedExpression { ast, keyword_renames, accessed_symbols, called_functions, local_bindings }
```

## Keyword Renaming

Python keywords like `if`, `is`, `not`, `and`, `or`, `in` cannot appear as attribute
names in Python syntax. But the OpenJD expression language needs them to work after `.`
because job parameters can have any name — a parameter named `def` or `is` must be
accessible as `Param.def` or `Param.is`.

The solution:

1. Attempt to parse the expression
2. If parsing fails, scan for Python keywords used as attribute names (after `.`)
3. Replace each keyword with a same-length placeholder identifier to preserve column offsets
4. Re-parse with the placeholders
5. Record the renames in `keyword_renames: HashMap<String, String>` so the evaluator
   can map placeholder names back to the original attribute names

This matches the Python implementation's `ast_parse_keyword_context()` approach, which
renames keywords before parsing and restores them after.

### Reverse mapping at evaluation

The parse-time replacement only rewrites the source; the resulting AST contains the
placeholder names (`xf` for `if`, `xlse` for `else`, etc.). The evaluator's
`eval_attribute` consults the `keyword_renames` map whenever it constructs a symbol
lookup key from an AST `Attribute` node, so a parsed-as-`Param.xf` access becomes a
real lookup of `Param.if` in the symbol table. Error messages also undo the
placeholder: `ExpressionError::with_node` uses the original source text (which still
contains `if`), so users never see the placeholder in diagnostics.

## AST Validation

After parsing, the AST is validated against an allowlist of allowed node types. This
rejects Python features that are syntactically valid but not part of the expression
language:

**Allowed nodes:**
- Expression, IfExp, BoolOp, UnaryOp, Compare, BinOp
- Subscript, Slice, Call, Attribute, Name, Constant
- List, ListComp, comprehension

**Rejected with descriptive errors:**
| Node | Error message |
|------|---------------|
| Lambda | Lambda expressions are not supported |
| Dict, Set | Dict/Set literals are not supported |
| SetComp, DictComp, GeneratorExp | Only list comprehensions are supported |
| Tuple | Tuple literals are not supported; use a list instead |
| NamedExpr (`:=`) | Walrus operator is not supported |
| Starred (`*x`) | Star expressions are not supported |
| JoinedStr (f-strings) | f-strings are not supported |
| Bitwise ops (`&`, `\|`, `^`, `~`, `<<`, `>>`) | Bitwise operations are not supported |
| `is` / `is not` | Identity comparison is not supported; use == or != |
| Keyword arguments | Keyword arguments are not supported |

**Structural checks:**
- List comprehensions: max 1 generator, max 1 `if` clause per generator
- No tuple unpacking in comprehension targets
- Loop variables must start with a lowercase letter (convention from spec)

## JSON Literal Normalization

The expression language accepts JSON-style literals alongside Python-style:

- `null` → treated as Python `None` → `ExprValue::Null`
- `true` → treated as Python `True` → `ExprValue::Bool(true)`
- `false` → treated as Python `False` → `ExprValue::Bool(false)`

These are normalized during AST validation by checking `Name` nodes against these
identifiers.

## Symbol Collection

After parsing, three sets are collected from the AST:

| Set | Purpose |
|-----|---------|
| `accessed_symbols` | All variable references (Name and dotted Attribute chains), excluding loop variables and function names |
| `called_functions` | All function/method names invoked |
| `local_bindings` | Loop variable names from list comprehensions |

These are used by the model layer for template validation — checking that all referenced
symbols exist in the parameter definitions, and that no comprehension variable shadows
a parameter name.

## Multi-line Expressions

Expressions can span multiple lines via implicit line continuation (wrapping in
parentheses). The parser handles this by:

1. Wrapping the expression in `(...)` before parsing
2. Adjusting line numbers in the resulting AST back to the original
3. Error messages show only the relevant line with correct column offsets

## Divergence from Python

The Python implementation uses `ast.parse(expr, mode='eval')` from the standard library.
The Rust implementation uses `ruff_python_parser::parse_expression()` which produces
`ruff_python_ast` types instead of Python `ast` types. The AST structures are nearly
identical, so the evaluator logic translates directly.
