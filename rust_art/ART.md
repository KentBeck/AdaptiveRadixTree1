# An Adaptive Radix Tree in Rust

An Adaptive Radix Tree (ART) is an ordered key-value map that uses
the bytes of the key to navigate a 256-way trie.  The "adaptive" part
is that each inner node picks from four representations—4, 16, 48,
or 256 children—so sparse nodes stay small while dense nodes stay
fast.  Path compression collapses single-child chains into a prefix
stored on the node, so long shared prefixes cost one comparison rather
than one node per byte.

This document walks through a Rust implementation that uses raw tagged
pointers for the node union, supports keys that are prefixes of other
keys (by storing values on inner nodes), and includes an O(log n + k)
range scan.  We benchmark it against `BTreeMap` at 10 million keys.

---

## 1. The tagged pointer

Every position in the tree holds a `NodePtr`—a single `usize` that
is either null, a pointer to a `Leaf`, or a pointer to one of four
inner node types.  We steal the low three bits of the pointer for a
tag.  Heap allocations are at least 8-byte aligned, so the low three
bits are always zero in a real pointer.

```
bit 0      = 1  →  Leaf
bits [1:2] = 00 →  Node4
             01 →  Node16
             10 →  Node48
             11 →  Node256
```

```rust
const TAG_LEAF: usize = 1;
const TAG_MASK: usize = 0b111;
const KIND_NODE4: usize   = 0b000;
const KIND_NODE16: usize  = 0b010;
const KIND_NODE48: usize  = 0b100;
const KIND_NODE256: usize = 0b110;

struct NodePtr<V>(usize, std::marker::PhantomData<V>);
```

`NodePtr` is `Copy`—it's just an integer.  We implement `Clone` and
`Copy` manually so that `V` doesn't need to be `Copy`:

```rust
impl<V> Clone for NodePtr<V> {
    fn clone(&self) -> Self { NodePtr(self.0, std::marker::PhantomData) }
}
impl<V> Copy for NodePtr<V> {}
```

Classification is a mask check.  Construction takes a `Box`, leaks it
to a raw pointer, ORs in the tag, and stores the result.  Recovering
the pointer is the reverse: mask off the tag, cast to the right type.

```rust
pub(crate) fn as_leaf<'a>(self) -> &'a Leaf<V> {
        debug_assert!(self.is_leaf());
        unsafe { &*((self.0 & !TAG_MASK) as *const Leaf<V>) }
    }
```

Every inner node type gets the same triple: `as_nodeN`, `as_nodeN_mut`,
`into_nodeN_box`.  The `into_` variant reconstructs the `Box` to
reclaim ownership for deallocation.

---

## 2. Node types

### Leaf

A leaf stores the full key and a value.  We keep the full key so that
`get` can verify an exact match—the trie path only tells you the
bytes consumed so far, and path compression means some bytes were
skipped.  The key is a `Box<[u8]>` rather than a `Vec<u8>` because
keys are write-once—the capacity field would be wasted.

```rust
pub(crate) struct Leaf<V> {
    pub(crate) key: Box<[u8]>,
    pub(crate) value: V,
}

impl<V> Leaf<V> {
    pub(crate) fn new_ptr(key: &[u8], value: V) -> NodePtr<V> {
        NodePtr::from_leaf(Box::new(Leaf {
            key: Box::from(key),
            value,
        }))
    }

    pub(crate) fn matches(&self, key: &[u8]) -> bool {
        *self.key == *key
    }

    pub(crate) fn get_value(&self, key: &[u8]) -> Option<&V> {
        if self.matches(key) {
            Some(&self.value)
        } else {
            None
        }
    }

    pub(crate) fn delete(node: NodePtr<V>, key: &[u8]) -> (NodePtr<V>, bool) {
        if node.as_leaf().matches(key) {
            drop(node.into_leaf_box());
            (NodePtr::NULL, true)
        } else {
            (node, false)
        }
    }

    pub(crate) fn put(node: NodePtr<V>, key: &[u8], value: V, depth: usize) -> (NodePtr<V>, bool) {
        let existing = node.as_leaf();
        if existing.matches(key) {
            let mut leaf_box = node.into_leaf_box();
            leaf_box.value = value;
            return (NodePtr::from_leaf(leaf_box), false);
        }

        let existing_key = &existing.key;
        let common = crate::inner::prefix_mismatch(key, depth, existing_key, depth);
        let sd = depth + common;

        let mut nn = Box::new(Node4::<V>::new());
        nn.header.prefix = crate::prefix::Prefix::from_slice(&key[depth..sd]);

        let mut nn_ptr = NodePtr::from_node4(nn);

        if sd == key.len() {
            crate::inner::inner_set_value(&mut nn_ptr, Box::from(key), value);
            crate::inner::inner_add_child(&mut nn_ptr, existing_key[sd], node);
        } else if sd == existing_key.len() {
            let existing_box = node.into_leaf_box();
            crate::inner::inner_set_value(&mut nn_ptr, existing_box.key, existing_box.value);
            crate::inner::inner_add_child(&mut nn_ptr, key[sd], Leaf::new_ptr(key, value));
        } else {
            let new_b = key[sd];
            let old_b = existing_key[sd];
            crate::inner::inner_add_child(&mut nn_ptr, new_b, Leaf::new_ptr(key, value));
            crate::inner::inner_add_child(&mut nn_ptr, old_b, node);
        }
        (nn_ptr, true)
    }
}
```

