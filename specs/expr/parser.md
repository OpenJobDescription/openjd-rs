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
Keyword rename (one lexer pass; context-sensitive keywords)
    │
    ▼
ruff_python_parser::parse_expression()
  (parse_parenthesized_expression_range() for multi-line input)
    │
    ▼
AST validation (allowlist of allowed node types)
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

The solution, before parsing:

1. Tokenize the expression once with the parser's own lexer (`ruff_python_parser::lexer`),
   in the same mode the parser will use
2. For each keyword token that is exactly a Python keyword and directly follows a `.`
   token, with nothing between them, replace it with a same-length placeholder
   identifier, which preserves every offset in the source
3. Parse the renamed text once
4. Record the renames in `keyword_renames: HashMap<String, String>` so the evaluator
   can map placeholder names back to the original attribute names

Placeholders are chosen left to right against the text as renamed so far, so a
placeholder never occurs in the source and no two keywords share one, and repeated
accesses (`A.class + B.class`) share one entry. One placeholder can still be a prefix
of another (`is` and `assert` become `bs` and `bssert`), so the evaluator maps them
back by whole dotted-path segment, never by substring. Syntax errors are reported against the
original text, which the same-length renames leave aligned.

This matches the Python implementation's `ast_parse_keyword_context()` approach, which
renames keywords before parsing and restores them after.

> **The lexer, not a text search, finds the keywords.** The lexer marks string
> literals, so a keyword inside a literal is never a keyword token and is never
> rewritten. Searching the source text for the first `.keyword` instead — the
> original implementation — rewrote literals, so `'a.class' + X.class` evaluated
> to `"a.alassc1"` instead of `"a.classc1"`, silently and with no error raised.
>
> **One pass, not one parse per keyword.** Until #489 the rename ran after a failed
> parse, at the offset the parser objected to, and re-parsed the whole expression
> once per keyword. That was quadratic: 64 KB of `a.if.if...` took 89 s, and a
> 24 KB template stalled `openjd check` for 11 s. The lexer pass is linear.
>
> The keyword is delimited by the lexer's token, so `.assert` is not read as `.as`
> followed by junk. All offsets involved are byte offsets.

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
   rest: once its recursion passes 20 levels it checks the remaining stack,
   and when less than 100 KB is left it continues on a new 1 MB segment
   allocated on the heap. The 32 MB thread is still needed for this
   crate's own AST walkers. Together they let the parser complete on *any*
   input up to
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

Expressions can span multiple lines via implicit line continuation, as if they were
inside parentheses. An expression containing a newline is parsed with
`ruff_python_parser::parse_parenthesized_expression_range`, the parser's parenthesized
mode, which allows the line breaks without adding a `(` to the text. Every offset the
parser reports, in AST ranges and errors alike, is therefore a byte offset into the
expression as written, and error messages show only the relevant line with its
columns (see [error-formatting.md](error-formatting.md)).

A multi-line expression may end in a `#` comment, as in parentheses in Python.

Until #489 the expression was wrapped in `(...)` before parsing instead. That shifted
every offset by one byte, which three consumers each undid separately and one didn't
(the caret for a multi-line binary operator landed one byte right of the operator),
and syntax errors could mention a `)` the author never wrote (`Param.A\nParam.B`
gave ``Expected `)`, found identifier``).

## Divergence from Python

The Python implementation uses `ast.parse(expr, mode='eval')` from the standard library.
The Rust implementation uses `ruff_python_parser::parse_expression()` which produces
`ruff_python_ast` types instead of Python `ast` types. The AST structures are nearly
identical, so the evaluator logic translates directly.

Syntax errors are worded differently: the Python implementation reports CPython's
`SyntaxError` messages, and this crate reports Ruff's (see "Error messages" above). The
error location and caret format are the same.
