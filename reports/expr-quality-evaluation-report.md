# openjd-expr Quality Evaluation Report

**Date:** 2026-04-20
**Last updated:** 2026-04-20 (post-fix pass)
**Crate:** `openjd-expr` v0.1.0
**Location:** `~/openjd-rs/crates/openjd-expr`
**Spec directory:** `~/openjd-rs/specs/expr/`
**Language reference:** `~/openjd-specifications/wiki/2026-02-Expression-Language.md`

## Executive Summary

The `openjd-expr` crate is the most mature and foundational crate in the `openjd-rs`
workspace. It implements the OpenJD Expression Language (EXPR extension) using
`ruff_python_parser` for Python expression syntax, plus format strings, range
expressions, typed values, a symbol table, a pluggable function library, and
path mapping.

The crate compiles with **zero errors and zero warnings**, `cargo clippy --all-targets
-- -D warnings` is also **clean**, and **all 3,033 tests pass** across 35 integration
test files, 10 inline unit test modules, and 5 doc-tests — in both dev and release
profiles. Specifications are thorough (14 documents, ~14,600 words) and closely
aligned with the implementation. Error messages are consistently high quality,
with caret-positioned source highlighting and edit-distance "did you mean?"
suggestions.

The evaluation identified one **confirmed spec-violation bug** (a failing test has
been written and verified), several undocumented design choices, some avoidable
hot-path clones, a couple of public-API ergonomic issues, and a handful of
specification gaps. None of the issues are severe, and overall the crate is in
excellent shape.

### Fix Status (2026-04-20)

The following issues have been **resolved** on the `expr-fixes` branch:

| Issue | Status | Notes |
|---|---|---|
| 5.1 — list comprehension filter silently accepts non-bool | ✅ FIXED | Red/green TDD, 8 new tests |
| 5.2 — `eval_attribute` clones the AST on every call | ✅ FIXED (2026-04-20) | New `build_dotted_name_from_attr` helper avoids clone |
| 5.3 — `eval_compare` clones the AST per comparison operator | ✅ FIXED (2026-04-20) | `dispatch_with_span` uses `TextRange` directly |
| 5.4 — `ParsedExpression` exposes all fields as `pub` | ✅ FIXED | Fields now `pub(crate)`, 4 accessors added |
| 5.5 — `ParsedExpression` missing `#[must_use]` | ✅ FIXED | Attribute added |
| 5.6 — `path(list)` diverges from `PurePosixPath`/`PureWindowsPath` | ✅ FIXED | 36 new tests, pathlib semantics |
| 5.7 — `any()`/`all()` `list[bool]` restriction not documented | ✅ FIXED | Spec note added |
| 5.9 — `is_truthy()` broader than necessary | ✅ FIXED | Method removed, call sites use explicit `Bool` match |
| 5.12 — Test-file naming (`test_parsing.rs`) | ✅ FIXED (2026-04-20) | Renamed to `test_evaluation.rs` via `git mv` |
| **Bonus** — 3 pre-existing conformance failures on `flatten`/`sorted`/`reversed` | ✅ FIXED | Root cause: empty-list nesting-depth misclassification |

Post-fix verification: **6,092 workspace tests pass**, conformance suite
**1,042 passed / 0 failed** on `2023-09/*` (was 1039/3 before). All CI gates
(fmt, clippy, MSRV 1.92, doc) clean. See §12 "Resolution Log" for details.

---

## 1. Compilation and Test Results

| Check | Command | Result |
|---|---|---|
| Build | `cargo build -p openjd-expr` | Clean, 0 errors, 0 warnings |
| Clippy | `cargo clippy -p openjd-expr --all-targets -- -D warnings` | Clean, 0 errors, 0 warnings |
| Tests (dev) | `cargo test -p openjd-expr` | 3,033 passed, 0 failed, 0 ignored |
| Tests (release) | `cargo test -p openjd-expr --release` | 3,033 passed, 0 failed, 0 ignored |

Test breakdown:
- Inline unit tests (in `src/`): 273 tests across 10 files
- Integration tests (in `tests/`): 2,755 tests across 35 files
- Doc-tests: 5 tests

Additionally, 4 quality-probe tests were written during this evaluation in
`/tmp/expr_probes/` to demonstrate issues — 3 of those fail on current code,
confirming the bug reported in §5.1.

---

## 2. Specifications Review

### 2.1 Files Reviewed

| File | Words | Topic | Assessment |
|---|---|---|---|
| `README.md` | 233 | Index/TOC | Complete navigation |
| `architecture.md` | 987 | Module layout, dependency graph, public API surface | Good — covers design rationale and dependency constraints |
| `type-system.md` | 634 | `ExprType`, `TypeCode`, matching, union normalization, type variables | Adequate — describes structure, lighter on algorithms |
| `values.md` | 2,354 | `ExprValue`, typed list variants, `Float64`, memory sizing, coercion rules | Excellent — design choices, memory layout, algorithms |
| `symbol-table.md` | 852 | Hierarchical symbol table with dotted-path lookup | Good |
| `parser.md` | 854 | `ruff_python_parser` integration, AST validation, keyword renaming | Good — rationale + workaround |
| `evaluator.md` | 1,883 | AST-walking evaluator, resource bounds, dispatch flow, divergence from Python | Excellent |
| `function-library.md` | 2,091 | `FunctionLibrary`, signature dispatch, sub-library composition, host context | Good |
| `format-string.md` | 963 | `FormatString` parsing, resolution, serde integration | Good |
| `error-formatting.md` | 918 | Caret error messages with smart positioning | Good |
| `edit-distance.md` | 379 | Levenshtein "did you mean?" suggestions | Adequate |
| `range-expr.md` | 920 | `RangeExpr` parsing, indexing, iteration, slicing | Good |
| `path-mapping.md` | 901 | `PathFormat`, `PathMappingRule`, URI-aware path operations | Good |
| `path-parse.md` | 662 | Format-aware path parsing (why not `std::path`), path property implementations | Good |

### 2.2 Specification Strengths

- **Design rationale throughout** — `architecture.md`, `evaluator.md`, `values.md`,
  and `parser.md` all explain *why* choices were made (ruff over alternatives,
  typed list variants over uniform `Vec<ExprValue>`, bounded evaluator, `Float64`
  for preserving original string forms).
- **Algorithms, not just API surface** — the evaluator, values, range-expr, and
  format-string specs describe algorithms, not just type signatures.
- **Cross-references** — specs link to each other appropriately (e.g., evaluator
  references value, parser, function-library).