### Inner node header

Every inner node carries three things beyond its children:

- **prefix**: an inline `Prefix` for path compression (see below).
  If a chain of nodes each have exactly one child, we collapse them
  into a single node whose prefix stores the skipped bytes.
- **value**: an `Option<(Box<[u8]>, V)>`.  When a key like `"ab"` is
  a prefix of another key like `"abc"`, the value for `"ab"` lives on
  the inner node at the branch point, not in a leaf.  The `Box<[u8]>`
  is the full key (for iteration and verification).
- **count**: how many children are occupied.

```rust
pub(crate) type InnerValue<V> = Option<(Box<[u8]>, V)>;

#[repr(C)]
pub(crate) struct NodeHeader<V> {
    pub(crate) prefix: Prefix,
    pub(crate) value: InnerValue<V>,
    pub(crate) count: u16,
}
```

### Inline prefix

Every inner node has a path-compression prefix.  The first version
stored it as `Vec<u8>`—24 bytes (pointer + length + capacity) plus a
separate heap allocation for the actual bytes.  Since the prefix is
read at every tree level during traversal, that extra pointer chase
costs a cache miss per level.

The fix is a small-buffer enum:

```rust
const INLINE_PREFIX_CAP: usize = 22;

enum Prefix {
    Inline { len: u8, data: [u8; INLINE_PREFIX_CAP] },
    Heap(Box<[u8]>),
}
```

The `Heap` variant (a `Box<[u8]>` fat pointer) forces this enum to
24 bytes—the same size as the `Vec<u8>` it replaces.  We use all
that space: 1 byte discriminant + 1 byte length + 22 bytes of inline
data.  Prefixes up to 22 bytes live directly in the node struct with
zero heap allocation; longer prefixes (rare in practice) fall back to
a heap-allocated `Box<[u8]>`.  For the benchmark workload (keys like
`key000000000000`), prefixes are typically 1–5 bytes and always fit
inline.

### Node4

Up to 4 children.  Keys and children are stored in parallel sorted
arrays.  Lookup is a linear scan.

```rust
pub(crate) struct Node4<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) keys: [u8; 4],
    pub(crate) children: [NodePtr<V>; 4],
}
```

### Node16

Up to 16 children.  Same parallel-array layout, but lookup uses
binary search.

```rust
pub(crate) struct Node16<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) keys: [u8; 16],
    pub(crate) children: [NodePtr<V>; 16],
}
```

### Node48

Up to 48 children.  A 256-byte index maps each byte value to a slot
(or `0xFF` for "empty").  The slots array holds the actual child
pointers.  Lookup is a single index into the 256-byte table, then a
single index into the 48-slot array—O(1) regardless of child count.

```rust
pub(crate) struct Node48<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) index: [u8; 256],
    pub(crate) slots: [NodePtr<V>; 48],
}
```

### Node256

Up to 256 children.  Direct indexing by byte value.  Lookup is a
single array access.

```rust
pub(crate) struct Node256<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) children: [NodePtr<V>; 256],
}
```

The trade-off is memory: Node4 is about 80 bytes, Node256 is about
2 KiB.  The adaptive sizing means most nodes are small (Node4 or
Node16 in practice), while hot dense nodes get the fast direct
indexing of Node48/Node256.

---

## 3. The public API

```rust
pub struct ARTMap<V> {
    root: NodePtr<V>,
    len: usize,
}
```

The surface is small: `put`, `get`, `delete`, `iter`, `range_iter`.

---

## 4. Lookup

