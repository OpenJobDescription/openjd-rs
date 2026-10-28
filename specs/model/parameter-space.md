# Parameter Space Iteration

The `step_param_space` module provides `StepParameterSpaceIterator` for lazily iterating over
the multidimensional space of task parameter values defined by a step's parameter space.

## Public API

```rust
impl StepParameterSpaceIterator {
    pub fn new(space: &StepParameterSpace) -> Result<Self, ModelError>
    pub fn new_with_chunk_override(space: &StepParameterSpace, override_count: Option<usize>) -> Result<Self, ModelError>
    pub fn names(&self) -> &HashSet<String>
    pub fn len(&self) -> usize
    pub fn is_empty(&self) -> bool
    pub fn get(&self, index: usize) -> Option<TaskParameterSet>
    pub fn contains(&self, params: &TaskParameterSet) -> bool
    pub fn validate_containment(&self, params: &TaskParameterSet) -> Result<(), String>
    pub fn chunks_adaptive(&self) -> bool
    pub fn chunks_parameter_name(&self) -> Option<&str>
    pub fn chunks_default_task_count(&self) -> Option<usize>
    pub fn set_chunks_default_task_count(&mut self, value: usize)
    pub fn reset(&mut self)
}

impl Iterator for StepParameterSpaceIterator {
    type Item = TaskParameterSet;
}
```

`new` returns `Result<Self, ModelError>` because construction can fail (e.g., if the
product of parameter dimensions overflows `usize`). `get` returns `Option<TaskParameterSet>`
(returns `None` for out-of-bounds indices, or for an adaptive space, where chunk
boundaries are not a function of the index alone).
`set_chunks_default_task_count` takes `&mut self` (no `Arc<AtomicUsize>` indirection).

`chunks_parameter_name` and `chunks_default_task_count` report for *any* chunked space,
adaptive or not: both values are in the template, and a chunk override replaces the size.
Only `set_chunks_default_task_count` is adaptive-specific, because only an adaptive size
is mutable mid-walk.

`new_with_chunk_override` accepts an optional chunk size that overrides the template's
`defaultTaskCount` for all chunk nodes. When `Some(n)`, it also suppresses adaptive
chunking (the parameter is treated as static with chunk size `n`).

`reset` rewinds the iterator so a fresh `Iterator::next` walk yields the same elements
again, without rebuilding from the parameter space. For non-sequential (random-access)
iterators it sets the internal `current_index` to 0; for sequential iterators (adaptive
chunking) it delegates to the inner node iterator's reset.
The adaptive `Arc<AtomicUsize>` chunk-size override set via
`set_chunks_default_task_count` is preserved across `reset` calls — `reset` does not
restore the template's original `defaultTaskCount`.

## Node Tree Architecture

The iterator is built on a tree of `Node` trait objects. Each node represents a dimension
or operation in the parameter space:

```
ProductNode (cartesian product)
├── RangeListNode ("color": ["red", "green", "blue"])
├── AssociationNode (lockstep iteration)
│   ├── RangeExprNode ("frame": "1-3")
│   └── RangeListNode ("camera": ["main", "side", "top"])
├── ContiguousChunkNode (gap-respecting contiguous chunks)
│   └── range: "1,10-12,18-50"
└── StaticChunkNode (noncontiguous chunks)
    └── range: "1-256"
```

### Node Types

| Node | Purpose | Length | Random Access |
|------|---------|--------|---------------|
| `ZeroDimSpaceNode` | Empty parameter space | 1 | O(1) |
| `RangeListNode` | Pre-materialized value list | List length | O(1) |
| `RangeExprNode` | Integer range expression | Computed from range | O(1) via arithmetic |
| `ProductNode` | Cartesian product (`*` operator) | Product of children (checked) | O(D) via divmod |
| `AssociationNode` | Lockstep (`,` in parens) | Same as children | O(1) |
| `ContiguousChunkNode` | Gap-respecting contiguous chunks | Cached exact count | O(log G) in interval groups |
| `StaticChunkNode` | Noncontiguous chunk boundaries | Number of chunks | O(log R + runs the chunk spans) |
| `AdaptiveChunkNode` | Runtime-adjustable chunks | Dynamic | None (sequential only) |

