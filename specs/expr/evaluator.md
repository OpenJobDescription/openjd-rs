# Evaluator

## Overview

The evaluator is an AST-walking interpreter that evaluates parsed EXPR expressions
against a symbol table, using the function library for dispatch. It tracks memory and
operation counts to enforce resource bounds.

Defined in `eval/evaluator.rs`. The central `evaluate_inner` method is a dispatch
table mapping each AST node type to a dedicated `eval_*` method. Child evaluators
(used by list comprehensions) are created via `child_evaluator()` with resource
counters propagated back via `absorb_counters()`.

## Builder Pattern

Evaluation is configured through a builder on `ParsedExpression`:

```rust
let parsed = ParsedExpression::new("Param.Frame * 2 + 1")?;

// Simple — defaults for everything
let value = parsed.evaluate(&symtab)?;

// Configured — custom limits, path format, library
let result = parsed.evaluator(&[&symtab])
    .with_library(&custom_lib)
    .with_memory_limit(50_000_000)
    .with_operation_limit(1_000_000)
    .with_path_format(PathFormat::Posix)
    .evaluate(&parsed.ast)?;

println!("Value: {:?}", result);
println!("Peak memory: {} bytes", parsed.peak_memory_usage);
println!("Operations: {}", parsed.operation_count);
```

Note: `Evaluator::evaluate` takes the AST node as an argument (not captured by the
builder). The AST is owned by `ParsedExpression`, so callers pass `&parsed.ast`.
`ParsedExpression::evaluate(&symtab)` is the convenience shortcut that constructs,
runs, and tears down the evaluator in one call.