`get` walks from root to leaf, consuming one byte of the key at each
level.  At each inner node it first checks the path-compressed prefix,
then looks up the next byte in the node's children.

The key subtlety is the `depth == key.len()` check: if we've consumed
the entire key but we're at an inner node (not a leaf), the key's
value is stored on this inner node—this is the prefix-key case.

```rust
unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
        let mut node = self.root;
        let mut depth = 0;

        while !node.is_null() {
            if node.is_leaf() {
                return node.as_leaf().get_value(key);
            }

            let prefix = inner_prefix_raw(node);
            let plen = prefix.len();
            if key.len() < depth + plen || key[depth..depth + plen] != *prefix {
                return None;
            }
            depth += plen;

            if depth == key.len() {
                return crate::inner::inner_value_raw(node).map(|(_, value)| value);
            }

            let b = key[depth];
            node = inner_find(node, b);
            depth += 1;
        }
        None
    }
```

`inner_find` dispatches on node kind.  Node4 does a linear scan,
Node16 does binary search, Node48 does an index-table lookup, Node256
does a direct array access:

```rust
pub(crate) fn inner_find<V>(node: NodePtr<V>, b: u8) -> NodePtr<V> {
    dispatch!(node, find_child, b)
}
// Now calls dispatch! to the Node impl methods directly.
```

---

## 5. Insertion

`put_recursive` handles four cases, returning `(new_node, was_new_key)`:

**Case 1: Empty slot.**  Create a leaf.

```rust
if node.is_null() {
    let leaf = Box::new(Leaf { key: key.to_vec(), value });
    return (NodePtr::from_leaf(leaf), true, ...);
}
```

**Case 2: Leaf, same key.**  Update the value in place.

```rust
if existing.key == key {
    let mut leaf_box = node.into_leaf_box();
    let old_value = std::mem::replace(&mut leaf_box.value, value);
    return (NodePtr::from_leaf(leaf_box), false, old_value);
}
```

**Case 3: Leaf, different key.**  Find where the keys diverge, create
a Node4 to hold both.  This is where path compression begins—the
shared prefix becomes the new node's prefix.  Three sub-cases handle
the new key being a prefix of the existing, the existing being a
prefix of the new, or both diverging at the same depth:

```rust
let common = prefix_mismatch(key, depth, ekb, depth);
let sd = depth + common;

let mut nn = Box::new(Node4::<V>::new());
nn.prefix = key[depth..sd].to_vec();
let mut nn_ptr = NodePtr::from_node4(nn);

if sd == key.len() {
    // New key is a prefix of existing: value goes on the inner node
    inner_set_value(&mut nn_ptr, key.to_vec(), value);
    inner_add_child(&mut nn_ptr, ekb[sd], node);
} else if sd == ekb.len() {
    // Existing key is a prefix of new: its value goes on the inner node
    inner_set_value(&mut nn_ptr, ekb_clone, existing_box.value);
    inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
} else {
    // Both diverge: two leaf children
    inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
    inner_add_child(&mut nn_ptr, ekb[sd], node);
}
```

**Case 4: Inner node.**  Check prefix match.  On partial match, split
the prefix and create a new Node4 above.  On full match, recurse into
the appropriate child, growing the node first if it's full:

```rust
if inner_is_full(&node) {
    node = grow(node);
}
inner_add_child(&mut node, b, NodePtr::from_leaf(new_leaf));
```

---

## 6. Node growth and shrinkage

When a node is full and needs another child, we allocate the next
size up, move the header (prefix + value), copy the children, and
free the old shell.

The critical detail: `inner_move_header` uses `Option::take` and
`std::mem::take` to *move* the prefix and value out of the source
before freeing it.  An earlier version used `ptr::read` (a bitwise
copy) followed by dropping the source, which caused a use-after-free
when the source's `Vec` was freed while the destination still pointed
to the same allocation.

```rust

```

Growth example—Node4 to Node16:

```rust
KIND_NODE4 => {
    let mut new_ptr = NodePtr::from_node16(Box::new(Node16::new()));
    inner_move_header(&mut node, &mut new_ptr);
    let old = node.as_node4();
    let cnt = old.count as usize;
    let dst = new_ptr.as_node16_mut();
    dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
    dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
    dst.count = cnt as u8;
    free_inner_node_shell(node);
    new_ptr
}
```

`free_inner_node_shell` reconstructs the `Box` to free the node's own
memory, but first zeroes out the count (and clears the index for
Node48, nulls children for Node256) so that `drop_recursive` won't
follow the child pointers that now live in the new node.

