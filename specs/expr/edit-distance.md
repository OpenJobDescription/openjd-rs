# Edit Distance ("Did you mean?" Suggestions)

## Overview

`edit_distance.rs` computes Levenshtein distance between two strings and uses it to
pick the closest match from a set of candidates. The result feeds the error messages
produced by `ExpressionError` when an unknown variable or function name is referenced.

## Public API

```rust
pub fn edit_distance(s1: &str, s2: &str) -> usize;
pub fn suggest_closest(name: &str, available: &[&str]) -> String;
```

## edit_distance

Classic two-row dynamic-programming Levenshtein implementation. Operates on
`char` slices so multi-byte UTF-8 is counted correctly (one grapheme of most scripts
is one edit).

- Time: `O(m × n)` where `m`, `n` are the char lengths.
- Space: `O(min(m, n))` — two rows of length `n + 1`.

The implementation is deliberately character-based (not byte-based) so that typos in
identifiers containing non-ASCII characters (e.g., `Param.名前` mistyped as
`Param.名前_x`) produce meaningful distances.

## suggest_closest

Returns a formatted suggestion suffix that error callers append to their message:

| Outcome | Return value |
|---|---|
| No candidate within the threshold | `""` (empty — error emits no suggestion) |
| Single best match | `" Did you mean: {name}"` (note leading space) |
| Multiple tied matches | `" Did you mean one of: {a}, {b}, ..."` alphabetized |

### Threshold

```rust
const MAX_SUGGESTION_DISTANCE: usize = 5;
```

Names with distance ≥ 5 are never suggested. The threshold is a compromise: small
enough that unrelated names (`x` vs `ReallyDifferentName`) don't produce noisy
suggestions, large enough to cover typical typos in identifiers up to ~20 characters
(distance 5 covers a misplaced prefix plus a couple of edits).

## Call Sites

Two places in the crate invoke `suggest_closest`:

1. **Unknown variable** (`Evaluator::eval_name`, `eval_attribute`) — candidates are
   the union of every dotted symbol path reachable in the active symbol tables
   (collected via `SymbolTable::all_paths`).
2. **Unknown function** (`FunctionLibrary::call`) — candidates are every function
   name registered in the library (the `functions` `HashMap` keys).

In both cases the suggestion is appended to the structured error message and rendered
in the caret-annotated error output.

## Future Work

`suggest_closest` currently runs a full `edit_distance` against every candidate. A
length-difference early rejection (skip any candidate where `|len(name) − len(cand)|
> MAX_SUGGESTION_DISTANCE`) would cut the common case in half without changing
behavior. Not yet implemented; see the quality evaluation report's P4
recommendations.