- **Distinguishes EXPR semantics from Python** — documents intentional
  divergences (no truthy concept, no `is`/`is not`, resource bounding).

### 2.3 Specification Gaps

1. **List comprehension `if` clause semantics under-specified.** `evaluator.md`
   and the language spec (§1.3.5) state the `if/else` ternary requires a
   boolean condition with "no truthy concept", but the spec does not explicitly
   extend this rule to the list comprehension filter clause. The code
   implements the filter clause *differently* from the ternary (see §5.1).
2. **`path(list[string])` semantics ambiguous.** `function-library.md` documents
   `path(parts: list[string]) -> path` as "construct path from components" but
   does not specify whether an absolute component in the middle of the list
   replaces prior components (pathlib semantics) or is simply concatenated with
   a separator. The implementation does the latter, which differs from Python's
   `PurePosixPath` constructor. See §5.6.
3. **`any()` / `all()` restriction to `list[bool]` not explicit.** Spec §2.3
   defines `any(values: list[bool]) -> bool` but does not call out that this
   differs from Python's `any()` which accepts any iterable. A brief note on
   this would help users migrating from Python.
4. **Regex per-match cost accounting under-specified.** `evaluator.md` describes
   memory and operation bounding but does not specify per-match cost for
   `re_findall`, `re_replace`, `re_split` beyond the initial string cost. The
   implementation counts `count_string_ops(s.len())` once per call, which is
   adequate but not formally specified.
5. **`ParsedExpression` stability not specified.** None of the specs mention
   whether `ParsedExpression` fields are part of the stable public API. The
   implementation exposes them all as `pub` including the external-crate `ast`
   field — see §5.8.
6. **Empty-expression error text not specified.** The code returns
   `"Empty expression"` without source context, but `error-formatting.md`
   doesn't mention this as a non-caret error case.

---

## 3. Implementation Review

### 3.1 Files Reviewed

