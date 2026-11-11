# Parser

## Overview

The expression language reuses a subset of Python's expression syntax (RFC 0005 §Design Constraints,
item 7). Rather than writing a custom parser, the crate uses `ruff_python_parser` to
parse expressions into a Python AST, then walks the AST with a custom evaluator.

Defined in `eval/parse.rs`.

## Parser Selection: ruff_python_parser

The `ruff_python_parser`, `ruff_python_ast` and `ruff_text_size` crates that
Astral publishes to crates.io from the [astral-sh/ruff](https://github.com/astral-sh/ruff)
monorepo are used directly. Astral describes them as internal components of
Ruff, publishes them about weekly as `0.0.x`, and makes no stability promise.

**Why ruff over rustpython-parser:**

- rustpython-parser's README states it is "superseded by" the ruff parser
- ruff is actively maintained (daily commits, backed by Astral)
- rustpython-parser has had no substantive code changes since early 2025

**Why not the `rustpython-ruff_*` crates.** Until #489 the crate used
`rustpython-ruff_python_parser`, which the RustPython project publishes from its
own fork of ruff. Its 0.16.10 is not the same code as Astral's: the fork adds
CPython-style syntax-error hints ("invalid syntax. Perhaps you forgot a
comma?") that parse ahead speculatively, rewind, and parse the same tokens
again. The hints nest inside brackets, so an invalid expression's parse time
doubled with each unclosed bracket level (`"f(a " * 24 + "x"`, 97 bytes, took
12 s), and a run of juxtaposed tokens inside one bracket was cubic in its
length. Expressions are untrusted template input, so that was a denial of
service. Astral's parser has no such hints and parses every one of those inputs
in milliseconds. `tests/integration/test_parser_work_bound.rs` holds the
reproducers with a time bound, so a parser bump that brings the behavior back
fails CI.

```toml
ruff_python_parser = { version = "=0.0.21", features = ["stacker"] }
ruff_python_ast = "=0.0.21"
ruff_text_size = "=0.0.21"
```

**Exact version pin.** The ruff crates are pinned with `=` because they make
breaking AST changes without notice. For example, rustpython-ruff 0.16.10
replaced `ExprCompare`'s `left` and `comparators` fields with a single
`operands` list in a patch release. With the earlier `^0.16.5` requirement,
anything that resolved dependencies without our `Cargo.lock` failed to compile
`openjd-expr`. That included downstream users and the cargo-semver-checks step
in the release-plz workflow. Upgrade all three crates together, review the
upstream AST changes, and keep `test_parser_work_bound.rs` passing as part of
each bump.

**`stacker` feature.** With it, the parser grows its stack on the heap once its
recursion gets deep (see Depth Limit below). Without it, 64 KB of `-` or of
nested brackets overflows even the 32 MB worker thread and aborts the process.

**Error messages.** Syntax errors carry Ruff's own wording, prefixed with
`Syntax error: `, for example `Expected an expression` for `1 +` and
`unexpected EOF while parsing` for `(1 + 2`. These are not CPython's messages.

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
2. If parsing fails, look at the offset the parser reported the error at. The parser
   rejects a contextual keyword with "Expected an identifier, but found a keyword",
   and its error range begins exactly at the keyword token
3. If the token at that offset is directly preceded by `.` and is exactly a Python
   keyword, replace it with a same-length placeholder identifier, which preserves
   every other offset in the source
4. Re-parse with the placeholder, repeating from step 1 — one keyword is renamed per
   parse attempt, so an expression with several of them converges in several passes
5. Record the renames in `keyword_renames: HashMap<String, String>` so the evaluator
   can map placeholder names back to the original attribute names

This matches the Python implementation's `ast_parse_keyword_context()` approach, which
renames keywords before parsing and restores them after.

> **The rename is located, not searched for.** Only the one position the parser
> objected to is considered. This is what keeps the fixup out of string literals:
> a keyword inside a literal is valid Python, so it is never the parser's error
> location. Searching the source text for the first `.keyword` instead — the
> original implementation — rewrote literals, so `'a.class' + X.class` evaluated
> to `"a.alassc1"` instead of `"a.classc1"`, silently and with no error raised.
>
> The keyword token is delimited by scanning to the first non-identifier
> character rather than by assuming a keyword's length, so `.assert` is not read
> as `.as` followed by junk. All offsets involved are byte offsets; mixing a byte
> offset into a `chars()` index was a latent defect in the original boundary
> check on sources containing multi-byte characters.

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

## Depth Limit

The parser and evaluator both enforce a maximum AST nesting depth of
[`MAX_EXPRESSION_DEPTH`](../../crates/openjd-expr/src/eval/parse.rs) (currently
**64**). This exists solely to prevent stack exhaustion on pathological inputs —
not to constrain legitimate expressions, which rarely nest more than ~10 levels.

Three independent mechanisms work together:

1. **Absolute input-size cap.** `ParsedExpression::new` rejects any source
   longer than [`MAX_PARSE_INPUT_LEN`](../../crates/openjd-expr/src/eval/parse.rs)
   (currently **64 KB**) before invoking the parser. This bounds the parser's
   worst-case recursion depth at source length: a recursive-descent parser
   cannot recurse more times than there are input characters to consume.

2. **Short-input fast path + long-input worker thread.** Inputs of at most
   **200** characters (`FAST_PATH_INPUT_LEN`) parse directly on the current
   thread. Two things together make this safe: a recursive-descent parser
   cannot recurse more times than there are input characters to consume,
   and the ruff parser's empirical overflow threshold on Rust's default
   2 MB thread stack is well above 200 frames (≥ 500 observed in release
   builds). Longer inputs run on a dedicated thread with a 32 MB stack via
   `std::thread::Builder::stack_size`. That stack is not enough on its own:
   the parser recurses once per nested bracket or prefix operator, and
   64 KB of them overflows 32 MB. The parser's `stacker` feature covers the
   rest, growing the stack on the heap once recursion passes about 20
   levels, so the parser completes on *any* input up to
   `MAX_PARSE_INPUT_LEN` without our enumerating which token patterns drive
   its recursion. The parser has no bracket-nesting limit of its own, so
   nesting is bounded only by `MAX_PARSE_INPUT_LEN`.

   The fast-path threshold is deliberately decoupled from
   `MAX_EXPRESSION_DEPTH`: the depth cap is a semantic abuse ceiling on
   AST nesting, while the fast-path threshold is a pragmatic tuning knob
   for parser stack-safety on the caller's thread. Real-world template
   expressions are nearly always under 100 characters, so 200 keeps the
   vast majority of inputs on the cheap path.

3. **Structural AST walker.** After parsing succeeds, `validate_structure`
   carries a `depth` counter and bumps it on every recursive descent into
   a child node. If the counter exceeds `MAX_EXPRESSION_DEPTH`, validation
   fails with `ExpressionErrorKind::ExpressionTooDeep`. Chained comparisons
   (`a < b < c < ...`) are also checked against the limit via the number of
   comparison operators (`ExprCompare::ops.len()`, one less than the number
   of operands).

The evaluator applies a parallel check: `Evaluator::evaluate` — the single
entry point through which every sub-node evaluation flows — carries a
`recursion_depth` field and checks it on entry. ASTs that slip past the
parser-phase check (e.g., a long left-associative binop chain `1+1+...+1`
which produces a deep-but-syntactically-shallow source) are caught here
before they can exhaust the evaluator's stack.

Rust threads default to 2 MB of stack, and Rust cannot recover from stack
overflow via `std::panic::catch_unwind` — it aborts the process. The input
cap + worker thread + the parser's `stacker` feature + depth walker +
evaluator guard together ensure that
no input to `ParsedExpression::new` or `Evaluator::evaluate` can cause a
process abort.

Exceeding the limit returns `ExpressionErrorKind::ExpressionTooDeep { depth,
limit: 64 }` with normal caret formatting.

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
