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
(returns `None` for out-of-bounds indices). `set_chunks_default_task_count` takes `&mut self`
(no `Arc<AtomicUsize>` indirection).

## Node Tree Architecture

The iterator is built on a tree of `Node` trait objects. Each node represents a dimension
or operation in the parameter space:

```
ProductNode (cartesian product)
├── RangeListNode ("color": ["red", "green", "blue"])
├── AssociationNode (lockstep iteration)
│   ├── RangeExprNode ("frame": "1-3")
│   └── RangeListNode ("camera": ["main", "side", "top"])
└── StaticChunkNode (chunked parameter)
    └── RangeExprNode ("tile": "1-256")
```

### Node Types

| Node | Purpose | Length | Random Access |
|------|---------|--------|---------------|
| `ZeroDimSpaceNode` | Empty parameter space | 1 | O(1) |
| `RangeListNode` | Pre-materialized value list | List length | O(1) |
| `RangeExprNode` | Integer range expression | Computed from range | O(1) via arithmetic |
| `ProductNode` | Cartesian product (`*` operator) | Product of children (checked) | O(D) via divmod |
| `AssociationNode` | Lockstep (`,` in parens) | Min of children | O(1) |
| `StaticChunkNode` | Lazy chunk boundary computation | Number of chunks | O(1) |
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

## Chunking

Chunking divides a parameter's range into groups (chunks) for batch processing.

### Static Chunking

`StaticChunkNode` computes chunk boundaries lazily via O(1) arithmetic. It stores only
the total range size, chunk count, base chunk size (`small = total / num_chunks`), and
remainder count (`leftovers = total % num_chunks`). The offset and size of chunk `i` are:

- size = `small + 1` if `i < leftovers`, else `small`
- offset = `i * small + min(i, leftovers)`

On `get(i)`, the node slices into the underlying range at the computed offset and builds
a `RangeExpr` string on the fly. For contiguous chunks this is just `"{start}-{end}"`;
for noncontiguous chunks, `compress_range_expr()` compresses the slice into compact form
(e.g., `[1,2,3,5,7,8,9]` → `"1-3,5,7-9"`).

Supports `RangeConstraint::Contiguous` (chunks are contiguous subsequences) and
`RangeConstraint::Noncontiguous` (chunks can be arbitrary subsets).

### Adaptive Chunking

`AdaptiveChunkNode` produces chunks on the fly. The chunk size is controlled by a value
that callers can update at runtime via `set_chunks_default_task_count()`. This supports
the `targetRuntimeSeconds` feature where chunk sizes are adjusted based on observed task
execution times. The presence of `targetRuntimeSeconds > 0` on a `ChunkInt` parameter
is what triggers adaptive (vs static) chunking.

Adaptive chunking disables random access (`get()` returns `None` and `len()` returns 0)
because chunk boundaries aren't known in advance.

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