| Path | Lines | Role | Inline tests |
|---|---|---|---|
| `src/lib.rs` | 42 | Crate root, module declarations, re-exports | No |
| `src/types.rs` | 980 | `ExprType`, `TypeCode`, matching, normalization, parsing, substitution | Yes (64) |
| `src/value.rs` | 1,175 | `ExprValue`, `Float64`, typed list variants, memory sizing, coercion, display | No |
| `src/symbol_table.rs` | 452 | `SymbolTable` with dotted-path lookup, merge, serde, `symtab!` macro | No |
| `src/eval/mod.rs` | 15 | Submodule declarations | No |
| `src/eval/parse.rs` | 881 | `ParsedExpression`, keyword renaming, AST validation, `EvalBuilder`, symbol collection | Yes (9) |
| `src/eval/evaluator.rs` | 1,443 | Core evaluator: AST dispatch, memory/op tracking, `eval_*` methods, `EvalContext` | No |
| `src/function_library.rs` | 869 | `FunctionLibrary`: registration, 3-phase dispatch, coercion, `derive_return_type` | Yes (16) |
| `src/default_library.rs` | 800 | Default library construction: registers all built-in signatures | Yes (13) |
| `src/functions/mod.rs` | 16 | Submodule declarations | No |
| `src/functions/arithmetic.rs` | 404 | Int/float operators (+, −, *, /, //, %, **), string/list concat | No |
| `src/functions/comparison.rs` | 222 | Equality, ordering, containment, slicing | No |
| `src/functions/conversion.rs` | 141 | Type conversion (int, float, string, bool) | No |
| `src/functions/list.rs` | 197 | List functions (sorted, reversed, unique, flatten, range, join, range_expr) | No |
| `src/functions/math.rs` | 232 | Math (min, max, floor, ceil, round, sum) | No |
| `src/functions/misc.rs` | 255 | Misc (fail, zfill, any, all, abs, len, path, path_join, getitem) | No |
| `src/functions/path.rs` | 448 | Path methods (as_posix, with_name, with_stem, with_suffix, with_number) | No |
| `src/functions/path_parse.rs` | 691 | Format-aware path parsing (parent, name, stem, suffix, parts, is_absolute) | Yes (61) |
| `src/functions/regex.rs` | 253 | Regex functions (re_match, re_search, re_findall, re_replace, re_split, re_escape) | No |
| `src/functions/repr.rs` | 245 | Repr functions (repr_py, repr_json, repr_sh, repr_cmd, repr_pwsh) | No |
| `src/functions/string.rs` | 383 | String methods | No |
| `src/format_string.rs` | 1,109 | `FormatString` parsing, resolution, serde Deserialize/Serialize, validation | Yes (46) |
| `src/range_expr.rs` | 862 | `RangeExpr` parsing, `IntRange`, iteration, indexing, slicing, `from_values` | Yes (25) |
| `src/path_mapping.rs` | 325 | `PathFormat`, `PathMappingRule` with apply logic (filesystem + URI) | Yes (7) |
| `src/uri_path.rs` | 247 | URI-aware path operations (parse, name, parent, suffix, stem, parts, join) | Yes (22) |
| `src/error.rs` | 404 | `ExpressionError`, `ExpressionErrorKind`, caret formatting, `Display` | No |
| `src/edit_distance.rs` | 125 | Levenshtein distance, `suggest_closest` | Yes (10) |

Total source: ~12,764 lines.

### 3.2 Cargo.toml Dependencies

- `shlex` — shell quoting for `repr_sh`
- `thiserror` — error derive
- `regex` — regex functions
- `serde` + `serde_json` — serialization / JSON transport
- `ruff_python_parser`, `ruff_python_ast`, `ruff_text_size` (all as
  `rustpython-ruff_*` v0.15.8) — the crates.io republishes of the Astral ruff
  parser. A detailed justification in `Cargo.toml` explains this choice.

### 3.3 Implementation Strengths

- **Clean, consistent module layout.** Submodules are grouped logically
  (eval, functions), public API is re-exported from `lib.rs` only.
- **Typed list variants.** `ListInt(Vec<i64>)`, `ListString(Vec<String>, usize)`,
  etc. provide memory efficiency vs. a uniform `Vec<ExprValue>` and enable
  O(1) element-type queries.
- **`Float64` preserves original string form.** When a float is parsed from
  literal text, its original string is cached so `repr_py` can reproduce it
  bitwise — important for matching the Python reference.
- **Three-phase dispatch.** `FunctionLibrary` tries exact match → coerced match
  → generic match. Keeps hot paths cheap and error cases informative.
- **Bounded evaluation.** Memory and operation caps (100 MB and 10 M ops by
  default) are enforced uniformly; attempts to exceed produce clear errors.
- **Error messages.** Every evaluation error includes a message, the source
  line, and a caret indicator pointing at the exact location — see §6.
- **Edit-distance suggestions.** Undefined variable and unknown-method errors
  include "did you mean?" suggestions computed from the symbol table and
  available methods.
- **Cross-implementation parity.** JSON serialization round-trips `ExprValue`
  losslessly, including preserved-form floats; `FormatString` parsing mirrors
  the Python implementation.

---

## 4. Spec ↔ Implementation Cross-Check

| Spec file | Source files | Alignment |
|---|---|---|
| `architecture.md` | `lib.rs`, `Cargo.toml` | ✅ Module layout, dependency graph, and public re-exports all match |
| `type-system.md` | `types.rs` | ✅ `TypeCode` variants, `ExprType` structure, normalization rules match |
| `values.md` | `value.rs` | ✅ Typed list variants, `Float64`, memory sizing, coercion all match |
| `symbol-table.md` | `symbol_table.rs` | ✅ Structure, dotted-path lookup, conflict handling match |
| `parser.md` | `eval/parse.rs` | ✅ Keyword-renaming strategy, retry loop, AST validation match; minor note below |
| `evaluator.md` | `eval/evaluator.rs` | ⚠️ Mostly accurate — one alignment issue, see §5.1 |
| `function-library.md` | `function_library.rs`, `default_library.rs` | ✅ 3-phase dispatch matches |
| `format-string.md` | `format_string.rs` | ✅ Parsing, resolution, serde integration match |
| `error-formatting.md` | `error.rs` | ✅ Caret positioning and display format match |
| `edit-distance.md` | `edit_distance.rs` | ✅ Algorithm and API match |
| `range-expr.md` | `range_expr.rs` | ✅ Syntax, normalization, slicing match |
| `path-mapping.md` | `path_mapping.rs`, `uri_path.rs` | ✅ `PathFormat`, apply logic, URI handling match |
| `path-parse.md` | `functions/path_parse.rs` | ✅ Format-aware parsing rationale and impl match |

Minor note on `parser.md`: the spec references "ruff_python_parser from astral-sh/ruff"
but `Cargo.toml` uses `rustpython-ruff_python_parser`. The `Cargo.toml` comment
explains this is the crates.io republish of the same code by the RustPython
project, so there's no inconsistency, but `parser.md` could add one line noting
the crates.io package name users will see in `Cargo.lock`.

Overall: specs are well-aligned with the implementation. No stale content.

---

## 5. Issues Found

Each issue below includes a location, quoted code, a verdict (BUG,
DESIGN-CHOICE-UNDOCUMENTED, INTENTIONAL, ERGONOMICS), and a suggested fix.

### 5.1 BUG — list comprehension filter silently accepts non-bool values ✅ FIXED (2026-04-20)

**File:** `src/eval/evaluator.rs:1303-1310`

```rust
if let Some(if_clause) = gen.ifs.first() {
    let cond = child.evaluate(if_clause)?;
    if let ExprValue::Bool(b) = cond {
        include = b;
    }
    // Silent pass-through: if cond is NOT Bool, `include` stays `true`
}
```

Compare with the ternary `if/else` at `src/eval/evaluator.rs:847-857`, which
correctly enforces the bool requirement:

```rust
if !matches!(&test, ExprValue::Bool(_)) {
    let err = ExpressionError::new(format!(
        "Condition must be a boolean, got {}",
        test.expr_type()
    ));
    ...
}
```

**Spec (2026-02-Expression-Language.md §1.3.5):**

> The `<condition>` must be a `bool`; there is no 'truthy' concept.

**Reproducer (verified failing by running as a test in this crate):**

```rust
// Expected: type error. Actual: Ok(ListInt([1, 2, 3]))
eval("[x for x in [1, 2, 3] if x]")

// Expected: type error. Actual: Ok(ListString(["a", "b", ""], 74))
eval("[x for x in ['a', 'b', ''] if x]")

// Expected: type error. Actual: Ok(ListInt([0, 1, 2, 3])) — 0 is NOT filtered
eval("[x for x in [0, 1, 2, 3] if x]")
```

The third case is the most surprising: because the `include = true` default is
never changed for non-bool values, the filter degrades to **no filtering at
all**, not even "truthy" filtering. A user who writes `if x` (expecting truthy
semantics) will get everything back including zeros and empty strings.

**Verdict: BUG.** Violates the spec, contradicts the ternary's correct
enforcement, and produces silently wrong results.

**Fix (3 lines):**

```rust
if let Some(if_clause) = gen.ifs.first() {
    let cond = child.evaluate(if_clause)?;
    match cond {
        ExprValue::Bool(b) => include = b,
        ExprValue::Unresolved(_) => { /* static type check path */ }
        other => return Err(ExpressionError::new(format!(
            "List comprehension filter must be a boolean, got {}",
            other.expr_type()
        )).with_node(self.expr_source.unwrap_or(""), if_clause)),
    }
}
```

A failing test demonstrating the bug is saved at
`/tmp/expr_probes/test_listcomp_filter.rs`.

### 5.2 DESIGN-CHOICE-UNDOCUMENTED — `eval_attribute` clones the AST on every call ✅ FIXED (2026-04-20)

**File:** `src/eval/evaluator.rs:480`

```rust
fn eval_attribute(&mut self, a: &ast::ExprAttribute) -> Result<ExprValue, ExpressionError> {
    // Try full dotted path lookup, resolving keyword renames
    let dotted_path = build_dotted_name(&ast::Expr::Attribute(a.clone()));  // ← always clones
    ...
}
```

`build_dotted_name` (line 1406) only reads the tree by reference:

```rust
fn build_dotted_name(expr: &ast::Expr) -> Option<String> {
    let mut parts = Vec::new();
    let mut current = expr;
    loop {
        match current {
            ast::Expr::Name(n) => { parts.push(n.id.as_str()); break; }
            ast::Expr::Attribute(a) => { parts.push(a.attr.as_str()); current = &a.value; }
            _ => return None,
        }
    }
    ...
}
```

The clone is on the hot path — every `Param.Frame`-style lookup pays for a
deep clone of the `ast::ExprAttribute` (which contains `Box<Expr>` for the
receiver). This is the single most impactful perf issue found.

**Verdict: DESIGN-CHOICE-UNDOCUMENTED.**

**Fix:** Add a variant of `build_dotted_name` that takes `&ast::ExprAttribute`
directly:

```rust
fn build_dotted_name_from_attr(a: &ast::ExprAttribute) -> Option<String> {
    let mut parts: Vec<&str> = vec![a.attr.as_str()];
    let mut current: &ast::Expr = &a.value;
    loop {
        match current {
            ast::Expr::Name(n) => { parts.push(n.id.as_str()); break; }
            ast::Expr::Attribute(a) => { parts.push(a.attr.as_str()); current = &a.value; }
            _ => return None,
        }
    }
    parts.reverse();
    Some(parts.join("."))
}
```

The error-reporting branch (`let attr_node = ast::Expr::Attribute(a.clone());`)
at line ~497 still clones, but it only runs when the dotted-path lookup fails
*and* subsequent evaluation also fails, so it's no longer on the hot path.

### 5.3 DESIGN-CHOICE-UNDOCUMENTED — `eval_compare` clones the AST per comparison operator ✅ FIXED (2026-04-20)

**File:** `src/eval/evaluator.rs:734-763`

```rust
let mut left = self.evaluate(&c.left)?;
for (op, right_node) in c.ops.iter().zip(c.comparators.iter()) {
    let right = self.evaluate(right_node)?;
    ...
    let (op_name, args) = match op {
        ast::CmpOp::Eq => ("__eq__", vec![left.clone(), right.clone()]),
        ast::CmpOp::NotEq => ("__ne__", vec![left.clone(), right.clone()]),
        ast::CmpOp::Lt => ("__lt__", vec![left.clone(), right.clone()]),
        ast::CmpOp::LtE => ("__le__", vec![left.clone(), right.clone()]),
        ast::CmpOp::Gt => ("__gt__", vec![left.clone(), right.clone()]),
        ast::CmpOp::GtE => ("__ge__", vec![left.clone(), right.clone()]),
        ast::CmpOp::In => ("__contains__", vec![right.clone(), left.clone()]),
        ast::CmpOp::NotIn => ("__not_contains__", vec![right.clone(), left.clone()]),
        ...
    };
    // Build a synthetic node spanning left..right for error caret positioning
    let node = ast::Expr::Compare(c.clone());            // ← clones full ExprCompare
    let result_val = self.dispatch_with_node(op_name, args, Some(&node))?;
```

For `1 < 2 < 3 < 4`, each iteration clones `left`, `right`, and the entire
`ExprCompare` node. The value clones are required by the dispatcher signature
(takes `Vec<ExprValue>` by value for memory accounting), but the `ExprCompare`
clone is solely for error-caret positioning. Since `c: &ast::ExprCompare`,
`dispatch_with_node` could accept the range and op directly, or a
`&ast::ExprCompare`, avoiding the clone entirely.

Additionally, after the final iteration `left` is overwritten by `right` so
the last `right.clone()` is wasted (though this is minor).

**Verdict: DESIGN-CHOICE-UNDOCUMENTED.**

**Fix:** Change `dispatch_with_node` to accept a `ruff_text_size::TextRange`
and op name, or thread a `&ast::Expr` through and borrow the caller's `c`
wrapped appropriately. The former is simpler because callers are already
passing just enough context to locate the error.

### 5.4 DESIGN-CHOICE-UNDOCUMENTED — `ParsedExpression` exposes all fields (including external AST) as `pub` ✅ FIXED (2026-04-20)

**File:** `src/eval/parse.rs:13-21`

```rust
#[derive(Debug, Clone)]
pub struct ParsedExpression {
    pub ast: ast::Expr,                                   // ruff_python_ast::Expr
    pub expr: String,
    pub source: String,
    pub keyword_renames: HashMap<String, String>,
    pub accessed_symbols: HashSet<String>,
    pub called_functions: HashSet<String>,
    pub local_bindings: HashSet<String>,
}
```

This leaks implementation details into the public API:

- `pub ast: ast::Expr` exposes a type from `rustpython-ruff_python_ast`, a
  third-party crate that the spec at `parser.md` acknowledges may be replaced
  in the future. Any downstream code reading `parsed.ast` is coupled to the
  parser choice.
- `pub source`, `pub expr`, `pub keyword_renames` expose internal bookkeeping
  that users should not need to read or modify.
- Direct field mutation could break invariants (e.g., `keyword_renames` must
  match what was actually renamed in `source`).

**Verdict: DESIGN-CHOICE-UNDOCUMENTED** and **ergonomics/stability risk**.

**Fix:** Make fields `pub(crate)` and add accessor methods for the ones users
genuinely need:

```rust
impl ParsedExpression {
    pub fn expression(&self) -> &str { &self.expr }
    pub fn accessed_symbols(&self) -> &HashSet<String> { &self.accessed_symbols }
    pub fn called_functions(&self) -> &HashSet<String> { &self.called_functions }
    pub fn local_bindings(&self) -> &HashSet<String> { &self.local_bindings }
}
```

The `ast` field should not have a public accessor — callers that need to
traverse the AST should either use `accessed_symbols()` / `called_functions()`
or work with the evaluator API directly.

### 5.5 ERGONOMICS — `ParsedExpression` is missing `#[must_use]` ✅ FIXED (2026-04-20)

**File:** `src/eval/parse.rs:12-13`

The struct has no `#[must_use]` annotation, unlike `EvalBuilder`
(`src/eval/parse.rs:276`) which correctly carries one. `ParsedExpression` is
useless without a subsequent call to `.evaluate()` or `.eval_builder()`, so
discarding it is always a programming mistake worth a lint.

**Verdict: ERGONOMICS — trivial one-line fix.**

```rust
#[must_use]
#[derive(Debug, Clone)]
pub struct ParsedExpression { ... }
```

### 5.6 DESIGN-CHOICE-UNDOCUMENTED — `path(list[string])` does not match Python `PurePosixPath(*parts)` ✅ FIXED (2026-04-20)

**File:** `src/functions/misc.rs:131-155`

The `path()` function on a list input concatenates with separator without
pathlib-style replacement when an element is absolute:

```
path(['a', '/b'])  →  Path { value: "a//b", format: Posix }
```

Python's `PurePosixPath('a', '/b')` returns `PurePosixPath('/b')` (absolute
component wins). The `/` operator in this crate correctly implements the
replacement rule (see `function-library.md` §2.1.5), but the `path(list)`
constructor takes the simpler "join with separator" path.

