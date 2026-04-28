# Format String

## Overview

Format strings are the interpolation mechanism in OpenJD templates (spec §7.3). They
contain literal text and `{{...}}` expressions that are resolved against a symbol table.

Defined in `format_string.rs`. For why this module lives in `openjd-expr` rather than
`openjd-model`, see [architecture.md](architecture.md) § "Why FormatString Lives Here".

## Parsing

`FormatString::new(input)` scans for `{{...}}` pairs and produces a list of segments:

```rust
enum Segment {
    Literal(String),
    /// Parsed EXPR expression — every `{{...}}` is parsed into a
    /// `ParsedExpression` up-front at construction time.
    Expression { start: usize, end: usize, parsed: ParsedExpression },
}
```

Every `{{...}}` — whether a bare dotted name like `{{Param.Frame}}` or a full
expression like `{{Param.X + 1}}` — is parsed into a `ParsedExpression` when
the format string is constructed. `ParsedExpression::as_name_lookup()` is
used downstream to distinguish the base-spec case (bare dotted name) from
complex expressions that require the EXPR extension.

Parsing validates:
- Matched `{{` and `}}` delimiters
- Each expression is syntactically valid (parsed by ruff_python_parser)
- No nested `{{...}}` within expressions

Errors include the position within the format string for precise error reporting.

## Defensive Caps

`FormatString::new` enforces two size limits intended as defense-in-depth
against pathological inputs whose total cost is bounded linearly by input size
but unbounded in absolute terms:

| Constant | Value | Check |
|---|---|---|
| [`MAX_FORMAT_STRING_LEN`](../../crates/openjd-expr/src/format_string.rs) | 1 MB | Input byte length |
| [`MAX_FORMAT_STRING_SEGMENTS`](../../crates/openjd-expr/src/format_string.rs) | 1,000 | Count of `{{…}}` segments |

The length cap is checked before `parse_segments` runs, so an oversized input
is rejected without allocating a segment vector or touching the parser. The
segment cap is checked after parsing because the segment count is only known
once the scan completes; by that point the `Vec<Segment>` allocation is
already proportional to the input size, so rejecting here is purely a policy
guard and not a further memory-protection measure.

These limits are well above the size of any real template field. The spec's
own examples fit in hundreds of bytes, with a handful of `{{…}}` interpolations
per field at most. Hitting either cap means either a malicious input or a
programming bug in an upstream codegen path — both warrant failing loudly.

Exceeding either cap produces an `ExpressionError` whose message names the
violated limit. No dedicated `ExpressionErrorKind` variant is allocated for
this case because callers already treat any `ExpressionError` from
`FormatString::new` as "invalid format string."

## Resolution

