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
but unbounded in absolute terms, and `validate_expressions` enforces a third
on the value it materializes:

| Constant | Value | Check |
|---|---|---|
| [`MAX_FORMAT_STRING_LEN`](../../crates/openjd-expr/src/format_string.rs) | 1 MB | Input byte length |
| [`MAX_FORMAT_STRING_SEGMENTS`](../../crates/openjd-expr/src/format_string.rs) | 1,000 | Count of `{{…}}` segments |
| [`MAX_STATIC_RESOLVED_VALUE_LEN`](../../crates/openjd-expr/src/format_string.rs) | 10 MiB | Concatenated `resolved_value` byte length |

The length cap is checked before `parse_segments` runs, so an oversized input
is rejected without allocating a segment vector or touching the parser. The
segment cap is checked after parsing because the segment count is only known
once the scan completes; by that point the `Vec<Segment>` allocation is
already proportional to the input size, so rejecting here is purely a policy
guard and not a further memory-protection measure.

The resolved-value cap exists because the evaluator's per-`evaluate` memory
limit does not compose across segments: `MAX_FORMAT_STRING_SEGMENTS` copies of
`{{ 'A' * 99999999 }}` fit in a 1 MB format string, each evaluates within its
own 100 MB limit, and the concatenation would be ~100 GB — reachable from
untrusted template text at validation time. Past the cap,
`validate_expressions` stops accumulating and reports `resolved_value: None`;
`min_resolved_string_len` keeps counting the full length, which is by itself
enough for a consumer to reject the string. Unlike the other two caps this is
not an error: validation still succeeds and the bound is still exact. The
typed single-expression passthrough performs no concatenation and is not
subject to this cap (it is bounded by the evaluator's own memory limit).

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
`to_display_string()`, so a list result interpolates as a JSON array — see
[values.md](values.md#list-display-strings). The `target_type` field on the options is
ignored here.

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
| `library` | `None` (evaluator falls back to `FunctionLibrary::for_profile(&ExprProfile::current())`) |
| `path_format` | `PathFormat::host()` |
| `target_type` | `None` |

Example — configure every axis:

```rust
// Build a library with host context baked in — this is how
// apply_path_mapping gets its rules.
let profile = ExprProfile::current()
    .with_host_context(HostContext::with_rules(rules));
let lib = FunctionLibrary::for_profile(&profile);

let opts = FormatStringOptions::new()
    .with_library(&*lib)
    .with_path_format(PathFormat::Posix)
    .with_target_type(&ExprType::PATH);

let value  = fs.resolve_with(&symtab, &opts)?;         // ExprValue
let string = fs.resolve_string_with(&symtab, &opts)?;  // String (ignores target_type)
```

Path mapping rules are **not** a format-string option. They belong to the
`apply_path_mapping` closure registered on the library when that library is
built from an [`ExprProfile`](../../crates/openjd-expr/src/profile.rs) whose
`host_context` is `HostContext::WithRules(...)` — see
[function-library.md § Host Context](function-library.md#host-context). Pass
the configured library into `with_library` and the closure handles the rest.

The `with_library` method accepts either `&FunctionLibrary` or
`Option<&FunctionLibrary>` (via `impl Into<Option<...>>`), so callers can plumb
through an already-optional library value without unwrapping it.

## Validation

### validate_expressions — type checking with unresolved values

```rust
let fs = FormatString::new("{{Param.Frame + Param.Name}}")?;
fs.validate_expressions(&unresolved_symtab, &library, None)?;
// → TypeError: cannot add int and string
```

Evaluates each expression with unresolved values to catch type errors at template
validation time, before parameter values are known.

The third argument is the same optional target type the caller will later
pass to `resolve_with`, so that validation observes exactly the values
resolution will produce. It follows the same rule as resolution: for a
format string that is exactly one expression segment and nothing else,
the root value is coerced toward the target — `{{ 1.0 }}` validated with
`Some(&ExprType::INT)` yields the int `1`, one character, not the
three-character float `1.0`. In the concatenated multi-segment case the
target is ignored, mirroring `resolve_string_with`. A value that cannot
coerce to the target fails validation with the same diagnostic resolution
would produce. Unresolved values coerce at the type level
(`ExprValue::coerce` on an `Unresolved` checks the constraint against the
target and stays unresolved), so passing a target also catches
type-level coercion errors statically.

On success, returns a `StaticResolution` describing what that evaluation
determined statically. Callers that only need pass/fail ignore it.

```rust
pub struct StaticResolution {
    pub min_resolved_string_len: usize,
    pub resolved_value: Option<ExprValue>,
    pub resolved_type: ExprType,
}
```

**`min_resolved_string_len`** is a lower bound, in characters, on the length of
any string the format string can resolve to — computed per segment:

| Segment | Contribution (characters) |
|---|---|
| Literal text | its character count |
| Expression → concrete value | interpolated display length (`to_display_string()`; `null` interpolates as the empty string → 0) |
| Expression → value that is or contains `Unresolved` | 0 (may resolve to the empty string) |

Because unresolved segments contribute 0, the bound holds for **every**
possible run-time resolution: if `min_resolved_string_len` already exceeds some
limit on the resolved value, no binding of the unresolved symbols can
produce a conforming string. This lets the model layer enforce
resolved-value constraints (spec limits phrased "after the format string
has been resolved") at template-validation or job-creation time, even
for partially-static strings such as
`"{{ Session.WorkingDirectory }}/{{ 'A' * 10000000 }}"` — the static
segment alone puts the bound over any plausible limit.

Two caveats on interpreting the bound. It describes resolution under the
same `target_type` that was passed to `validate_expressions` — it is only
a valid bound for a resolution using that same target. And for a
single-expression format string whose value is a list, the bound measures
the interpolated display form (`[1, 2, 3]` → 9 characters) while
`resolved_value` is the typed list, so consumers enforcing string-length
limits should only apply it to fields consumed as strings.

**`resolved_value`** is the exact resolved value, present iff every
expression segment evaluated to a concrete value (checked with
`ExprValue::contains_unresolved`, so a list containing an unresolved
element does not count as concrete) **and**, in the concatenated case,
the resulting string is at most `MAX_STATIC_RESOLVED_VALUE_LEN` bytes (see
Defensive Caps). It follows the
[`resolve_with`](#resolve_with--preserves-typed-values-for-single-expression-strings)
typed-passthrough rule: a format string that is exactly one expression
segment and nothing else keeps the typed value (which may be a list or
`null`), coerced toward the given `target_type`; anything else produces
the concatenated string with `null` segments interpolated as empty. When
`resolved_value` is `Some`, `min_resolved_string_len` is exact. The values come
from the evaluation the method already performs for type checking — no
second evaluation occurs, and each segment renders exactly once into a
single buffer that doubles as measuring scratch and the concatenation
being built (segment values are dropped as they are consumed, so peak
memory is bounded by one segment's evaluation plus the capped
concatenation; `String` and `Path` values are counted in place without
rendering at all).

Note that `path()` values inside `resolved_value` are rendered with the
*validating* host's `PathFormat` (the evaluator default), not the
worker's. Character counts are identical either way, so the bound is
unaffected, but the separators in `resolved_value` are not necessarily the
ones the job will see on another host.

**`resolved_type`** is the static type the format string resolves to
under the same target — the type of the value resolution will eventually
produce. The run-time value is never unresolved, so `unresolved[T]`
placeholders read through to their constraint `T` (the marker is a
validation-time artifact; normalization hoists it to the root of a type,
so a single unwrap suffices). Unlike `resolved_value`, the type is
therefore available even when a segment is unresolved: for the
single-expression passthrough it is that expression's value type after
target coercion (`int` for `{{ 1.0 }}` under an `int` target,
`list[string]` for an unresolved `list[string]` symbol under the args
union `nulltype | string | list[string]`), and for every other shape it
is `string`. When a coercion's outcome depends on the unknown payload,
the type is the union of the possible results — an unresolved
`string | list[int]` under the args union resolves to
`string | list[string]`, correctly excluding `nulltype`. This is what
lets a consumer distinguish "resolves to a string, so `min_resolved_string_len`
is a true string-length bound" from "resolves to a typed list, whose
display form the bound merely measures" without inspecting
`resolved_value` — which matters precisely when `resolved_value` is
`None`.

The bound is per-segment, not per-subexpression: a single expression
mixing static and unresolved parts (e.g.
`{{ 'A' * 10000000 + Session.WorkingDirectory }}`) evaluates to
`Unresolved` as a whole and contributes 0.

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