Whether this is right depends on intent. If `path(list)` is "construct from
components" (like `os.path.sep.join`), the current behavior is correct. If it's
"construct via pathlib semantics", it's wrong.

**Verdict: DESIGN-CHOICE-UNDOCUMENTED.** The spec is silent on absolute-component
handling, and the code's behavior surprises Python users.

**Fix:** Either

1. Document the current "join with separator" behavior explicitly in
   `function-library.md`, noting the divergence from `pathlib`, or
2. Change the implementation to match `pathlib` (absolute component resets the
   accumulator). Recommend (1) as less disruptive since existing templates
   likely rely on the current behavior.

### 5.7 DESIGN-CHOICE-UNDOCUMENTED — `any()` / `all()` type restriction surprises Python users ✅ FIXED (2026-04-20)

**File:** `src/default_library.rs:461-464` (signatures) and
`src/functions/misc.rs:51-80` (implementation)

The type system restricts `any()` and `all()` to `list[bool]` before the
implementation is reached, so the internal `is_truthy()` call is effectively
only ever invoked on `Bool` values. This matches the spec's
"no truthy concept" stance and is a deliberate choice.

```
any([1, 2, 3])  →  Err("No matching signature for any(list[int])")
any([True, False])  →  Ok(Bool(true))
```

**Verdict: INTENTIONAL — spec-conformant**, but the spec should call out the
divergence from Python's `any`/`all` (which accept any iterable) so users
migrating from Python aren't surprised.