### ProductNode Index Arithmetic

Cartesian product uses row-major order (rightmost dimension changes fastest). For a product
of dimensions with sizes `[s0, s1, s2]`, element at flat index `i` maps to:

```
d2 = i % s2
d1 = (i / s2) % s1
d0 = (i / (s2 * s1)) % s0
```

This gives O(D) random access (where D is the number of dimensions) without materializing
the full product. The total length is computed using `checked_mul` to detect overflow.

### AssociationNode

Lockstep iteration: all children advance together. All children must have the same length
(validated during construction — produces a `ModelError::DecodeValidation` if lengths
differ). Element at index `i` is the union of child[j].get(i) for all children j.

## Combination Expression Parsing

The `combination` field in `StepParameterSpaceDefinition` controls how task parameters
are combined:

- `*` — Cartesian product (default if no expression or just `*`)
- `(A, B)` — Association (lockstep iteration)
- Nesting: `A * (B, C)` — Product of A with the association of B and C

**Parsing:**
1. `tokenize()` splits into `Name`, `Star`, `LParen`, `RParen`, `Comma` tokens
2. Recursive descent parser builds the node tree:
   - `*` creates `ProductNode`
   - `(A, B)` creates `AssociationNode`
   - Bare names create leaf nodes (`RangeListNode` or `RangeExprNode`)
3. Default (no expression): product of all parameters in definition order
4. When adaptive chunking is active, the adaptive child is moved to the last
   (innermost) position in the `ProductNode` so it varies fastest during iteration,
   matching the Python reference implementation's behavior

## Chunking

Chunking divides a parameter's range into groups (chunks) for batch processing.