Path mapping rules are **not** an evaluator configuration. They are state owned
by the `apply_path_mapping` implementation, which is a closure registered on the
function library via [`FunctionLibrary::with_host_context(rules)`](function-library.md#host-context).
The evaluator just calls functions; it has no direct awareness of mapping rules.

Builder methods on `Evaluator`:

| Method | Default | Purpose | Notes |
|--------|---------|---------|-------|
| `with_library(&FunctionLibrary)` | `get_default_library()` | Custom function library | Add host-context functions or restrict operations |
| `with_memory_limit(usize)` | 100,000,000 (100 MB) | Memory bound | Tracks all intermediate values; returns `MemoryLimitExceeded` |
| `with_operation_limit(usize)` | 10,000,000 (10M) | Operation bound | Each call=1, list iteration=N, string ops=ceil(len/256) |
| `with_path_format(PathFormat)` | Host native | Path normalization format | Also validates that `Path` values in symbol table match |
| `with_target_type(&ExprType)` | None | Context-dependent coercion | Influences empty list element type and mixed-type coercion |
| `with_expr_source(&str)` | From ParsedExpression | Source text for error messages | Internal — set automatically by `ParsedExpression::evaluator()` |
| `with_keyword_renames(&HashMap)` | From ParsedExpression | Keyword rename map | Internal — set automatically by `ParsedExpression::evaluator()` |

The last two methods (`with_expr_source`, `with_keyword_renames`) are internal plumbing
that `ParsedExpression::evaluator()` configures automatically. Users building an
`Evaluator` directly via `Evaluator::new()` would need to set these manually to get
correct error messages and keyword-as-attribute support (e.g., `Param.if`).

## Resource Bounding

### Memory Tracking

Every `ExprValue` created during evaluation is tracked:

```
create value → _track(value)     → current_memory += value.memory_size()
                                   peak_memory = max(peak_memory, current_memory)
                                   if current_memory > limit → error

consume value → _release(value)  → current_memory -= value.memory_size()
```

The `dispatch` method centralizes this: it releases input values, calls the function
implementation, and tracks the output value. This keeps function implementations pure
(values in, value out) with memory tracking handled by the evaluator.

### Operation Counting

Every function call increments the operation counter:

| Operation | Cost |
|-----------|------|
| Function/operator call | 1 |
| List iteration (comprehension, sorted, etc.) | +N (element count) |
| String processing | +ceil(len / 256) |

The proportional costs prevent DoS via large strings or lists — a million-element list
comprehension costs 1M operations even if each iteration is trivial.

## AST Node Evaluation

The evaluator dispatches on AST node type:

### Constant (`eval_constant`)
Converts Python literals to `ExprValue`. Preserves the original string representation
for float literals (e.g., `3.14` stores both the f64 and `"3.14"`) for lossless
round-tripping.

### Name (`eval_name`)
Looks up a simple name in the symbol tables (searched in order). Also handles JSON
literal normalization (`null`, `true`, `false`) and comprehension loop variables.

### Attribute (`eval_attribute`)
Handles dotted access like `Param.Frame` or `path.name`. Resolution order:

1. Try the full dotted path as a variable lookup (e.g., `Param.Frame` in symbol table)
2. Try progressively shorter prefixes as variable lookups, with the remainder as
   property access (e.g., look up `Param`, then access `.Frame` as a property)
3. If the base is a value, dispatch to `__property_NAME__` in the function library

This dual-purpose resolution is why `Param.Frame` works as a variable lookup and
`my_path.name` works as a property access, using the same syntax.

### BinOp (`eval_binop`)
Maps Python operators to dunder function names and dispatches through the library:

```
a + b  → __add__(a, b)
a - b  → __sub__(a, b)
a * b  → __mul__(a, b)
a / b  → __truediv__(a, b)
a // b → __floordiv__(a, b)
a % b  → __mod__(a, b)
a ** b → __pow__(a, b)
```

Both operands are evaluated, then `dispatch` handles memory release/track.

### UnaryOp (`eval_unaryop`)
Maps to `__neg__`, `__pos__`, `__not__`.

Special case: `-<int literal>` is folded into a single negative integer to handle
`INT64_MIN` (-2^63), which cannot be represented as a negated positive literal.

### Compare (`eval_compare`)
Supports chained comparisons (`1 < x < 10`). Maps operators:

```
==  → __eq__       !=  → __ne__
<   → __lt__       <=  → __le__
>   → __gt__       >=  → __ge__
in  → __contains__(container, item)    # note: args swapped
not in → __not_contains__(container, item)
```

Chained comparisons short-circuit: `a < b < c` evaluates `a < b`, and only if true,
evaluates `b < c`. The intermediate value `b` is reused.

Comparison stays in the evaluator (not fully delegated to the library) because the
chaining logic requires control flow that doesn't fit the simple dispatch model.

#### Chained Comparison Memory Management

Each comparison in the chain clones `left` and `right` to pass to `dispatch` (which
releases its inputs), while retaining the originals for the chain. After dispatch
returns, the boolean result is extracted and the dispatch return value is released.
If the comparison is false, both `left` and `right` are released and `false` is returned
immediately (short-circuit). If true, `left` is released and `right` becomes the new
`left` for the next iteration — avoiding a redundant re-evaluation of the shared
operand. After the final comparison, the surviving `left` is released and `true` is
returned.

```
a < b < c
  │
  ├── left = eval(a), right = eval(b)
  ├── dispatch("__lt__", [left.clone(), right.clone()])
  ├── result true → release(left), left = right
  │
  ├── right = eval(c)
  ├── dispatch("__lt__", [left.clone(), right.clone()])
  ├── result true → release(left), release(right)
  └── return true
```

The clone-and-release pattern is necessary because `dispatch` consumes (releases) its
arguments for memory tracking, but the chained comparison needs to retain the shared
middle operand for the next pair.

### BoolOp (`eval_boolop`)
`and` and `or` are value-returning with null-coalescing semantics:

- `a and b` → returns `a` if falsy, else `b`
- `a or b` → returns `a` if truthy, else `b`

**Falsy definition:** In the OpenJD expression language, only `null` and `false` are
falsy. `0`, `""`, `[]`, and all other values are truthy. This narrower definition
differs from Python (where `0`, `""`, `[]`, and `None` are all falsy) and was chosen
because treating empty strings and zero as falsy is a common source of bugs in
template expressions — `Param.Name or "default"` should only fall through to the
default when the parameter is genuinely absent (null), not when it happens to be
an empty string.

This enables null-coalescing patterns like `Param.X or "default"` while still
letting `Param.Flag or true` yield `true` when the flag is `false`.

When an earlier operand is unresolved, subsequent operands are still evaluated (to
catch type errors in them), but the final result is `Unresolved(BOOL)` unless a
subsequent concrete operand proves the result by short-circuiting (e.g.,
`Unresolved and false` returns `false`). Errors in operands past the unresolved one
are suppressed, since a runtime short-circuit could make them unreachable.

### IfExp (`eval_ifexp`)
Ternary: `x if condition else y`. Evaluates the condition first, then only the
selected branch. When the condition is unresolved, both branches are evaluated and
the result type is the union.

### Call (`eval_call`)
Handles both function calls (`len(x)`) and method calls (`x.upper()`).

Method calls are transformed to function calls via UFCS (Uniform Function Call Syntax):
`obj.method(args)` → `method(obj, args)`.

Rejects:
- Direct dunder calls (`__add__(1, 2)` — use `1 + 2` instead)
- Calling properties as methods (`path.name()` — use `path.name` instead)

### List (`eval_list`)
Evaluates list literals. Validates max 2 nesting levels. Coerces elements when mixed
types are present (int→float, path→string). Empty lists use the target type context
to determine element type.

### ListComp (`eval_listcomp`)
Evaluates list comprehensions: `[expr for var in iterable if condition]`.

- Single generator only (no nested `for` clauses)
- Creates a local scope per iteration (loop variable doesn't leak)
- Handles unresolved iterables by evaluating the body once with an unresolved loop variable
- Operation count: +1 per iteration

### Subscript (`eval_subscript`)
Handles indexing (`x[0]`) and slicing (`x[1:3]`, `x[::2]`).

- Negative indices wrap around (Python semantics)
- Slices return the same type as the input (string→string, list→list)
- Out-of-bounds index raises an error; out-of-bounds slice clamps silently

## Dispatch Flow

The `dispatch` method is the centralized point for calling library functions:

```
dispatch(name, args, ast_node)
    │
    ├── Release input values from memory tracking
    │
    ├── Check if any arg is unresolved
    │   └── Yes → infer return type from signature, return Unresolved(return_type)
    │
    ├── Call library.call(name, args, eval_context)
    │   │
    │   ├── Phase 1: Exact non-generic match
    │   ├── Phase 2: Non-generic with coercion (skip receiver coercion for methods)
    │   └── Phase 3: Generic match with type variable binding
    │
    ├── Track output value in memory
    │
    └── On error: attach AST node context for caret formatting
```

## Fast Path: Simple Name Lookup

`ParsedExpression::as_name_lookup()` detects expressions that are just a dotted name
(e.g., `Param.Frame`) and returns the name string directly. This bypasses full evaluation
for simple variable substitution, which is the common case for base-spec format strings.

Note that `as_name_lookup` returns the raw dotted name without interpreting it — for
`Param.OutputDir.name` it returns `"Param.OutputDir.name"`, and the caller must determine
that `"Param.OutputDir"` is the symbol table entry and `"name"` is a property access on
it. Most callers will want to use regular evaluation instead; this fast path is primarily
for letting implementations validate templates correctly without EXPR enabled, where
expressions must be simple dotted name lookups.

## Regex Cache

The evaluator maintains a `HashMap<String, regex::Regex>` that caches compiled regex
patterns across function calls within a single evaluation. When a regex function
(`re_match`, `re_search`, `re_sub`) is called, the evaluator checks the cache
before compiling. This avoids recompiling the same pattern when used multiple times,
e.g., in a list comprehension like `[x for x in items if re_search(x, "shot") != null]`.

The cache is per-evaluation, not global — it is created fresh for each `evaluate()` call
and discarded afterward. Child evaluators (created for list comprehensions) share the
parent's cache via move-and-return: the parent's cache is moved into the child before
each iteration and moved back after, so patterns compiled in one iteration are available
in subsequent iterations without recompilation.

## Divergence from Python

The Rust evaluator uses the same algorithmic structure as the Python `Evaluator` class.
Key differences:

1. **Builder pattern** — Python uses constructor arguments; Rust uses a builder for
   clearer configuration.
2. **Centralized dispatch** — The Rust `dispatch` method handles memory release/track
   around every function call. Python does this inline in each eval method.
3. **EvalContext trait** — Function implementations access evaluator state through the
   `EvalContext` trait rather than receiving the evaluator directly. This prevents
   function implementations from calling evaluation methods, enforcing the separation
   between evaluation control flow and function logic.
4. **Typed list variants** — The evaluator constructs typed list variants directly,
   avoiding the overhead of wrapping each element in a tagged `ExprValue`.