### 5.8 INTENTIONAL — `truediv` precision loss for large integers

**File:** `src/functions/arithmetic.rs:53-57`

```rust
pub fn truediv_int(_: Ctx, a: &[ExprValue]) -> R {
    match (&a[0], &a[1]) {
        (ExprValue::Int(l), ExprValue::Int(r)) => {
            if *r == 0 { return Err(ExpressionError::division_by_zero("Division")); }
            Ok(ExprValue::Float(Float64::new(*l as f64 / *r as f64)?))
        }
        ...
    }
}
```

`(2**53 + 1) / 1` produces `9007199254740992.0` (rounds to 2^53) because f64
cannot represent integers > 2^53 exactly. Python's `/` on ints has the same
behavior (also returns a float and loses precision). This is matching Python
semantics exactly.

**Verdict: INTENTIONAL — Python parity.** Could optionally document in
`function-library.md` or `values.md` for clarity.

### 5.9 OBSERVATION — `is_truthy()` broader than necessary but functionally safe ✅ FIXED (2026-04-20)

**File:** `src/value.rs:958-975`

The `is_truthy()` method implements Python-style truthiness (empty string is
falsy, 0 is falsy, etc.). In this crate, its only call sites are:

1. `any()` / `all()` — only reachable with `list[bool]` (type system enforces),
   so effectively reduces to `== true`.
2. The list comprehension filter bug in §5.1 — where the expected semantics
   are "must be a bool, no truthy concept", so the method should not be called
   there.

So `is_truthy()` itself is technically unreachable for non-bool values on
current code paths except through the bug. Fixing §5.1 makes the broader
behavior fully unreachable.

**Verdict: INTENTIONAL.** Consider either renaming to `as_bool()` (since it's
only safely called on `Bool`) or removing it entirely after fixing §5.1, to
eliminate a subtle footgun.

### 5.10 OBSERVATION — `collect_symbol_names()` walks the full symbol-table tree on every error

**File:** `src/eval/evaluator.rs:313, 466, 499`

Called on the error path of `eval_name` and `eval_attribute` for "did you
mean?" suggestions. It walks every symbol table reference, collects all dotted
paths, then sorts and deduplicates — O(total entries log N). This is
error-path only and is perfectly acceptable; noted here for completeness.

**Verdict: INTENTIONAL — acceptable cost on error paths.**

### 5.11 OBSERVATION — No fuzz tests or benchmarks

- No `benches/` directory. Given the crate is used in hot paths
  (expression evaluation during job parameter expansion), a small `criterion`
  benchmark for common operations (arithmetic, list comprehension, format
  string resolution) would help catch performance regressions.
- No fuzz tests. The parser accepts arbitrary strings (via `ruff`) and the
  evaluator accepts arbitrary ASTs. A `cargo fuzz` harness on
  `ParsedExpression::new` + `evaluate` would be a cheap way to surface panics
  or infinite loops.

**Verdict: TEST-GAP (non-blocking).**

### 5.12 OBSERVATION — Test-file naming is mildly confusing ✅ FIXED (2026-04-20)

`tests/test_parsing.rs` (1,179 lines, 220 tests) primarily exercises expression
**evaluation**, not parsing. Parsing-behavior tests live in
`tests/test_parse_expression.rs` (483 lines, 64 tests, tests `ParsedExpression`
API like `accessed_symbols`, `called_functions`) and
`tests/test_ast_validation.rs` (rejection of unsupported syntax). Renaming
`test_parsing.rs` to `test_evaluation.rs` or `test_expressions.rs` would match
the content.

**Verdict: MINOR — naming only.**

---

## 6. Error Message Quality

Sampled 5 error cases from `tests/test_error_formatting.rs`:

| Test | Message | Caret | Quality |
|---|---|---|---|
| `type_error_in_middle` | "Cannot convert 'bad' to int" | Points at `int('bad')` | Excellent |
| `operator_error_friendly_name` | "Cannot use '+' operator with string and int" | Spans the operator | Excellent |
| `method_on_wrong_type` | "startswith() is not available for path. Available for: string" | Points at method | Excellent — shows valid types |
| `attribute_without_call_suggests_parens` | "'upper' is a method, not a property. Did you mean upper()?" | Points at attribute | Excellent — actionable suggestion |
| `undefined_variable_with_suggestion` | "Did you mean: Param.Frame" | Points at variable | Excellent — edit-distance suggestion |

Each error includes:

1. A clear human-readable message
2. The full expression source line
3. A caret `^~~~~~` spanning the exact error location
4. Contextual suggestions where applicable

No issues found with caret positioning, unhelpful suggestions, or missing
context. The test coverage of error formatting (90 tests in
`test_error_formatting.rs` alone) is a significant asset.

---

## 7. Algorithmic Complexity Scan