`CHUNK[INT]` values are bounded to magnitudes below 2^62 (`CHUNK_INT_VALUE_BOUND`), a
narrower domain than a range expression accepts (every `i64` except `i64::MIN`; see
[expr range-expr.md](../expr/range-expr.md#value-bounds)). The chunking arithmetic
(`last - first + 1`, `value + 1` adjacency) is plain i64, which is exact inside that bound.
`check_chunk_int_value_bound` rejects a list or range-expression range with a value at or
beyond it, at job creation and again in `StepParameterSpaceIterator::new` (a
`StepParameterSpace` is publicly constructible), with the error
`Task parameter '<name>': value <v> exceeds the CHUNK[INT] range value bound (magnitude
must be below 2^62)`. A range expression's values are sorted, so only its first and last
values are checked.

A literal (`{{`-free) range expression is also checked during template validation, so
`openjd check` reports it with the template path:
`steps[0] -> parameterSpace -> taskParameterDefinitions[0]: CHUNK[INT] parameter '<name>'
range expression error: value <v> exceeds the CHUNK[INT] range value bound (magnitude must be
below 2^62)`.

**Deviation from Template Schemas §3.4.1.5:** a `CHUNK[INT]` range is an `<IntRangeExpr>` or
a list of `<Int>`, and `<Int>` is any integer value (§3.4.1.1.1), so the bound is a limit of
this implementation, not of the specification.

**Divergence from the Python reference:** openjd-model-for-python has no such bound: with
range `"1-3,4611686018427387904"` and `NONCONTIGUOUS`, its CLI runs a chunk
`1-3,4611686018427387904`, while openjd-rs rejects the job with the error above. The bound
is far beyond any frame range, and it keeps the chunking arithmetic exact without
widening it.

The node type used depends on the `rangeConstraint` and whether adaptive chunking
is active:

| Condition | Node Type |
|-----------|-----------|
| `targetRuntimeSeconds > 0` (adaptive) | `AdaptiveChunkNode` |
| `rangeConstraint == Contiguous` (static) | `ContiguousChunkNode` |
| `rangeConstraint == Noncontiguous` (static) | `StaticChunkNode` |

### Contiguous Chunking

`ContiguousChunkNode` respects gaps in the source range. It first identifies contiguous
intervals (runs of consecutive integers), then divides each interval independently into
evenly-sized chunks. This matches the Python `divide_int_list_into_contiguous_chunks`
algorithm.

For example, range `"1,10-12,18-50"` with `defaultTaskCount=10` produces chunks:
`"1-1"`, `"10-12"`, `"18-27"`, `"28-37"`, `"38-47"`, `"48-50"` — gaps between 1, 10,
and 18 are never bridged.

**Interval groups.** The node never lists values. `interval_groups()` turns the range into
a sequence of `IntervalGroup`s, built once when the node is built:

- `Interval { first, last }`: one interval, the values `first..=last`;
- `Singles { first, step, count }`: `count` intervals of one value each, `first`,
  `first + step`, and so on.

For a range expression the groups come from its runs in O(R): a step-1 run (including a
one-value run) is an interval, merged into the interval before it when its first value is
one more than that interval's last. A run with a step above 1 separates every pair of its
values, so its first and last values are one-value intervals (which can merge with a
neighbour) and its inner values are one `Singles` group. So a range of `R` runs has at most
`3R` groups however many values it has: `"1-1000000000000:2"` (5e11 intervals) has two. A
list's intervals follow list order, as in the reference: a value joins the interval before
it when it is one more than the value before it in the list, so a list of `N` values has at
most `N` groups.

**Even distribution** within each interval uses the Python `divide_chunk_sizes` algorithm:
`chunk_count = ceil(interval_len / default_task_count)`, then `small = interval_len / chunk_count`
with leftovers distributed evenly across chunks. A `Singles` group has one chunk per value.

**Counting and random access.** The node caches the cumulative chunk count through each
group, so `len()` is O(1) and `chunk_at(index)` finds the group by binary search, O(log G)
for `G` groups, then computes the chunk within it arithmetically (below), O(1) however large
the interval. Iteration walks the groups in O(1) per chunk.

**Containment.** For a range expression the groups are in increasing order of value, so
`validate_containment()` finds the group holding the candidate's first value by binary
search, then the chunk within it holding that value by binary search over the chunk starts,
and compares that one chunk with the candidate: the chunks are disjoint, so no other can
equal it. That is O(log G + log chunks). A list's groups may be unsorted, so its chunks (at
most one per value, already in memory) are compared one by one.

Within an interval, chunk `j` gets one extra value when
`ceil((j+1)*leftovers/chunk_count) > ceil(j*leftovers/chunk_count)`, so the number of
larger chunks before `j` telescopes to `ceil(j*leftovers/chunk_count)`. That is what lets
the offset and size of chunk `j` be computed without walking the chunks before it.

### Noncontiguous Static Chunking

`StaticChunkNode` computes chunk boundaries lazily via O(1) arithmetic. Its
`StaticLayout` holds the chunk count, base chunk size (`small = total / num_chunks`), and
remainder count (`leftovers = total % num_chunks`); the first `leftovers` chunks take one
more value. Both `StaticChunkNode` and `StaticChunkIterator` build chunk `i` with the
shared `static_chunk()`.

A chunk is the values at a window of indices, and `noncontiguous_chunk()` builds it without
listing them. For a range expression it is `RangeExpr::slice(offset, offset + size, 1)`,
O(log R + runs the window spans), so a chunk of `"1-1000000000000"` costs the same as a
chunk of `"1-10"`. A list is already in memory and goes through `RangeExpr::from_values`.
Both write the chunk in the canonical form of Template Schemas §3.4.1.1.1, as §3.4.1.5
(`rangeConstraint`) requires: for example `[1,3,5]` → `"1-5:2"`, `[1,2,4,6]` →
`"1,2-6:2"`, `[1,2,3,5,7,8,9]` → `"1-3,5,7-9"`.

**Divergence from the Python reference:** `IntRangeExpr.from_list` consumes a value that
cannot start a run of three together with the value after it, so it writes `[1,2,4,6]` as
`"1,2,4,6"`, `[1,3,4,5]` as `"1,3,4,5"`, and `[1,2,3,5,7,8,9]` as `"1-3,5,7,8,9"`. The
canonical form writes the first value alone and lets the second start a run. The two agree
whenever no such pair occurs.

**The run cap.** A chunk is a `RangeExpr`, and every `RangeExpr` has at most
`openjd_expr::MAX_RANGE_EXPR_CHUNKS` (50,000) runs in its canonical form
(`specs/expr/range-expr.md`, Defensive Caps). A chunk of `n` values has at most `n` runs,
so chunks of at most 50,000 values always fit. A larger chunk can fail to: it is a window
of the range's values, and its canonical form can have more runs than the range has. A
CONTIGUOUS chunk is one run, so only NONCONTIGUOUS chunking is affected.

*Bound.* Let `T` be the range's sorted values, with canonical runs `G(T)`. The canonical
form of a window `W = T[i..j)` has at most `I + 2` runs, where `I` is the number of runs of
`G(T)` that `W` intersects. The bound is reached: `"0-2,4-6"` has 2 runs and its window
`[1:5]` is `"1,2,4,5"`, 4 runs. Proof, writing `g(q)` for the number of runs the greedy
canonical algorithm produces on the suffix `T[q..]`:

1. *Prefixes.* The canonical runs of `T[0..j)` are the runs of `G(T)` before the one
   holding `T[j - 1]`, unchanged, followed by at most two runs for the rest of that run.
   An earlier run of three or more values is decided by the values through the one after
   its end, all in the prefix. An earlier one-value run at `q` is decided by `T[q + 1]`,
   in the prefix, and by `T[q + 2]`; where the prefix ends before `T[q + 2]` the extension
   reaches two values either way, so `T[q]` is again alone. The rest of `T[j - 1]`'s run is
   part of an arithmetic progression: one run with three or more values, otherwise one run
   per value.
2. *Lemma: `g(q) <= g(q + 1) + 1` and `g(q + 1) <= g(q) + 1` for every `q`.* Both by one
   induction from the right (`g` of an empty suffix is 0). If the greedy run at `q` is
   `T[q]` alone, `g(q) = 1 + g(q + 1)`, which gives both. If it is `T[q..=f]` with
   `f >= q + 3`, the greedy run at `q + 1` is `T[q + 1..=f]` (the same step and the same
   stopping test), so `g(q + 1) = 1 + g(f + 1) = g(q)`. If `f = q + 2`, the extension at
   `q + 1` reaches only two values (the difference after `T[q + 2]` is not the step, or the
   run at `q` would have continued), so `T[q + 1]` is alone: `g(q) = 1 + g(q + 3)` and
   `g(q + 1) = 1 + g(q + 2)`. The second half of the lemma at `q + 2`,
   `g(q + 3) <= g(q + 2) + 1`, gives `g(q) <= g(q + 1) + 1`; its first half,
   `g(q + 2) <= g(q + 3) + 1`, gives `g(q + 1) <= g(q) + 1`.
3. *Suffixes.* Let `T[i]` fall in the run `T[p..=e]` of `G(T)`, so `g(p) = 1 + g(e + 1)`.
   If `T[i..=e]` has three or more values, the greedy run at `i` also ends at `e`, so
   `g(i) = g(p)`. If it has two, the extension at `e - 1` reaches two values, so
   `g(e - 1) = 1 + g(e) <= g(e + 1) + 2 = g(p) + 1` by the lemma. If it has one,
   `g(e) <= g(e + 1) + 1 = g(p)`. So `g(i) <= g(p) + 1`.
4. *Windows.* Let `T[i]` fall in run `k` of `G(T)` and `T[j - 1]` in run `m`, so
   `I = m - k + 1`. If `k = m`, `W` is part of one progression: at most two runs.
   Otherwise, by 1, the canonical runs of `T[0..j)` are runs `k` to `m - 1` of `G(T)`
   (from `T[i]`'s run on), unchanged, then at most two runs: at most `I + 1` runs from the
   start of `T[i]`'s run. By 3, applied to `T[0..j)`, its suffix from `i`, which is `W`,
   has at most one more: `I + 2`.

*Check.* Chunks are built during iteration, which cannot return an error, so
`make_chunk_node` checks the static chunking when the node is built
(`check_noncontiguous_chunks_fit`). For a range expression with `R` runs: if the largest
chunk has at most 50,000 values, or `R + 2 <= 50,000`, every chunk fits. Otherwise
`R >= 49,999`, and a chunk that does not fit intersects `I >= 49,999 >= R - 1` runs. The
runs a window intersects are consecutive, so it misses at most one run, the first or the
last, and contains every value of run 2 (zero-based), including its first. The chunks are disjoint, so only that one chunk is checked, with
`RangeExpr::slice` in O(R), without listing its values. For a list range (already in
memory; a template caps it at 1,024 values, so this matters only for a
`StepParameterSpace` built through the public API) every chunk is checked with
`RangeExpr::from_values`. A chunk that does not fit fails `StepParameterSpaceIterator::new`,
and so job creation, with `Task parameter 'F': chunk 1 (values 50004 to 3499850112): Range
expression has too many sub-ranges in canonical form (50001); maximum is 50000`, naming the
chunk by its index and its smallest and largest values.

**Deviation from Template Schemas §3.4.1.5:** the specification places no limit on the runs
of a chunk, so this is a limit of this implementation. It is reachable only with chunks of
more than 50,000 values, cut from a range expression of at least 49,999 runs or from a
public-API list of more than 50,000 values. (A list is chunked in list order, so a chunk of
an unsorted list can have more runs than the whole list.) openjd-model-for-python has no
such limit.

A CONTIGUOUS chunk is built by `contiguous_chunk()` from the string `"<N>-<M>"` and flagged with
`RangeExpr::with_contiguous(true)`, so it is written `<N>-<M>` even with one or two values
(`"3-3"`, `"7-8"`), the one exception §3.4.1.5 makes to the canonical form. Values
computed from a chunk (slices, `range_expr(list(...))`) do not carry the flag and are
written canonically. Containment checks compare chunks with `RangeExpr` equality, which
compares values, so a chunk string handed back by a caller matches however it is spelled.

### Adaptive Chunking

`AdaptiveChunkNode` produces chunks on the fly. The chunk size is controlled by a value
that callers can update at runtime via `set_chunks_default_task_count()`. This supports
the `targetRuntimeSeconds` feature where chunk sizes are adjusted based on observed task
execution times. The presence of `targetRuntimeSeconds > 0` on a `ChunkInt` parameter
is what triggers adaptive (vs static) chunking.

Adaptive chunking disables random access (`get()` returns `None` and `len()` returns 0)
because chunk boundaries aren't known in advance.

`AdaptiveChunkNode` keeps the range as given, never its list of values. A NONCONTIGUOUS
chunk is the next `chunk size` values by index, built with `noncontiguous_chunk()` like a
static one. A CONTIGUOUS chunk is up to `chunk size` values from the current interval,
taken from the range's interval groups (see Contiguous Chunking). Any chunk size from 1 to
`usize::MAX` is valid: the chunk's end is `cursor.saturating_add(chunk_size)` clamped to
the value count, so a size beyond the values that remain takes all of them. (openjd-cli's
adaptive size saturates at `usize::MAX` when a quick task has a huge
`targetRuntimeSeconds`.)

The chunk size can change at run time, where iteration cannot fail, so a NONCONTIGUOUS
adaptive chunk is never checked in advance against the run cap (see Noncontiguous Static
Chunking). Instead, when the next `chunk size` values have a canonical form with more than
`MAX_RANGE_EXPR_CHUNKS` (50,000) runs, the chunk takes 50,000 values, which always fit,
and the following chunk starts after them. Only a chunk of more than 50,000 values can be
shortened, and only for a range of at least 49,999 runs (or a public-API list of more than
50,000 values). §3.4.1.5 leaves the size of an adaptive chunk to the implementation
(`targetRuntimeSeconds` is a target), so this stays within the specification.

`AdaptiveChunkNode::validate_containment()` accepts any subset of the range. For a range
expression `range_expr_is_subset()` decides it on the runs, O(R_sub log R + R): for each
run of the candidate it finds the range's run that could hold the next unchecked value,
and when that run holds it and its step divides the candidate run's step, skips every value
of the candidate run up to that run's end in one move. For a list it looks each candidate
value up in a `HashSet` of the list; the candidate's values are distinct, so this stops
after at most the list's length plus one lookups however long the candidate is.

`StaticChunkNode::validate_containment()` compares a candidate with the chunk that holds
its first value: for a range expression that value's index (`range_expr_index_of()`,
O(log R)) gives the chunk (`StaticLayout::chunk_holding`), and the values are distinct, so
no other chunk can equal it. A list may repeat a value, so its chunks are compared one by
one. The comparison is `RangeExpr` equality, which compares values (Expression Language
§1.2.5) because every `RangeExpr` stores the runs of its canonical form:
`from_values([6,9])` and the parse of its rendered form `"6,9"` are the same value. This keeps the invariant that a
chunk string printed by the iterator passes validation when handed back (e.g. via the
CLI's `--task-params`).

### Chunk Override

`new_with_chunk_override(space, Some(n))` overrides the `defaultTaskCount` for all chunk
nodes. The override value is threaded through `make_leaf_node` → `make_chunk_node` and
used in place of `chunks.default_task_count`. When an override is provided, adaptive
chunking is suppressed (the parameter uses static chunking with the override size).

### Sequential Iteration

Two separate questions, tracked by two different flags.

**Which path `Iterator::next` takes** is the `sequential` flag: adaptive chunking or
contiguous chunking. Adaptive must be sequential because its chunk size can change
part-way through a walk. Contiguous answers per index in O(log G), but its node iterator
walks the interval groups in O(1) per chunk, so both use the `node_iter` path. Every other space takes the random-access path, where
`Iterator::next` is implemented as `get(current_index)`.

**Whether `get()` answers at all** is gated on adaptive alone. A contiguous space supports
random access; an adaptive one does not, because chunk N is not a function of N.

`len()` returns the exact count except for adaptive, which returns 0.

## Design Decisions

### Lazy Evaluation (vs Eager Expansion)

The Python library originally used `expand_parameter_space()` which materialized the entire
parameter space as a list of dicts. For large spaces (e.g., 1M frames × 3 cameras = 3M tasks),
this consumed significant memory.

The Rust crate uses lazy evaluation via the node tree. `RangeExprNode` computes values on
demand via index arithmetic. `ProductNode` uses divmod indexing. Memory usage is O(1)
regardless of space size (for non-list ranges).

### Index Arithmetic for Random Access

Random access (`get(index)`) is O(D) node lookups for all non-adaptive node types (where D
is the number of dimensions); a chunk node's lookup costs what the Node Types table says. This is important for schedulers that need to access arbitrary tasks without
iterating from the beginning. The divmod decomposition in `ProductNode` avoids materializing
intermediate products.

### Reusable TaskParameterSet

The Python iterator mutates a passed-in dict rather than allocating a new one per iteration.
The Rust iterator returns a new `TaskParameterSet` (HashMap) per call to `next()`, but the
node tree's `fill()` method writes into a pre-allocated map to minimize allocation. The
HashMap itself is allocated once and reused across iterations.
