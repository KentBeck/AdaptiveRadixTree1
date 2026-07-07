# An Adaptive Radix Tree — By Operation

The previous document walked through this tree by *structure*: tagged
pointers, then node types, then algorithms.  This document walks
through it by *operation*: what happens when you get, put, delete, or
range-scan.  For each operation we start with the top-level flow,
then show what happens at a leaf, then at each of the four inner node
types.

A quick refresher on the cast: every position in the tree holds a
`NodePtr`, a tagged pointer that is either null, a `Leaf` (full key +
value), or one of four inner node types (`Node4`, `Node16`, `Node48`,
`Node256`).  Inner nodes carry a path-compression prefix, an optional
value (for keys that are prefixes of other keys), and children keyed
by a single byte.  The four types differ only in how they store and
look up those children.

---

## Get

### The top-level walk

`get` takes a key and walks from the root toward a leaf, consuming
one byte of the key at each level.

```rust
unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
    let mut node = self.root;
    let mut depth = 0;

    while !node.is_null() {
        if node.is_leaf() { ... }

        // Inner node: check prefix
        let prefix = inner_prefix_raw(node);
        let plen = prefix.len();
        if key.len() < depth + plen || key[depth..depth + plen] != *prefix {
            return None;
        }
        depth += plen;

        if depth == key.len() {
            return inner_value_raw(node).map(|(_, v)| v);
        }

        let b = key[depth];
        node = inner_find(node, b);
        depth += 1;
    }
    None
}
```

Three things happen at each inner node, always in this order:

1. **Check the prefix.**  Path compression means some bytes of the key
   were collapsed into the node's prefix rather than branching.  If the
   key doesn't match the prefix, the key isn't in the tree.

2. **Check for key exhaustion.**  If we've consumed the entire key but
   we're at an inner node, the key's value (if any) lives on this node.
   This is the prefix-key case: `"ab"` is stored on the inner node at
   the branch point between `"abc"` and `"abd"`.

3. **Descend.**  Consume the next byte of the key, find the
   corresponding child, and repeat.

### At a Leaf

When the walk reaches a leaf, we compare the full key:

```rust
if node.is_leaf() {
    let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
    if *leaf.key == *key {
        return Some(&leaf.value);
    }
    return None;
}
```

Why compare the full key when the trie path already matched each byte?
Because path compression skips bytes.  The prefix on an inner node
tells us those bytes matched, but a leaf reached via path compression
might not be the key we're looking for.  The full-key comparison is
the final proof.

### At Node4: linear scan

Node4 stores up to 4 children in a sorted parallel array of keys and
child pointers.  Finding a child is a linear scan:

```rust
KIND_NODE4 => {
    let n = node.as_node4();
    for i in 0..n.count as usize {
        if n.keys[i] == b { return n.children[i]; }
    }
    NodePtr::NULL
}
```

Four comparisons at most.  The sorted order isn't needed for lookup
(linear scan checks everything) but matters for iteration and for
insertion, which maintains sort order.

### At Node16: binary search

Node16 stores up to 16 children in the same sorted parallel-array
layout.  With 16 entries, binary search beats linear scan:

```rust
KIND_NODE16 => {
    let n = node.as_node16();
    match n.keys[..n.count as usize].binary_search(&b) {
        Ok(i) => n.children[i],
        Err(_) => NodePtr::NULL,
    }
}
```

At most 4 comparisons (log2(16) = 4).  The original ART paper uses
SIMD for this lookup — a single SSE instruction compares all 16 bytes
at once.  We don't, yet.

### At Node48: index table

Node48 uses a 256-byte index array that maps each byte value to a
slot number (or `0xFF` for "empty"), plus a 48-slot array of child
pointers.  Lookup is two array reads:

```rust
KIND_NODE48 => {
    let n = node.as_node48();
    let idx = n.index[b as usize];
    if idx == 0xFF { NodePtr::NULL } else { n.slots[idx as usize] }
}
```