| Component | Complexity | Assessment |
|---|---|---|
| `types.rs` union normalization | O(n log n) — sort + dedup | OK, n = union members (typically ≤5) |
| `types.rs` match_type union×union | O(m×n) nested loop | OK, m,n typically ≤5 |
| `function_library.rs` dispatch | HashMap by name O(1) + O(k) linear scan over overloads | OK, k typically 1–5 |
| `eval/evaluator.rs` listcomp | O(n) per element, op-counted | OK |
| `eval/evaluator.rs` `in` operator | O(n) linear scan | Optimal for unsorted lists |
| `format_string.rs` parsing | O(n) single pass with `find()` | OK |
| `symbol_table.rs` lookups | O(d) where d = path depth | OK — HashMap at each level |
| `symbol_table.rs` `all_paths()` | O(total entries) tree walk | OK — error path only |
| `collect_symbol_names()` | O(total) + O(n log n) sort | OK — error path only |

**No O(N²) issues found.** `unique_fn` uses `HashSet` for O(n) dedup.
`contains_list` is O(n) which is optimal for unsorted lists without a hash
index.

---

## 8. Rust Best Practices

| Check | Finding |
|---|---|
| `unwrap()` in non-test code | All sites guarded by preceding invariants (evaluator.rs:498, 1094, 1152; types.rs:165, 231, 376, 379, 567; range_expr.rs:290, 295). Clean. |
| `panic!` in non-test code | Only `function_library.rs:166` (`register_sig` on invalid signature literal — a builder-time assertion equivalent to a compile-time check). Justified. |
| `unsafe` blocks | None. |
| `clone()` overuse | Two hot-path clones avoidable (§5.2, §5.3). Other clones (`a[0].clone().into_list()`) are required by consuming APIs. |
| `String` vs `&str` parameters | Public API uses `&str` appropriately. |
| `Vec` vs `&[T]` parameters | Dispatcher takes `Vec<ExprValue>` because value construction requires ownership; acceptable. |
| `#[must_use]` | Applied consistently on builders and `EvalBuilder`. Missing on `ParsedExpression` (§5.5). |
| `Default` impls | `FunctionLibrary::new()` could derive `Default` but explicit `new()` is fine. |
| `non_exhaustive` | Applied on `ExprValue::Path` variant to enforce invariants via `new_path()`. Good. |

---

## 9. Cross-Crate Naming Consistency

| Crate | Error Type | Pattern |
|---|---|---|
| `openjd-expr` | `ExpressionError` + `ExpressionErrorKind` | Domain-prefixed + Kind enum |
| `openjd-model` | `OpenJdError` | Project-prefixed |
| `openjd-sessions` | `SessionError` | Domain-prefixed |
| `openjd-snapshots` | `SnapshotError` | Domain-prefixed |

Minor inconsistency: `openjd-model` uses `OpenJdError` while all other crates
use domain-specific names. Not harmful in practice (each crate is used
independently), but if a future refactor touches error types, renaming
`OpenJdError` → `ModelError` would bring `openjd-model` in line.

Module naming (`pub mod error`), builder patterns, and `Result<T, *Error>`
return-type conventions are consistent across all four crates.

---

## 10. Recommendations

Listed in priority order.

### High priority

1. ✅ **Fix the list-comprehension filter bug (§5.1).** DONE. Three-line code
   change in `eval_listcomp` plus matching `Unresolved` handling; 8 new tests
   cover runtime and static-type-check paths.

### Medium priority

2. ✅ **Eliminate the `eval_attribute` AST clone (§5.2).** DONE. Added
   `build_dotted_name_from_attr` that takes `&ast::ExprAttribute` directly,
   removing the hot-path clone.
3. ✅ **Reduce `eval_compare` AST clones (§5.3).** DONE. Added
   `dispatch_with_span` that accepts a `TextRange` instead of cloning the
   full `ExprCompare` node for error positioning.
4. ✅ **Tighten the public API of `ParsedExpression` (§5.4).** DONE. Fields
   are now `pub(crate)` or private; four accessor methods (`expression()`,
   `accessed_symbols()`, `called_functions()`, `local_bindings()`) form the
   public surface. 18 external call sites updated.
5. ✅ **Add `#[must_use]` to `ParsedExpression` (§5.5).** DONE.

### Low priority / documentation

6. ✅ **Document `path(list[string])` component semantics (§5.6).** DONE.
   Implementation changed to match `PurePosixPath` / `PureWindowsPath`
   exactly (absolute resets, empty-string skip, dot collapse, drive/root
   handling on Windows, UNC support). Spec updated in
   `specs/expr/function-library.md`.
7. ✅ **Note in the spec that `any()`/`all()` require `list[bool]` (§5.7).**
   DONE. One-bullet note added to `specs/expr/function-library.md`.
8. ✅ **Clarify list-comprehension `if` clause rule in the language spec.** DONE.
   Added explicit statement in §1.3.7 of the canonical spec that the `if`
   filter clause requires `bool`, referencing §1.3.5's "no truthy concept" rule.
9. ✅ **Document the `truediv` precision note (§5.8).** DONE. Added precision
   note to `specs/expr/values.md` in the Float64 section.
10. ✅ **Remove `is_truthy()` (§5.9).** DONE. Method deleted; call sites in
    `any()`, `all()`, and `eval_ifexp` now use explicit `Bool` pattern
    matching.
11. ✅ **Rename `tests/test_parsing.rs` to match its content (§5.12).** DONE.
    Renamed to `test_evaluation.rs` via `git mv`.

### Optional enhancements

12. **Add a `benches/` directory with `criterion` benchmarks** for the most
    common evaluation patterns (arithmetic, comparisons, list
    comprehensions, format string resolution). Helps catch perf regressions.
13. **Add a `cargo fuzz` harness** for `ParsedExpression::new` and
    `ParsedExpression::evaluate`. Given the external-input attack surface,
    fuzz coverage would be valuable.
14. **Consider renaming `OpenJdError` → `ModelError` in `openjd-model`** for
    cross-crate consistency (§9). Breaking change; bundle with other v0.x
    breaking changes if any.
15. ✅ **Add parser-package-name note to `specs/expr/parser.md`.** DONE.
    Clarifies that `rustpython-ruff_python_parser` on crates.io is the same
    code as `ruff_python_parser` in the `astral-sh/ruff` monorepo.

---

## 11. Conclusion

`openjd-expr` is a **high-quality, well-engineered crate**. The specifications
are thorough, the implementation is clean (zero clippy or compiler warnings,
no `unsafe`, no unjustified `unwrap`/`panic`), the test suite is exemplary
(3,033 passing tests with strong error-message coverage), and the public API
is ergonomic.

