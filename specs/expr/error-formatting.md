# Error Formatting

## Overview

Expression errors display the source expression with a caret pointer indicating exactly
where the error occurred. The caret position is chosen based on the AST node type to
point at the most relevant part of the expression.

Defined in `error.rs`.

## Error Display Format

```
Cannot convert 'bad' to int
  1 + int('bad') + 2
      ^~~~~~~~~~
```

Three lines:
1. Error message
2. Expression with 2-space indent
3. Caret line: `~` underlines the error span, `^` marks the most relevant position

## ExpressionError

```rust
pub struct ExpressionError {
    kind: ExpressionErrorKind,
    expr: Option<String>,             // source expression text
    col_offset: Option<usize>,        // start of error span
    end_col_offset: Option<usize>,    // end of error span
    caret_offset: Option<usize>,      // position of ^ within span (relative to col_offset)
}
```

Fields are private; callers access them via `kind()`, `expr()`, `col_offset()`,
`end_col_offset()`, and `caret_offset()` accessor methods. This keeps the error
representation free to evolve (e.g., caching precomputed line info) without a
source-breaking change.

### with_node — attach AST context

```rust
let err = ExpressionError::new("Type mismatch")
    .with_node(expr_source, ast_node);
```

Extracts the span from the AST node's range and computes the smart caret position
based on node type.

### with_span — manual span

```rust
let err = ExpressionError::new("Unknown variable")
    .with_span(expr_source, col, end_col);
```

For errors where the AST node isn't available (e.g., symbol table lookup failures).

### message_with_expr_prefix — adjust for format string context

```rust
err.message_with_expr_prefix("{{")
```

When an expression error occurs inside a format string `{{expr}}`, the column offsets
need adjustment to account for the `{{` prefix. This method shifts the caret position
accordingly.

## ExpressionErrorKind

`ExpressionError` carries a structured `kind` field for programmatic error handling.
Variants carry the data they need for the `Display` impl (via `thiserror`), so each
error kind is self-describing and its message format is enforced at the type level:

```rust
#[non_exhaustive]
pub enum ExpressionErrorKind {
    UndefinedVariable { name: String, suggestion: String },
    UnknownFunction   { name: String },
    TypeError         { message: String },
    IntegerOverflow,
    DivisionByZero    { op: &'static str },  // "Division" or "Modulo"
    FloatError        { message: String },
    IndexOutOfBounds  { message: String },
    MemoryLimitExceeded { used: usize, limit: usize },
    OperationLimitExceeded,
    UnsupportedSyntax { feature: String },
    ExplicitFail(String),
    ParseError(String),
    Other(String),
}
```

| Variant | Trigger |
|---|---|
| `UndefinedVariable` | Symbol table lookup finds no matching variable |
| `UnknownFunction` | No function with the given name in the library |
| `TypeError` | Type mismatch in operation or function call |
| `IntegerOverflow` | i64 arithmetic exceeds bounds |
| `DivisionByZero` | Division or modulo by zero |
| `FloatError` | Float operation produces NaN or infinity |
| `IndexOutOfBounds` | List or range index outside valid range |
| `MemoryLimitExceeded` | Cumulative value memory exceeds configured limit |
| `OperationLimitExceeded` | Operation count exceeds configured limit |
| `UnsupportedSyntax` | Valid Python syntax not supported by the expression language |
| `ExplicitFail` | The `fail()` function was called |
| `ParseError` | Expression could not be parsed |
| `Other` | Catch-all for errors that don't fit other categories |

The enum is `#[non_exhaustive]` to allow adding new variants without breaking callers.
Callers should always include a wildcard arm when matching.

Convenience constructors on `ExpressionError` set the kind automatically:

```rust
ExpressionError::undefined_variable("Param.Missing")
ExpressionError::type_error("expected int, got string")
ExpressionError::integer_overflow()           // no arg — message is fixed
ExpressionError::division_by_zero("Division") // op label: "Division" or "Modulo"
```

## Smart Caret Positioning

The `^` position depends on the AST node type, pointing at the most informative part
of the expression rather than always at the start. The underlying principle is:
**point at the operator or name that failed**, not at the beginning of the whole
expression containing it — so `1 + "x"` underlines the full sub-expression but the
`^` lands on the `+`, the operation that actually couldn't be performed.

| Node Type | Caret Position | Example |
|-----------|---------------|---------|
| BinOp | At the operator | `1 + "x"` → `~~^~~~~` |
| Attribute | At the attribute name (after `.`) | `x.bad` → `~~^~~` |
| Call (method) | At the method name | `x.bad()` → `~~^~~~~` |
| Subscript | At the `[` | `x[99]` → `~^~~~` |
| Default | At the start of the span | `bad_var` → `^~~~~~~` |

### BinOp caret computation

For binary operations, the operator position is found by scanning backwards from the
right operand's start position, skipping whitespace. Two-character operators (`**`, `//`,
`>=`, `<=`, `!=`, `==`) are detected by checking the preceding character.

## Evaluator Integration

Errors originate in two places:

### Direct evaluator errors

Methods like `eval_compare`, `eval_boolop`, `eval_ifexp`, `eval_subscript` create errors
with AST node context directly:

```rust
Err(ExpressionError::new("Condition must be bool")
    .with_node(self.expr_source(), node))
```

### Library-dispatched errors

Function implementations return errors without source context (they don't have access
to the AST). The evaluator's `dispatch` method wraps them:

```rust
match library.call(name, &args, self) {
    Ok(v) => Ok(v),
    Err(e) if e.expr.is_none() => Err(e.with_node(source, call_node)),
    Err(e) => Err(e),
}
```

The `call_node` is the AST node that triggered the dispatch (e.g., `ExprBinOp` for
operators, `ExprCall` for function calls).

## Multi-line Expressions

For multi-line expressions (using implicit line continuation), only the relevant line
is shown. The `lineno` from the AST node selects the line, and `col_offset` is relative
to that line.

## "Did You Mean?" Suggestions

When a function name is not found in the library, the error includes a suggestion based
on edit distance (Levenshtein distance, implemented in `edit_distance.rs` — see
[edit-distance.md](edit-distance.md)):

```
Unknown function 'lne'
  lne(my_list)
  ^~~
Did you mean 'len'?
```

Similarly for unknown variable names in the symbol table.
