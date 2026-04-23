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
}

impl Iterator for StepParameterSpaceIterator {
    type Item = TaskParameterSet;
}
```

`new` returns `Result<Self, ModelError>` because construction can fail (e.g., if the
product of parameter dimensions overflows `usize`). `get` returns `Option<TaskParameterSet>`
(returns `None` for out-of-bounds indices or when sequential iteration is required).
`set_chunks_default_task_count` takes `&mut self` (no `Arc<AtomicUsize>` indirection).

`new_with_chunk_override` accepts an optional chunk size that overrides the template's
`defaultTaskCount` for all chunk nodes. When `Some(n)`, it also suppresses adaptive
chunking (the parameter is treated as static with chunk size `n`).

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
| `ContiguousChunkNode` | Gap-respecting contiguous chunks | Cached exact count | Sequential only |
| `StaticChunkNode` | Noncontiguous chunk boundaries | Number of chunks | O(1) |
| `AdaptiveChunkNode` | Runtime-adjustable chunks | Dynamic | Sequential only |

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

**Chunk count computation** uses the `RangeExpr` sub-range structure (`Vec<IntRange>`)
for O(R) complexity where R is the number of sub-ranges, not O(N) over individual values.
A 100-billion-element contiguous range like `"1-100000000000"` has one sub-range, so
counting is instant.

**Even distribution** within each interval uses the Python `divide_chunk_sizes` algorithm:
`chunk_count = ceil(interval_len / default_task_count)`, then `small = interval_len / chunk_count`
with leftovers distributed evenly across chunks.

`ContiguousChunkNode` is sequential-only (no random access via `get()`) because chunk
boundaries depend on scanning for gaps from the beginning. The exact chunk count is
cached at construction time.

### Noncontiguous Static Chunking

`StaticChunkNode` computes chunk boundaries lazily via O(1) arithmetic. It stores only
the total range size, chunk count, base chunk size (`small = total / num_chunks`), and
remainder count (`leftovers = total % num_chunks`). Both `StaticChunkNode` and
`StaticChunkIterator` delegate to a shared `build_chunk_range_expr()` function.

On `get(i)`, the node slices into the underlying range at the computed offset and builds
a `RangeExpr` string on the fly. `compress_range_expr()` compresses the slice into compact
form, detecting constant-step arithmetic sequences (e.g., `[1,3,5]` → `"1-5:2"`,
`[1,2,3,5,7,8,9]` → `"1-3,5,7-9"`). Step detection requires 3+ values in a run.

### Adaptive Chunking

`AdaptiveChunkNode` produces chunks on the fly. The chunk size is controlled by a value
that callers can update at runtime via `set_chunks_default_task_count()`. This supports
the `targetRuntimeSeconds` feature where chunk sizes are adjusted based on observed task
execution times. The presence of `targetRuntimeSeconds > 0` on a `ChunkInt` parameter
is what triggers adaptive (vs static) chunking.

Adaptive chunking disables random access (`get()` returns `None` and `len()` returns 0)
because chunk boundaries aren't known in advance.

`AdaptiveChunkNode::validate_containment()` uses a `HashSet` for O(N) lookup.

### Chunk Override

`new_with_chunk_override(space, Some(n))` overrides the `defaultTaskCount` for all chunk
nodes. The override value is threaded through `make_leaf_node` → `make_chunk_node` and
used in place of `chunks.default_task_count`. When an override is provided, adaptive
chunking is suppressed (the parameter uses static chunking with the override size).

### Sequential Iteration

When any node in the tree requires sequential iteration (adaptive chunking or contiguous
chunking), the `StepParameterSpaceIterator` uses the `node_iter` path instead of
random-access `get()`. The `sequential` flag tracks this. `len()` still returns the
exact count (except for adaptive, which returns 0).

## Design Decisions

### Lazy Evaluation (vs Eager Expansion)

The Python library originally used `expand_parameter_space()` which materialized the entire
parameter space as a list of dicts. For large spaces (e.g., 1M frames × 3 cameras = 3M tasks),
this consumed significant memory.

The Rust crate uses lazy evaluation via the node tree. `RangeExprNode` computes values on
demand via index arithmetic. `ProductNode` uses divmod indexing. Memory usage is O(1)
regardless of space size (for non-list ranges).

### Index Arithmetic for Random Access

Random access (`get(index)`) is O(D) for all non-adaptive node types (where D is the number
of dimensions). This is important for schedulers that need to access arbitrary tasks without
iterating from the beginning. The divmod decomposition in `ProductNode` avoids materializing
intermediate products.

### Reusable TaskParameterSet

The Python iterator mutates a passed-in dict rather than allocating a new one per iteration.
The Rust iterator returns a new `TaskParameterSet` (HashMap) per call to `next()`, but the
node tree's `fill()` method writes into a pre-allocated map to minimize allocation. The
HashMap itself is allocated once and reused across iterations.