The most significant finding is a **single correctness bug** (§5.1) where the
list comprehension filter silently accepts non-bool values, violating the
language spec's "no truthy concept" rule. A failing test has been saved at
`/tmp/expr_probes/test_listcomp_filter.rs` and the fix is a three-line
change in `eval_listcomp`.

Two avoidable hot-path clones (§5.2, §5.3) and a public-API stability concern
around `ParsedExpression` (§5.4, §5.5) round out the primary
recommendations. Everything else is documentation or minor polish.

Appendix: probe tests from this evaluation are stored in `/tmp/expr_probes/`
(four files covering the list-comprehension filter bug and the truediv
precision check).

---

## 12. Resolution Log (2026-04-20)

Fixes landed on branch `expr-fixes`. Each fix followed strict red/green TDD
(write a failing test, watch it fail, implement the minimum change, watch it
pass, then run full CI).

### 12.1 Fix for §5.1 — list comprehension filter requires `bool`

**Files changed:**
- `crates/openjd-expr/src/eval/evaluator.rs` — `eval_listcomp`: both the
  concrete-value path and the unresolved (static-type-check) path now enforce
  `bool`, matching `eval_ifexp`.
- `crates/openjd-expr/tests/test_error_formatting.rs` — 5 new tests:
  `listcomp_filter_int_is_error`, `listcomp_filter_string_is_error`,
  `listcomp_filter_float_is_error`, `listcomp_filter_bool_happy_path`,
  `listcomp_no_filter_happy_path`. Each runtime error test asserts the
  message + source line + caret, per `AGENTS.md` error-test standard.
- `crates/openjd-expr/tests/test_unresolved_eval.rs` — 3 new tests:
  `comp_filter_unresolved_int_is_error`,
  `comp_filter_unresolved_bool_succeeds`,
  `comp_filter_unresolved_unconstrained_succeeds`.

**Before:** `[x for x in [0, 1, 2, 3] if x]` returned `[0, 1, 2, 3]` — the
filter was silently ignored for non-bool values.

**After:** Returns the error `List comprehension filter must be a boolean,
got int` with the caret pointing at the `if` clause expression. Unresolved
filters of type `unresolved[int]` also error at type-check time; only
`unresolved[bool]` and `unresolved[unresolved]` are accepted.

### 12.2 Fix for §5.4 + §5.5 — `ParsedExpression` public API

**Files changed:**
- `crates/openjd-expr/src/eval/parse.rs` — struct is now `#[must_use]`; all
  fields are `pub(crate)` (or fully private for `source`, which is only
  constructed and never read). Four accessor methods added: `expression()`,
  `accessed_symbols()`, `called_functions()`, `local_bindings()`. No
  accessors for `ast`, `source`, or `keyword_renames` — these are
  intentionally not part of the public API.
- `crates/openjd-expr/tests/test_parse_expression.rs` — 15 call sites
  updated to use the new accessor methods.
- `crates/openjd-model/src/job/create_job/instantiate.rs` — 3 call sites
  updated.

No other crates (`openjd-cli`, `openjd-sessions`, `openjd-snapshots`)
referenced the removed `pub` fields directly.

### 12.3 Fix for §5.6 — `path(list)` matches Python `PurePosixPath`/`PureWindowsPath`

**Characterized Python reference behavior** via `/tmp/path_probe.py`, covering
30+ cases including simple joins, absolute resets, empty strings, trailing
slashes, dot/dotdot segments, Windows drive letters, UNC prefixes,
drive-relative paths, and forward-slash separators on Windows.

**Files changed:**
- `crates/openjd-expr/src/functions/misc.rs` — `path_fn` list branch
  replaced with a single call to `path_parse::join_pathlib()`.
- `crates/openjd-expr/src/functions/path_parse.rs` — ~130 new lines:
  - `pub fn join_pathlib(parts, fmt)` — dispatcher
  - `fn join_pathlib_posix(parts)` — filters empty and `.` segments,
    handles absolute resets on leading `/`, preserves `..`, collapses
    duplicate separators
  - `fn win_parse_drive(s)` — parses drive letter (`C:`) or UNC prefix
    (`\\server\share`)
  - `fn join_pathlib_windows(parts)` — tracks drive/root/segments
    separately, applies pathlib rules for drive replacement, root
    replacement, and same-drive append
- `crates/openjd-expr/tests/test_paths.rs` — 36 new tests grouped by
  Posix simple / Posix absolute resets / Posix edge cases / Windows simple
  / Windows drive-root resets / Windows UNC / Windows edge cases.
- `specs/expr/function-library.md` — new documentation bullet stating
  `path(list[string])` follows pathlib semantics.

**Selected before/after:**

| Expression | Before | After |
|---|---|---|
| `path(['a', '/b'])` (Posix) | `"a//b"` | `"/b"` |
| `path([''])` | `""` | `"."` |
| `path(['a', '.', 'b'])` (Posix) | `"a/./b"` | `"a/b"` |
| `path(['C:\\a', 'D:\\b'])` (Win) | invalid | `"D:\b"` |
| `path(['C:\\a', '\\b'])` (Win) | naive join | `"C:\b"` |
| `path(['C:\\a', 'C:b'])` (Win) | naive join | `"C:\a\b"` |

URI paths (`s3://...`, `https://...`) retain the pre-existing simple-join
behavior — they are unaffected.

### 12.4 Bonus fix — 3 pre-existing conformance failures

During post-fix CI the OpenJD conformance suite reported 3 failures on
`expr2.2.3--flatten`, `expr2.2.3--sorted`, and `expr2.2.3--reversed`.
Confirmed via `git stash` that these were **pre-existing** on the base
branch — not regressions from the other fixes. Fixed as a bonus.

**Root cause:** Empty list literals `[]` are stored internally as
`ListList([], NULLTYPE, 0)` — the same enum variant as non-empty nested
lists like `[[1,2],[3,4]]`. The nesting-depth check in
`ExprValue::make_list()` used `matches!(e, Self::ListList(..))` which
rejected **any** `ListList` element, so a mixed literal like
`[[], [1], [2, 3]]` was falsely flagged as "3 levels deep" even though its
actual type is `list[list[int]]` (2 levels, allowed).

**Files changed:**
- `crates/openjd-expr/src/value.rs` — `make_list()`:
  1. Nesting check refined to
     `matches!(e, Self::ListList(_, et, _) if *et != ExprType::NULLTYPE)`
     so empty lists don't count as a nested level.
  2. After the check, empty `ListList(_, NULLTYPE)` elements are converted
     to match their typed siblings (e.g., `ListInt([])`) so the outer
     list's element type is correctly unified.