Shrinkage is the mirror image, triggered by `compact` when a child
count drops below the threshold (Node256 at 48, Node48 at 16, Node16
at 4).

---

## 7. Deletion and compaction

`delete_recursive` walks to the target, removes it, then calls
`compact` on the way back up.  The interesting cases:

**Deleting an inner node's value** (when key is a prefix of other
keys): clear the value, then compact.

**Compaction** handles three degenerate states:

1. **Zero children, has value** → convert to a leaf.
2. **Zero children, no value** → free the node, return null.
3. **One child, no value** → merge with the child.  If the child is a
   leaf, just return it.  If the child is an inner node, concatenate
   the prefixes: `parent.prefix + connecting_byte + child.prefix`.

```rust
if count == 1 && !inner_has_value(&node) {
    let (b, child) = children[0];
    if child.is_leaf() {
        free_inner_node_shell(node);
        return child;
    }
    // Merge prefixes
    let parent_prefix = inner_prefix_raw(node).to_vec();
    free_inner_node_shell(node);
    let child_prefix = inner_take_prefix(&mut child);
    let mut merged = parent_prefix;
    merged.push(b);
    merged.extend_from_slice(&child_prefix);
    inner_set_prefix(&mut child, merged);
    return child;
}
```

This prefix re-merging is what keeps the tree compressed after
deletions.  Without it, a sequence of inserts and deletes would leave
behind chains of single-child nodes, degrading lookup performance.

---

## 8. Iteration

### First version: recursive collect

The first iteration implementation used a recursive function that
pushed every entry into a caller-supplied `Vec`:

```rust
unsafe fn iter_all<'a, V>(node: NodePtr<V>, out: &mut Vec<(&'a [u8], &'a V)>) {
    if node.is_null() { return; }
    if node.is_leaf() {
        let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
        out.push((&leaf.key, &leaf.value));
        return;
    }
    if let Some((k, v)) = inner_value_raw(node) {
        out.push((k.as_slice(), v));
    }
    for (_, child) in inner_children(&node) {
        iter_all(child, out);
    }
}
```

This was simple but had two allocation costs: the output `Vec` grew to
hold all n entries, and `inner_children` allocated a temporary `Vec` at
every inner node to list its children.  At 10 million keys the
iterate-all benchmark ran at 2.69x BTreeMap's time—the extra
allocation traffic was measurable.

### Optimization: lazy stack-based iterator

The fix was to replace the recursive collect with a lazy `Iterator`
that yields one entry at a time.  An explicit stack of `NodePtr`
values replaces the call stack.  When an inner node is popped, its
children are pushed in reverse byte order (so the smallest byte ends
up on top), and if the node carries a value it's yielded immediately.
Leaves are yielded directly.

```rust
impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a [u8], &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.stack.pop()?;
            if node.is_leaf() {
                let leaf = unsafe { &*((node.0 & !TAG_MASK) as *const Leaf<V>) };
                return Some((&leaf.key, &leaf.value));
            }
            push_children_rev(node, &mut self.stack);
            if let Some((k, v)) = unsafe { inner_value_raw(node) } {
                return Some((k.as_slice(), v));
            }
        }
    }
}
```

`push_children_rev` dispatches on node kind and writes directly to the
stack—no temporary `Vec`.  For Node4/Node16 it iterates the keys array
in reverse; for Node48/Node256 it scans bytes 255 down to 0.

This approach costs O(tree height) stack space, never allocates an
O(n) buffer, and does total O(n) work across all `next()` calls.

The `items()` and `range()` convenience methods still return a `Vec`
for callers that want a snapshot, but they now delegate to the lazy
iterators rather than running their own recursive traversal.

---

## 9. Range scan

The range iterator is the most complex part.  The Python prototype
needed an optimization from O(n) to O(log n + k) to be competitive:
instead of iterating everything and filtering, we prune entire
subtrees at each level.

Each stack frame carries `(node, depth, lo, hi)` where `lo` and `hi`
are the remaining range bounds.  At each inner node, the algorithm:

1. **Prefix analysis** compares the node's prefix against the bound
   bytes to decide whether the entire subtree is outside the range
   (prune), whether the bound is still "active" at the child level
   (`lo_on`/`hi_on`), or whether we've passed the boundary (relax the
   constraint to `None`).

2. **Child pruning** uses the next bound byte to compute `lo_byte`
   and `hi_byte`, skipping children outside that range.

