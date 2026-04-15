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
    .with_path_mapping_rules(&rules)
    .evaluate()?;

println!("Value: {:?}", result);
println!("Peak memory: {} bytes", parsed.peak_memory_usage);
println!("Operations: {}", parsed.operation_count);
```

Builder methods on `Evaluator`:

| Method | Default | Purpose | Notes |
|--------|---------|---------|-------|
| `with_library(&FunctionLibrary)` | `get_default_library()` | Custom function library | Add host-context functions or restrict operations |
| `with_memory_limit(usize)` | 100,000,000 (100 MB) | Memory bound | Tracks all intermediate values; returns `MemoryLimitExceeded` |
| `with_operation_limit(usize)` | 10,000,000 (10M) | Operation bound | Each call=1, list iteration=N, string ops=ceil(len/256) |
| `with_path_format(PathFormat)` | Host native | Path normalization format | Also validates that `Path` values in symbol table match |
| `with_target_type(&ExprType)` | None | Context-dependent coercion | Influences empty list element type and mixed-type coercion |
| `with_path_mapping_rules(&[PathMappingRule])` | Empty | Path mapping for `apply_path_mapping` | Host-context only; first matching rule wins |
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

### BoolOp (`eval_boolop`)
`and` and `or` are value-returning with null-coalescing semantics:

- `a and b` → returns `a` if falsy, else `b`
- `a or b` → returns `a` if truthy, else `b`

The definition of "falsy" here differs from Python. In Python, `0`, `""`, `[]`, and
`None` are all falsy. In the OpenJD expression language, only `null` and `false` are
falsy — `0`, `""`, and `[]` are truthy. This narrower definition was chosen because
treating empty strings and zero as falsy is a common source of bugs in template
expressions, where `Param.Name or "default"` should only fall through to the default
when the parameter is genuinely absent (null), not when it happens to be an empty string.

This enables null-coalescing patterns like `Param.X or "default"`. The exception is
`bool`, where `false` is falsy as expected — `Param.Flag or true` evaluates to `true`
when the flag is false.

When either operand is unresolved, both branches are evaluated to propagate type
information, and the result is `Unresolved(union[type_a, type_b])`.

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
e.g., in a list comprehension like `[x for x in items if re_match(x, "^shot_\\d+$")]`.

The cache is per-evaluation, not global — it is created fresh for each `evaluate()` call
and discarded afterward.

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