O(1) regardless of how many of the 48 slots are occupied.  The cost
is the 256-byte index array, but that buys constant-time lookup
without needing to search.

### At Node256: direct indexing

Node256 is the simplest.  The byte value *is* the index:

```rust
KIND_NODE256 => node.as_node256().children[b as usize]
```

One array access.  A null child pointer means "not present."  The
cost is 256 pointers (2 KiB on 64-bit), so we only use this
representation when a node has more than 48 children.

### The four find strategies, summarized

| Node type | Storage      | Lookup strategy | Comparisons |
|-----------|--------------|-----------------|-------------|
| Node4     | sorted array | linear scan     | up to 4     |
| Node16    | sorted array | binary search   | up to 4     |
| Node48    | index table  | two array reads | 0           |
| Node256   | direct array | one array read  | 0           |

Each strategy is the natural fit for its capacity range: small arrays
are fastest to scan, large arrays are fastest to index.

---

## Put

### The top-level flow

`put_recursive` walks the tree like `get`, but modifies the tree on
the way.  It returns `(new_node, was_new_key)` — the (possibly
replaced) subtree root and whether the key was new.

There are five situations:

1. **Empty slot** — create a leaf.
2. **Leaf, same key** — update the value.
3. **Leaf, different key** — split into a new inner node.
4. **Inner node, partial prefix match** — split the prefix.
5. **Inner node, full prefix match** — descend (or store value if key
   exhausted).

### Empty slot

The simplest case.  We've descended past the last child into a null
pointer.  Make a leaf:

```rust
if node.is_null() {
    let leaf = Box::new(Leaf { key: Box::from(key), value });
    return (NodePtr::from_leaf(leaf), true);
}
```

### At a Leaf: same key

If the leaf holds the same key, we're updating an existing entry:

```rust
if *existing.key == *key {
    let mut leaf_box = node.into_leaf_box();
    leaf_box.value = value;
    return (NodePtr::from_leaf(leaf_box), false);
}
```

`into_leaf_box` reclaims ownership of the `Box` from the raw pointer.
We write the new value and return the same leaf (same pointer, even).
`false` tells the caller this wasn't a new key.

### At a Leaf: different key — the split

This is where the tree grows.  Two keys share a path up to this point
but now diverge.  We find where they diverge and create a Node4 to
hold both:

```rust
let common = prefix_mismatch(key, depth, ekb, depth);
let sd = depth + common;

let mut nn = Box::new(Node4::<V>::new());
nn.prefix = Prefix::from_slice(&key[depth..sd]);
let mut nn_ptr = NodePtr::from_node4(nn);
```

`sd` is the "split depth" — the first byte where the two keys
differ.  Everything before that becomes the new node's prefix (path
compression).

Three sub-cases handle where the keys diverge:

**New key is a prefix of the existing** (`sd == key.len()`).  The new
key's value goes on the inner node; the existing leaf becomes a child:

```rust
inner_set_value(&mut nn_ptr, Box::from(key), value);
inner_add_child(&mut nn_ptr, ekb[sd], node);
```

**Existing key is a prefix of the new** (`sd == ekb.len()`).  Mirror
image — the existing key's value goes on the inner node:

```rust
let existing_box = node.into_leaf_box();
inner_set_value(&mut nn_ptr, existing_box.key, existing_box.value);
let new_leaf = Box::new(Leaf { key: Box::from(key), value });
inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
```

**Both have more bytes** — two leaf children:

```rust
inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
inner_add_child(&mut nn_ptr, ekb[sd], node);
```

### At an inner node: partial prefix match — the prefix split

When the key diverges partway through an inner node's prefix, we
split the prefix.  A new Node4 holds the shared part; the old node
keeps the suffix:

```rust
if ml < plen {
    let mut nn = Box::new(Node4::<V>::new());
    nn.prefix = Prefix::from_slice(&prefix[..ml]);
    let mut nn_ptr = NodePtr::from_node4(nn);

    let mut old_node = node;
    inner_set_prefix(&mut old_node, Prefix::from_slice(&prefix[ml + 1..]));
    inner_add_child(&mut nn_ptr, prefix[ml], old_node);

    // ... add the new key as a leaf or value
}
```

The byte at position `ml` — the first byte where the key and prefix
disagree — becomes the branch byte.  The old node becomes a child
under that byte, and the new key goes under its diverging byte.

### At an inner node: full prefix match — descend or store

If the prefix matches fully, we either store the value on this node
(key exhausted) or descend to a child:

```rust
if nd == key.len() {
    let added = !inner_has_value(&node);
    inner_set_value(&mut node, Box::from(key), value);
    return (node, added);
}

let b = key[nd];
let child = inner_find(node, b);
```

If the child slot is empty, we create a new leaf there.  If the node
is full, we grow it first.

### Adding a child to Node4

Node4 keeps its keys sorted.  Adding a child means finding the
insertion position, shifting entries right, and inserting:

```rust
KIND_NODE4 => {
    let n = node.as_node4_mut();
    let cnt = n.count as usize;
    let pos = n.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
    for i in (pos..cnt).rev() {
        n.keys[i + 1] = n.keys[i];
        n.children[i + 1] = n.children[i];
    }
    n.keys[pos] = b;
    n.children[pos] = child;
    n.count += 1;
}
```

With at most 3 entries to shift, this is cheap.

### Adding a child to Node16

Identical logic — sorted insert with shift.  With up to 15 entries to
shift, still fast enough.

### Adding a child to Node48

Node48 doesn't maintain sorted order.  It finds any free slot and
links it:

```rust
KIND_NODE48 => {
    let n = node.as_node48_mut();
    let slot = (0u8..48).find(|&j| n.slots[j as usize].is_null()).unwrap();
    n.index[b as usize] = slot;
    n.slots[slot as usize] = child;
    n.count += 1;
}
```

The linear scan for a free slot is at most 48 checks.  The index
table handles the mapping from byte to slot, so insertion order
doesn't affect lookup speed.

### Adding a child to Node256

The byte value *is* the index.  Adding a child is one array write:

```rust
KIND_NODE256 => {
    let n = node.as_node256_mut();
    n.children[b as usize] = child;
    n.count += 1;
}
```

### Growing: when a node is full

When `inner_add_child` needs a slot and there isn't one, `grow`
promotes the node to the next size.  The steps are always the same:
allocate the bigger node, move the header (prefix + value), copy the
children, free the old shell.

The header move uses `take` semantics to avoid double-free:

```rust
fn inner_move_header<V>(src: &mut NodePtr<V>, dst: &mut NodePtr<V>) {
    let prefix = inner_take_prefix(src);
    let value = inner_clear_value(src);
    inner_set_prefix(dst, prefix);
    // ... set dst.value = value
}
```

`inner_take_prefix` replaces the source's prefix with an empty one.
`inner_clear_value` replaces the source's value with `None`.  Then
`free_inner_node_shell` drops the source's `Box` without recursing
into children — those children now live in the new node.

The growth transitions:

| From    | To      | Child copy strategy                       |
|---------|---------|-------------------------------------------|
| Node4   | Node16  | Copy sorted key + child arrays            |
| Node16  | Node48  | Build index table from sorted keys        |
| Node48  | Node256 | Scatter slots into 256-entry direct array  |

Each transition changes the child-lookup strategy to match the new
capacity.  Node16→Node48 is the most interesting: it walks the sorted
keys and assigns each one an index-table entry, converting from
"search for the byte" to "index by the byte."

---

## Delete

### The top-level walk

`delete_recursive` walks to the target, removes it, and compacts the
tree on the way back up.  It returns `(new_node, was_deleted)`:

```rust
fn delete_recursive<V>(
    node: NodePtr<V>, key: &[u8], depth: usize,
) -> (NodePtr<V>, bool) {
    if node.is_null() { return (NodePtr::NULL, false); }
    if node.is_leaf() { ... }
    // Inner node: check prefix, descend, remove, compact
}
```