- `crates/openjd-expr/tests/test_lists.rs` — 5 new tests:
  `list_literal_empty_with_typed_sublists`,
  `list_literal_empty_in_middle`,
  `list_literal_all_empty`,
  `list_literal_empty_at_end`,
  `list_literal_empty_at_start`.

### 12.5 Fix for §5.9 — remove `is_truthy()` footgun

OpenJD's language spec §1.3.5 explicitly rejects Python-style truthy
semantics: "there is no 'truthy' concept." The `ExprValue::is_truthy()`
method in `value.rs` implemented the broader Python rules (empty string
falsy, 0 falsy, etc.) but all of its call sites already restricted the
argument to `Bool` via the signature dispatcher or preceding guards.
That made `is_truthy()` dead code for non-bool values — and a footgun for
future contributors who might reach for it.

**Files changed:**
- `crates/openjd-expr/src/value.rs` — deleted the `is_truthy()` method
  (18 lines).
- `crates/openjd-expr/src/functions/misc.rs` — `any_fn` now uses
  `if let ExprValue::Bool(true) = e`; `all_fn` uses
  `if let ExprValue::Bool(false) = e`. The signature dispatcher already
  guarantees `list[bool]` inputs.
- `crates/openjd-expr/src/eval/evaluator.rs` — `eval_ifexp` now uses
  `matches!(test, ExprValue::Bool(true))`. The preceding guard already
  validates the condition is a `Bool`.
- `crates/openjd-expr/tests/test_expr_value.rs` — removed 11 unit tests
  that exercised the deleted `is_truthy` method. (3 tests in
  `test_comparison.rs` with "truthy" in their names test `and`/`or`
  operator semantics, not the method, and were left in place.)

### 12.6 Fix for §5.7 — document `any()`/`all()` `list[bool]` restriction

**Files changed:**
- `specs/expr/function-library.md` — added a bullet under the "Function
  Semantics" section:

  > **`any()` / `all()`** accept `list[bool]` only, not arbitrary
  > iterables as in Python. The signature dispatcher enforces this.
  > There is no "truthy" concept in OpenJD (Expression Language spec
  > §1.3.5), so these functions test actual boolean values.

The canonical language spec at
`openjd-specifications/wiki/2026-02-Expression-Language.md` already
documents the signatures as `any(values: list[bool]) -> bool` and
`all(values: list[bool]) -> bool`, so no upstream change is needed.

### 12.7 CI verification (Linux, post-fix)

| Gate | Command | Result |
|---|---|---|
| fmt | `cargo +nightly fmt --all -- --check` | Clean |
| clippy (workspace) | `cargo clippy --all-features --all-targets --workspace -- -D warnings` | Zero warnings |
| clippy (helper) | `cargo clippy --manifest-path crates/openjd-sessions/src/helper/Cargo.toml -- -D warnings` | Zero warnings |
| tests (workspace) | `cargo test --workspace` | **6,092 passed**, 0 failed, 15 ignored |
| tests (helper) | `cargo test --manifest-path crates/openjd-sessions/src/helper/Cargo.toml` | 3 passed, 0 failed |
| MSRV 1.92 | `cargo +1.92 check --workspace` | Clean |
| doc | `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace` | Clean |
| conformance | `uv run run_openjd_cli_tests.py '2023-09/*'` | **1,042 passed, 0 failed** |

Compared to the pre-fix baseline: +46 tests net in the workspace suite
(new red/green tests for §5.1, §5.6, and the empty-list fix, minus the 11
unit tests removed with `is_truthy()` in §5.9), and the conformance suite
improved from 1,039 passed / 3 failed to 1,042 passed / 0 failed.

Remaining open items: §5.8 (truediv precision — documented but no code change
needed), §5.10 (symbol collection cost — intentional), §5.11 (fuzz/benchmarks —
optional enhancements).

### 12.8 Fix for §5.2 — eliminate `eval_attribute` AST clone

**Files changed:**
- `crates/openjd-expr/src/eval/evaluator.rs` — added `build_dotted_name_from_attr`
  helper that takes `&ast::ExprAttribute` directly and walks the attribute chain
  without cloning. `eval_attribute` now calls this instead of wrapping in
  `ast::Expr::Attribute(a.clone())`. The original `build_dotted_name` is retained
  (with `#[allow(dead_code)]`) for potential future callers.

**Before:** Every `Param.Frame`-style lookup deep-cloned the entire
`ExprAttribute` AST node (including the `Box<Expr>` receiver chain) just to
pass it to `build_dotted_name` which only reads by reference.

**After:** Zero clones on the hot path. Error-path clones (for `with_node`
positioning) remain but only execute on lookup failure.

### 12.9 Fix for §5.3 — eliminate `eval_compare` AST clone

**Files changed:**
- `crates/openjd-expr/src/eval/evaluator.rs` — added `dispatch_with_span` method
  that accepts a `ruff_text_size::TextRange` and uses `with_span` for error
  positioning instead of requiring an `&ast::Expr` node. `eval_compare` now calls
  `dispatch_with_span(op_name, args, c.range())` instead of cloning the full
  `ExprCompare` node per comparison operator.

**Before:** For `1 < 2 < 3 < 4`, each iteration cloned the entire `ExprCompare`
AST node solely for error caret positioning.

**After:** Uses the compare expression's `TextRange` directly — no AST clone.
Error carets still point at the full compare expression span.

### 12.10 Fix for §5.12 — rename test file

**Files changed:**
- `crates/openjd-expr/tests/test_parsing.rs` → `test_evaluation.rs` via `git mv`.

The file contains 222 tests that exercise expression **evaluation** (arithmetic,
comparisons, string ops, list ops, etc.), not parsing. The rename matches the
content. No Cargo.toml or other references needed updating.

### 12.11 Spec/docs updates

**Files changed:**
- `~/openjd-specifications/wiki/2026-02-Expression-Language.md` — §1.3.7 (List
  Comprehensions): added explicit statement that the `if` filter clause requires
  `bool`, referencing §1.3.5's "no truthy concept" rule.
- `~/openjd-rs/specs/expr/values.md` — Float64 section: added precision note
  that integer true-division loses precision above 2^53, matching Python.
- `~/openjd-rs/specs/expr/parser.md` — added note clarifying that the
  `rustpython-ruff_python_parser` crates.io package is the published version of
  the same code as `ruff_python_parser` in the `astral-sh/ruff` monorepo.