Two resolution methods serve different purposes; both take a
[`FormatStringOptions`](#formatstringoptions) configured via chainable `with_*`
builders.

### resolve_string_with — always returns `String`

```rust
let fs = FormatString::new("frame_{{Param.Frame}}_{{Param.Name}}")?;
let result = fs.resolve_string_with(&symtab, &FormatStringOptions::default())?;
// → "frame_42_shot_01"
```

Concatenates all segments into a single string. Expression results are converted via
`to_display_string()`. The `target_type` field on the options is ignored here.

### resolve_with — preserves typed values for single-expression strings

```rust
let fs = FormatString::new("{{Param.Frame}}")?;
let result = fs.resolve_with(&symtab, &FormatStringOptions::default())?;
// → ExprValue::Int(42)  — not a string!
```

Typed-value passthrough applies when the format string consists of **exactly one
expression segment and zero literal segments**. Any surrounding literal text —
even a single whitespace character — forces string conversion. When these
preconditions aren't met, `resolve_with` falls back internally to
`resolve_string_with` and wraps the result in `ExprValue::String`.

## FormatStringOptions

```rust
pub struct FormatStringOptions<'a> { /* private fields */ }

impl<'a> FormatStringOptions<'a> {
    pub fn new() -> Self;                                          // == Default::default()
    pub fn with_library(self, lib: impl Into<Option<&FunctionLibrary>>) -> Self;
    pub fn with_path_format(self, fmt: PathFormat) -> Self;
    pub fn with_target_type(self, t: &ExprType) -> Self;
}
```

Defaults:

| Field | Default |
|---|---|
| `library` | `None` (use `get_default_library()`) |
| `path_format` | `PathFormat::host()` |
| `target_type` | `None` |

Example — configure every axis:

```rust
// Build a library with host context baked in — this is how
// apply_path_mapping gets its rules.
let lib = get_default_library().clone().with_host_context(rules);

let opts = FormatStringOptions::new()
    .with_library(&lib)
    .with_path_format(PathFormat::Posix)
    .with_target_type(&ExprType::PATH);

let value  = fs.resolve_with(&symtab, &opts)?;         // ExprValue
let string = fs.resolve_string_with(&symtab, &opts)?;  // String (ignores target_type)
```

Path mapping rules are **not** a format-string option. They belong to the
`apply_path_mapping` closure registered on the library via
[`FunctionLibrary::with_host_context(rules)`](function-library.md#host-context).
Pass the configured library into `with_library` and the closure handles the rest.

The `with_library` method accepts either `&FunctionLibrary` or
`Option<&FunctionLibrary>` (via `impl Into<Option<...>>`), so callers can plumb
through an already-optional library value without unwrapping it.

## Validation

### validate_expressions — type checking with unresolved values

```rust
let fs = FormatString::new("{{Param.Frame + Param.Name}}")?;
fs.validate_expressions(&unresolved_symtab, &library)?;
// → TypeError: cannot add int and string
```

Evaluates each expression with unresolved values to catch type errors at template
validation time, before parameter values are known.

### validate_comprehension_vars — let binding shadowing check

```rust
fs.validate_comprehension_vars(&let_binding_names)?;
```

Checks that list comprehension loop variables in the format string don't shadow
let-binding names from the enclosing template scope.

## Symbol Table Extraction

### copy_used_symtab_values — build minimal symbol tables

```rust
fs.copy_used_symtab_values(&source_symtab, &mut dest_symtab);
```

Copies only the symbol table entries referenced by the format string's expressions from
`source` into `dest`. Walks each referenced dotted path into the source table, stops at
the first `Value` entry (since the remainder is property/method access, not a symtab
key), and copies that value into `dest` at the same path.

Used by the model layer to build minimal symbol tables for session handoff — only the
parameters actually referenced by a step's format strings are included.

### accessed_symbols — collect referenced symbol names

```rust
let symbols: HashSet<String> = fs.accessed_symbols();
```

Returns the set of symbol names accessed by the format string's expressions, without
copying values. Used by the model layer to detect references to symbols that are absent
from the template-scope symbol table (e.g., `Param.X` for PATH parameters) so it can
include related entries like `RawParam.X` in the filtered symbol table.

## FormatStringValidationError

Structured error returned by `validate_expressions`:

```rust
pub struct FormatStringValidationError {
    pub message: String,  // e.g. "Undefined variable 'Param.X'"
    pub input: String,    // the raw format string
    pub start: usize,     // byte offset of the opening {{
    pub end: usize,       // byte offset past the closing }}
}
```

Carries the position of the failing interpolation within the format string for
caret-style diagnostics or structured error responses.

## Serde Integration

`FormatString` implements `Deserialize` by deserializing as a `String` then parsing:

```rust
impl<'de> Deserialize<'de> for FormatString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        FormatString::new(&s).map_err(serde::de::Error::custom)
    }
}
```

This catches format string syntax errors at template deserialization time, matching the
Python behavior where Pydantic validates format strings on model construction.

## Utility

```rust
/// Escape `{{` and `}}` in a string so the format string parser treats them as literals.
/// Replaces `{{` with `{{ "{{" }}` and `}}` with `{{ "}" + "}" }}` — wrapping the
/// literal brace characters in expression interpolations that produce them as string values.
pub fn escape_format_string(s: &str) -> String;
```

## Divergence from Python

The Python `FormatString` lives in `openjd.model._format_strings` and imports evaluation
machinery from `openjd.expr`. The Rust version lives entirely in `openjd-expr`, which is
architecturally cleaner.

The Python version stores segments as tuples `(literal, expr_string)`. The Rust version
uses a typed enum with pre-parsed `ParsedExpression` objects, avoiding re-parsing on
each resolution call.