### At a Leaf

If the leaf's key matches, free it and return null:

```rust
if *node.as_leaf().key == *key {
    drop(node.into_leaf_box());
    return (NodePtr::NULL, true);
}
return (node, false);
```

`into_leaf_box` reconstructs the `Box` from the raw pointer, and
`drop` frees the memory.  The parent will see the null and remove
this child slot.

### At an inner node: descending

Like `get`, we check the prefix, then look up the next byte.  But
after recursing, we have to update the tree:

```rust
let (new_child, deleted) = delete_recursive(child, key, nd + 1);
if !deleted { return (node, false); }

if new_child.is_null() {
    inner_remove_child(&mut node, b);
} else {
    inner_replace_child(&mut node, b, new_child);
}

(compact(node), true)
```

If the child was deleted entirely (returned null), we remove the
child slot.  If it was replaced (e.g., a Node16 shrank to a Node4),
we update the pointer.  Then we compact.

### Deleting a value from an inner node

When the key matches the prefix and is exactly exhausted (`depth ==
key.len()`), the value lives on the inner node itself:

```rust
if nd == key.len() {
    if !inner_has_value(&node) { return (node, false); }
    inner_clear_value(&mut node);
    return (compact(node), true);
}
```

### Removing a child from Node4

Find the byte, shift everything left to close the gap:

```rust
KIND_NODE4 => {
    let n = node.as_node4_mut();
    let cnt = n.count as usize;
    if let Some(pos) = n.keys[..cnt].iter().position(|&k| k == b) {
        for i in pos..cnt - 1 {
            n.keys[i] = n.keys[i + 1];
            n.children[i] = n.children[i + 1];
        }
        n.children[cnt - 1] = NodePtr::NULL;
        n.count -= 1;
    }
}
```

### Removing a child from Node16

Same shift logic, but uses binary search to find the position.

### Removing a child from Node48

No shifting needed — just clear the index entry and the slot:

```rust
KIND_NODE48 => {
    let n = node.as_node48_mut();
    let idx = n.index[b as usize];
    if idx != 0xFF {
        n.slots[idx as usize] = NodePtr::NULL;
        n.index[b as usize] = 0xFF;
        n.count -= 1;
    }
}
```

This leaves a hole in the slots array, which is fine — new insertions
find holes by scanning for null slots.

### Removing a child from Node256

One array write:

```rust
KIND_NODE256 => {
    let n = node.as_node256_mut();
    n.children[b as usize] = NodePtr::NULL;
    n.count -= 1;
}
```

### Compaction: cleaning up after removal

After a child removal, the node might be degenerate — too few children
for its type.  `compact` handles three cases, then checks whether to
shrink:

**Zero children, has value** — the inner node exists only to hold a
value.  Convert it to a leaf:

```rust
if count == 0 && inner_has_value(&node) {
    let val = inner_clear_value(&mut node);
    free_inner_node_shell(node);
    let (k, v) = val.unwrap();
    return NodePtr::from_leaf(Box::new(Leaf { key: k, value: v }));
}
```

**Zero children, no value** — completely empty.  Free and return null:

```rust
free_inner_node_shell(node);
return NodePtr::NULL;
```

**One child, no value** — a single-child node wastes a level of
indirection.  Merge it with its child.  If the child is a leaf, just
return the leaf.  If the child is an inner node, concatenate the
prefixes:

```rust
if count == 1 && !inner_has_value(&node) {
    let (b, child) = children[0];
    if child.is_leaf() {
        free_inner_node_shell(node);
        return child;
    }
    // parent.prefix + byte + child.prefix → child.prefix
    let parent_prefix = inner_prefix_raw(node).to_vec();
    free_inner_node_shell(node);
    let child_prefix = inner_take_prefix(&mut child);
    let mut merged = parent_prefix;
    merged.push(b);
    merged.extend_from_slice(child_prefix.as_slice());
    inner_set_prefix(&mut child, Prefix::from_slice(&merged));
    return child;
}
```