3. **Boundary passing**: only the child on the exact boundary byte
   receives the bound.  All other in-range children scan
   unconditionally—they're fully inside the range.

```rust
let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

let mut push = |byte: u8, child: NodePtr<V>| {
    let b = byte as i16;
    if b < lo_byte || b > hi_byte { return; }
    let child_lo = if lo_on && b == lo_byte { lo } else { None };
    let child_hi = if hi_on && b == hi_byte { hi } else { None };
    stack.push(RangeFrame { node: child, depth: nd + 1, lo: child_lo, hi: child_hi });
};
```

The complexity is O(tree depth) for boundary work plus O(k) for the k
results—the same as a B-tree range scan.

---

## 10. Memory management

Rust has no garbage collector, so `ARTMap` implements `Drop` by
walking the tree and reconstructing each `Box` for deallocation:

```rust
impl<V> Drop for ARTMap<V> {
    fn drop(&mut self) {
        unsafe { self.root.drop_recursive(); }
    }
}
```

`drop_recursive` dispatches on the tag to determine the node type,
iterates its children recursively, then drops the node's own `Box`.
The key invariant: when we `free_inner_node_shell` during
grow/shrink/compact, we zero the count or null the children first, so
that if `drop_recursive` ever sees the node, it won't double-free the
children that were moved to a new node.

---

## 11. Benchmark results

We compare against `BTreeMap<Vec<u8>, usize>` using `key{i:012}`
format keys at three scales.  Each operation processes all n keys.

### 100,000 keys

| Operation        |    ART |  BTreeMap | Ratio |
|------------------|--------|-----------|-------|
| Random put       | 0.044s |    0.052s | 0.84x |
| Sequential put   | 0.030s |    0.031s | 0.95x |
| Random get (hit) | 0.026s |    0.035s | 0.75x |
| Random get (miss)| 0.001s |    0.013s | 0.06x |
| Iterate all      | 0.002s |    0.001s | 2.33x |
| Range query (1%) | 0.000s |    0.000s |   —   |
| Random delete    | 0.040s |    0.036s | 1.13x |

### 1,000,000 keys

| Operation        |    ART |  BTreeMap | Ratio |
|------------------|--------|-----------|-------|
| Random put       | 0.799s |    1.034s | 0.77x |
| Sequential put   | 0.301s |    0.363s | 0.83x |
| Random get (hit) | 0.542s |    0.734s | 0.74x |
| Random get (miss)| 0.008s |    0.175s | 0.05x |
| Iterate all      | 0.067s |    0.020s | 3.43x |
| Range query (1%) | 0.001s |    0.000s | 2.52x |
| Random delete    | 0.746s |    0.708s | 1.05x |

### 10,000,000 keys

| Operation        |     ART |  BTreeMap | Ratio |
|------------------|---------|-----------|-------|
| Random put       | 11.722s |   18.691s | 0.63x |
| Sequential put   |  3.442s |    3.564s | 0.97x |
| Random get (hit) |  9.394s |   15.289s | 0.61x |
| Random get (miss)|  0.105s |    1.563s | 0.07x |
| Iterate all      |  0.584s |    0.283s | 2.06x |
| Range query (1%) |  0.007s |    0.003s | 2.36x |
| Random delete    | 12.627s |   15.249s | 0.83x |

Ratio below 1.0 means the ART is faster.

**Where ART wins.** Point operations—put, get, delete—scale better
because lookup is O(key length) regardless of tree size, while
BTreeMap's O(key length * log n) comparison cost grows with n.  The
advantage widens from ~20% at 100K to ~37% at 10M.  Miss lookups are
the most dramatic: ART rejects misses at the first non-matching prefix
byte, making them nearly free (0.07x at 10M).

**Where BTreeMap wins.** Sequential iteration.  BTreeMap nodes are
contiguous arrays of dozens of keys, so iterating them is a sequential
memory scan that the hardware prefetcher loves.  ART iteration chases
pointers across scattered heap allocations—each `Leaf` and inner node
is a separate `Box`.  The 2x gap is the cost of pointer-chasing vs.
cache-line scanning.

### Performance tuning: lazy iterators

The iteration numbers above reflect the optimized lazy iterator.  The
original recursive `iter_all` function collected every entry into a
`Vec`, and called `inner_children` (which itself allocates a `Vec`) at
each inner node.  Those allocations added measurable overhead at scale.

Replacing the recursive collect with the stack-based `Iter` and
`RangeIter` (see section 8) cut iteration costs significantly at 10
million keys:

| Operation        | Before |  After | Improvement |
|------------------|--------|--------|-------------|
| Iterate all      |  2.69x |  2.06x |  23% faster |
| Range query (1%) |  2.51x |  2.36x |   6% faster |

The iterate-all improvement comes from eliminating the O(n) output
`Vec` and the per-node temporary `Vec` from `inner_children`.  Range
queries benefit less because their cost is dominated by boundary
analysis at each level rather than allocation overhead.

### Performance tuning: eliminating Vec from nodes

The original implementation used `Vec<u8>` for three fields inside
every node: the path-compression prefix in each inner node, the key in
each leaf, and the key in each inner node value.  Each `Vec<u8>` costs
24 bytes (pointer + length + capacity) plus a separate heap allocation
for the data—even for a 3-byte prefix like `"key"`.

We eliminated all `Vec<u8>` from the node types in three changes:

1. **Leaf key and inner-value key: `Vec<u8>` → `Box<[u8]>`.**  Keys
   are write-once, so the capacity field is wasted.  `Box<[u8]>` is
   16 bytes instead of 24—saves 8 bytes per leaf.

2. **Prefix: `Vec<u8>` → inline `Prefix` enum.**  Short prefixes
   (≤ 22 bytes) are stored directly in the node struct with zero heap
   allocation.  The enum is the same 24 bytes as the `Vec` it replaces,
   so node struct sizes don't change—but the prefix bytes are now in
   the same cache line as the node's other fields, eliminating one
   pointer dereference per tree level during traversal.

After these changes, the benchmark at 10 million keys:

| Operation        |     ART |  BTreeMap | Ratio |
|------------------|---------|-----------|-------|
| Random put       | 15.505s |   23.963s | 0.65x |
| Sequential put   |  3.667s |    4.548s | 0.81x |
| Random get (hit) | 11.034s |   18.473s | 0.60x |
| Random get (miss)|  0.121s |    1.804s | 0.07x |
| Iterate all      |  0.605s |    0.324s | 1.87x |
| Range query (1%) |  0.011s |    0.003s | 3.66x |
| Random delete    | 15.665s |   18.552s | 0.84x |

Iteration improved from 2.06x to 1.87x—the inline prefix means fewer
cache misses when visiting each inner node.

### Realistic key workloads

The benchmarks above use a synthetic `key{i:012}` format—15-byte keys
with a fixed 3-byte shared prefix.  That's a worst-case workload for
ART's structural advantages: short keys mean BTreeMap's per-comparison
cost is cheap, and short shared prefixes mean path compression barely
helps.

Real workloads tend to have longer keys with longer shared prefixes—
URL paths, file paths, log-line keys.  Running the same operations at
1 million keys across four key distributions shows the trend clearly
(`cargo run --release --example bench_realistic`):

| Workload       | Avg len | Put   | Get (hit) | Get (miss) | Iterate | Delete |
|----------------|---------|-------|-----------|------------|---------|--------|
| `key{:012}`    | 15 B    | 0.76x | 0.71x     | 0.05x      | 2.00x   | 1.05x  |
| URL paths      | 38 B    | 0.85x | 0.69x     | 0.09x      | 2.23x   | 1.10x  |
| File paths     | 54 B    | 0.62x | 0.68x     | 0.10x      | 2.23x   | 1.10x  |
| Log lines      | 49 B    | 0.73x | 0.68x     | 0.09x      | 2.05x   | 0.70x  |

Ratios are ART time / BTreeMap time; below 1.0 means ART is faster.

As keys get longer, ART's advantage on point operations widens.  File
paths show ART **38% faster on inserts** (0.62x) and **32% faster on
lookups** (0.68x), because BTreeMap's O(key_length * log n) comparison
cost grows with both key length and tree size, while ART walks the
trie at O(key_length) regardless of n.

Miss lookups stay around 0.09x–0.11x across all realistic workloads:
ART rejects a miss at the first non-matching byte, which happens near
the root for almost every miss, no matter how long the full key is.

Iteration stays BTreeMap's territory (~2x faster) regardless of key
length.  The inline prefix helps with iteration (section above) but
can't overcome the fundamental disadvantage of pointer-chasing through
scattered heap allocations versus scanning contiguous node arrays.

### The fundamental trade-off

This is the fundamental architectural trade-off: tries give O(key
length) point operations by eliminating key comparisons, but pay for
it with pointer-heavy layouts that hurt sequential access.  B-trees
give cache-friendly sequential access by packing keys into contiguous
nodes, but pay O(log n) comparisons per lookup.