This prefix re-merging is the inverse of the prefix split in `put`.
Without it, inserts and deletes would leave chains of single-child
nodes, degrading every subsequent lookup through that part of the
tree.

**Below threshold** — shrink to a smaller node type:

```rust
let should_shrink = match node.kind() {
    KIND_NODE256 => count <= 48,
    KIND_NODE48  => count <= 16,
    KIND_NODE16  => count <= 4,
    _ => false,
};
```

`shrink` mirrors `grow`: allocate the smaller type, move the header,
copy the children, free the old shell.

---

## Range Query

### The idea

The naive way to range-scan is to iterate everything and filter.
That's O(n).  The range iterator does O(log n + k) by pruning entire
subtrees at each level — if a subtree's prefix falls entirely outside
the range, we skip it.

Each stack frame carries four things:

```rust
struct RangeFrame<'a, V> {
    node: NodePtr<V>,
    depth: usize,
    lo: Option<&'a [u8]>,   // lower bound (or None = unbounded)
    hi: Option<&'a [u8]>,   // upper bound (or None = unbounded)
}
```

The bounds narrow as we descend.  A child that is strictly between the
boundary bytes gets `lo: None, hi: None` — it's fully inside the
range, so everything in its subtree qualifies without further
checking.

### At a Leaf

The simplest case.  Check the full key against the bounds:

```rust
if node.is_leaf() {
    let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
    let kb = &leaf.key[..];
    if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
        return Some((kb, &leaf.value));
    }
    continue;
}
```

When both bounds are `None` (subtree fully in range), the two
`map_or(true, ...)` calls both return `true` and the leaf is yielded
unconditionally.

### At an inner node: prefix analysis

Before looking at children, we compare the node's prefix against
the bounds.  The goal is to answer three questions for each bound:

1. Is the entire subtree outside the range?  (Skip this node.)
2. Has the bound been passed?  (Relax it to `None` for children.)
3. Is the bound still active?  (Propagate it to the boundary child.)

For the lower bound, the logic compares the prefix bytes against the
corresponding `lo` bytes:

- **Prefix < lo bytes**: the entire subtree is below the range.  Skip.
- **Prefix > lo bytes**: we've passed the lower bound.  Set `lo = None`.
- **Prefix == lo bytes**: the bound is still active.  Set `lo_on = true`.

The upper bound uses the same logic in mirror image, with an
additional wrinkle: when `hi` is exhausted partway through the prefix,
only the node's own value (if any) could match — no children can be
in range.

### Pushing children in range

Once we know which bounds are active, we compute `lo_byte` and
`hi_byte` — the byte range of children that could be in the query
range:

```rust
let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };
```

Using `i16` lets us represent "no constraint" as -1 (below any byte)
and 256 (above any byte).  Children outside `[lo_byte, hi_byte]` are
skipped entirely.

For children inside the range, bounds propagate selectively:

```rust
let mut push = |byte: u8, child: NodePtr<V>| {
    let b = byte as i16;
    if b < lo_byte || b > hi_byte { return; }
    let child_lo = if lo_on && b == lo_byte { lo } else { None };
    let child_hi = if hi_on && b == hi_byte { hi } else { None };
    stack.push(RangeFrame { node: child, depth: nd + 1,
                            lo: child_lo, hi: child_hi });
};
```

Only the child *on* the boundary byte receives the bound.  All other
in-range children get `None` — they're fully inside the range.  This
is the key insight that makes range queries O(log n + k): at each
level, at most two children (the lo-boundary and hi-boundary) carry a
bound forward.  Every other child is scanned unconditionally, doing
O(subtree size) total work — which is exactly the k results.

### The node-type dispatch

`push_range_children_rev` iterates children in reverse byte order
(smallest on top of stack, so it's yielded first) and calls the
`push` closure for each.  The per-node-type logic is the same as
`push_children_rev` for full iteration — Node4 and Node16 iterate
their sorted arrays in reverse, Node48 and Node256 scan bytes 255
down to 0 — but with the `push` closure filtering by `lo_byte` /
`hi_byte`.

---

## Speculations About Future Development

### SIMD lookup in Node16

The original ART paper describes using a single SSE `_mm_cmpeq_epi8`
instruction to compare all 16 keys simultaneously.  A 16-byte SIMD
register holds the keys array; a broadcast fills another register
with the search byte; the comparison produces a bitmask; a bit-scan
finds the match.  This replaces four comparisons (binary search) with
one instruction.  The sorted-array layout is already SIMD-friendly.

### Arena allocation

Every leaf and inner node is a separate `Box` — a separate heap
allocation.  This means the tree's nodes are scattered across memory,
causing cache misses during traversal.  An arena allocator could place
nodes in contiguous memory regions, improving spatial locality.
Iteration would benefit most, since it visits every node and
currently chases pointers across the heap.

### Lock-free concurrency

The ART paper includes an optimistic lock coupling scheme (ART-OLC)
where readers proceed without locks and retry if a concurrent writer
modified a node.  The tagged-pointer representation maps naturally to
atomic operations — `NodePtr` is already a `usize`.

### Persistent/immutable variant

A copy-on-write ART would copy inner nodes on mutation rather than
modifying them in place, yielding a persistent (immutable) data
structure where old versions remain valid.  The inner node headers are
small enough that copying them is cheap; the cost is proportional to
the tree height (the path from root to the modified leaf).

### Replacing the old-value hack

The previous version of `put_recursive` returned a third tuple
element — the old value on key overwrite — synthesized via
`unsafe { std::mem::zeroed() }` in every non-overwrite branch.
That was undefined behavior for value types like `Box<T>` or `&T`,
whose bit patterns have invariants that all-zeros violates.  We
simplified the function to return `(NodePtr<V>, bool)` and drop
the old value inside the leaf update rather than propagating it.
If callers need the old value, the right fix is for `put` to return
`Option<V>`, matching `BTreeMap::insert`.

### Avoiding allocation in compact

When `compact` handles the one-child-no-value case, it calls
`inner_children` to get the single remaining child — but
`inner_children` allocates a `Vec` of all children.  A dedicated
`inner_first_child` that returns a single `(u8, NodePtr)` would
avoid that allocation on every compaction.

### Inline keys for short leaves

Leaf keys are stored as `Box<[u8]>` — a heap-allocated slice.  For
short keys (common in practice), the key bytes could be stored inline
in the leaf struct, similar to how `Prefix` inlines short prefixes.
This would eliminate one heap allocation per leaf and improve cache
locality during key comparison.

### Prefix hashing for early rejection

During lookup, the full prefix comparison at each level is O(prefix
length).  Storing a hash of the prefix alongside it would let us
reject mismatches with a single integer comparison, falling back to
the full comparison only on hash match.  For long prefixes this could
save work, though for the typical 1–5 byte prefixes in practice, the
overhead of computing the hash might outweigh the savings.

### Range iterator boundary simplification

The range iterator's prefix-vs-bound analysis handles many cases
inline: bound exhausted, bound active, bound released, subtree pruned.
Factoring this into a helper that returns an enum of outcomes —
`OutOfRange`, `Released`, `Active`, `ExhaustedAtNode` — would make
the flow easier to follow without changing the algorithm.  The current
code works correctly and is well-tested, but its density makes it the
hardest part of the codebase to modify confidently.

### Ordered-index operations

An ART can support rank queries (what's the k-th key?) and select
queries (what's the rank of this key?) by augmenting each inner node
with subtree sizes.  This would add a `size` field to the inner node
header and maintain it during put/delete, giving O(key length)
rank/select instead of the O(n) scan that iteration-based approaches
require.
