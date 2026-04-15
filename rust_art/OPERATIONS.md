# ART by Operation

This is the companion to [ART.md](ART.md). That document explains the data
structures from the bottom up. This one explains the implementation the way a
reader usually meets it in practice: **what happens when you call an
operation**.

The code now lives across `src/map.rs`, `src/inner.rs`, `src/iter.rs`,
`src/raw.rs`, and `src/prefix.rs`. The excerpts below are copied from those
modules and arranged as a literate walkthrough of four operations:

1. `get`
2. `put`
3. `delete`
4. range scan

The recurring idea is simple:

- the tree is navigated one byte at a time,
- path compression stores skipped bytes as an inline prefix on inner nodes,
- a key that is also a prefix of longer keys stores its value on the inner node,
- node shapes adapt as fanout grows and shrinks.

---

## The public entry points

All four operations start in `ARTMap<V>`:

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

The public methods are deliberately thin. The interesting logic lives in the
recursive walkers and the inner-node helpers.

---

## 1. Get

Lookup is the cleanest operation, so it is the best place to learn the tree.

The walk carries two pieces of state:

- `node`: where we are in the tree,
- `depth`: how many bytes of the key have already been consumed.

```rust
unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
    let mut node = self.root;
    let mut depth = 0;

    while !node.is_null() {
        if node.is_leaf() {
            let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
            if *leaf.key == *key {
                return Some(&leaf.value);
            }
            return None;
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

### Step 1: if we hit a leaf, verify the whole key

ART navigation only gets us to a **candidate**. Because of path compression, the
traversal knows that the bytes seen so far match, but it does not by itself
prove full-key equality. So the leaf stores the whole key:

```rust
pub(crate) struct Leaf<V> {
    pub(crate) key: Box<[u8]>,
    pub(crate) value: V,
}
```

That is why the leaf case does a final `leaf.key == key` check.

### Step 2: if we hit an inner node, compare the compressed prefix

Each inner node stores a prefix shared by every key in its subtree. Lookup must
consume that prefix before it looks for the next branch byte:

```rust
pub(crate) unsafe fn inner_prefix_raw<'a, V>(node: NodePtr<V>) -> &'a [u8] {
    let header = &*(node.inner_ptr() as *const crate::raw::NodeHeader<V>);
    header.prefix.as_slice()
}
```

If the query ends *exactly* at that inner node, the value may live there instead
of in a leaf. That is how the tree represents keys like `ab` and `abc`
simultaneously:

```rust
pub(crate) unsafe fn inner_value_raw<'a, V>(node: NodePtr<V>) -> Option<(&'a [u8], &'a V)> {
    let header = &*(node.inner_ptr() as *const crate::raw::NodeHeader<V>);
    let opt = &header.value;
    opt.as_ref().map(|(key, value)| (&**key, value))
}
```

### Step 3: choose the child for the next byte

All node shapes share the same logical operation — “find the child for byte
`b`” — but each shape implements it differently:

```rust
pub(crate) fn inner_find<V>(node: NodePtr<V>, b: u8) -> NodePtr<V> {
    dispatch!(node, find_child, b)
}
```

That is the whole lookup algorithm: validate prefix, maybe return the inner-node
value, otherwise follow one byte to the next node.

---

## 2. Put

Insertion is where the ART earns its keep. It must handle:

- adding into an empty slot,
- overwriting an existing leaf,
- splitting a leaf into a branching inner node,
- splitting an inner node when a compressed prefix only partially matches,
- storing values on inner nodes for prefix keys,
- growing a node when it runs out of room.

The key helper is “how far do these two byte slices agree?”:

```rust
pub(crate) fn prefix_mismatch(a: &[u8], a_off: usize, b: &[u8], b_off: usize) -> usize {
    let n = (a.len() - a_off).min(b.len() - b_off);
    for i in 0..n {
        if a[a_off + i] != b[b_off + i] {
            return i;
        }
    }
    n
}
```

Before the recursive insert, the implementation names a tiny helper:

```rust
fn leaf_ptr<V>(key: &[u8], value: V) -> NodePtr<V> {
    NodePtr::from_leaf(Box::new(Leaf {
        key: Box::from(key),
        value,
    }))
}
```

That helper matters because insertion creates leaves in several branches. Giving
that action a name makes the recursive logic read in terms of tree structure
instead of allocation boilerplate.

### The insertion walk

```rust
fn put_recursive<V>(node: NodePtr<V>, key: &[u8], value: V, depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (Leaf::new_ptr(key, value), true);
    }

    if node.is_leaf() {
        return Leaf::put(node, key, value, depth);
    }

    let prefix = unsafe { inner_prefix_raw(node) }.to_vec();
    let plen = prefix.len();
    let ml = prefix_mismatch(key, depth, &prefix, 0);

    if ml < plen {
        let mut nn = Box::new(Node4::<V>::new());
        nn.header.prefix = Prefix::from_slice(&prefix[..ml]);
        let mut nn_ptr = NodePtr::from_node4(nn);

        let mut old_node = node;
        inner_set_prefix(&mut old_node, Prefix::from_slice(&prefix[ml + 1..]));
        inner_add_child(&mut nn_ptr, prefix[ml], old_node);

        let nd = depth + ml;
        if nd == key.len() {
            inner_set_value(&mut nn_ptr, Box::from(key), value);
        } else {
            inner_add_child(&mut nn_ptr, key[nd], Leaf::new_ptr(key, value));
        }
        return (nn_ptr, true);
    }

    let nd = depth + plen;
    let mut node = node;

    if nd == key.len() {
        let added = !inner_has_value(&node);
        inner_set_value(&mut node, Box::from(key), value);
        return (node, added);
    }

    let b = key[nd];
    let child = inner_find(node, b);

    if child.is_null() {
        if inner_is_full(&node) {
            node = grow(node);
        }
        inner_add_child(&mut node, b, Leaf::new_ptr(key, value));
        return (node, true);
    }

    let (new_child, added) = put_recursive(child, key, value, nd + 1);
    if new_child.0 != child.0 {
        inner_replace_child(&mut node, b, new_child);
    }
    (node, added)
}
```

### Case A: empty slot -> leaf

This is the base case. No subtree exists, so insertion allocates a new leaf and
returns it.

### Case B: leaf -> overwrite or split

If the keys are identical, insertion is just an overwrite.

One simplification worth calling out: `put_recursive` now returns only
`(new_node, added)`. The earlier version carried an extra `V` back up the stack
even though `ARTMap::put` never used it. Removing that dead value made the code
match the real question insertion is answering: **did the subtree root change,
and did the map grow?**

If they differ, one leaf is no longer enough. The code allocates a `Node4`,
stores the common bytes as its prefix, and then handles one of three subcases:

1. the **new key** is a prefix of the old key,
2. the **old key** is a prefix of the new key,
3. neither key is a prefix; they simply branch at the split byte.

That is the first important design move in the implementation: **branch points
become inner nodes, and prefix keys live on those nodes**.

### Case C: inner node -> prefix split or descend

If the incoming key only partially matches the node’s compressed prefix, the
node itself must be split. The existing node keeps the unmatched suffix of the
old prefix; a fresh `Node4` becomes the new parent and holds the common prefix.

If the full prefix matches, insertion either:

- stores a value on the current node (when the key ends here),
- adds a new child leaf (when no child exists for the next byte),
- or recurses into the existing child.

### Growing nodes

When an inner node is full, insertion promotes it to the next shape before
adding the new child:

```rust
pub(crate) fn grow<V>(node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => Node4::grow(node),
        KIND_NODE16 => Node16::grow(node),
        KIND_NODE48 => Node48::grow(node),
        _ => unreachable!("Node256 cannot grow"),
    }
}
```

The header move is important: growth keeps the **prefix** and the optional
**inner-node value** intact while only changing the child representation.

---

## 3. Delete

Deletion mirrors insertion in the forward direction, then does extra work on
the way back up to keep the tree compact.

```rust
fn delete_recursive<V>(node: NodePtr<V>, key: &[u8], depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (NodePtr::NULL, false);
    }

    if node.is_leaf() {
        return Leaf::delete(node, key);
    }

    let prefix = unsafe { inner_prefix_raw(node) }.to_vec();
    let plen = prefix.len();
    if key.len() < depth + plen || key[depth..depth + plen] != prefix[..] {
        return (node, false);
    }

    let nd = depth + plen;
    let mut node = node;

    if nd == key.len() {
        if !inner_has_value(&node) {
            return (node, false);
        }
        inner_clear_value(&mut node);
        return (compact(node), true);
    }

    let b = key[nd];
    let child = inner_find(node, b);
    if child.is_null() {
        return (node, false);
    }

    let (new_child, deleted) = delete_recursive(child, key, nd + 1);
    if !deleted {
        return (node, false);
    }

    if new_child.is_null() {
        inner_remove_child(&mut node, b);
    } else {
        inner_replace_child(&mut node, b, new_child);
    }

    (compact(node), true)
}
```

### The forward walk

The forward half is straightforward:

- `NULL` means “not found,”
- a leaf is deleted only on exact key match,
- an inner node must match its compressed prefix before we descend,
- if the key ends exactly at an inner node, deletion clears the stored prefix
  value instead of removing a child.

### Removing a child

Once a recursive delete succeeds, the parent either removes the child slot or
replaces it with a compacted child pointer:

```rust
pub(crate) fn inner_remove_child<V>(node: &mut NodePtr<V>, b: u8) {
    dispatch_mut!(node, remove_child, b)
}
```

### Compaction

The crucial cleanup step is `compact`:

```rust
pub(crate) fn compact<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    let count = inner_count(&node);

    if count == 0 {
        if inner_has_value(&node) {
            let val = inner_clear_value(&mut node);
            free_inner_node_shell(node);
            let (key, value) = val.unwrap();
            return NodePtr::from_leaf(Box::new(Leaf { key, value }));
        }
        free_inner_node_shell(node);
        return NodePtr::NULL;
    }

    if count == 1 && !inner_has_value(&node) {
        let children = inner_children(&node);
        let (b, child) = children[0];
        if child.is_leaf() {
            free_inner_node_shell(node);
            return child;
        }
        let parent_prefix = unsafe { inner_prefix_raw(node) }.to_vec();
        free_inner_node_shell(node);
        let mut child = child;
        let child_prefix = inner_take_prefix(&mut child);
        let mut merged = parent_prefix;
        merged.push(b);
        merged.extend_from_slice(child_prefix.as_slice());
        inner_set_prefix(&mut child, Prefix::from_slice(&merged));
        return child;
    }

    let should_shrink = match node.kind() {
        KIND_NODE256 => count <= 48,
        KIND_NODE48 => count <= 16,
        KIND_NODE16 => count <= 4,
        _ => false,
    };
    if should_shrink {
        return shrink(node);
    }

    node
}
```

`compact` handles three structural repairs:

1. **no children left**  
   If the node still has its own value, it becomes a leaf. Otherwise it becomes
   null.

2. **exactly one child and no own value**  
   The node is redundant. If the child is a leaf, we return it directly. If the
   child is another inner node, we merge:

   `parent.prefix + branch_byte + child.prefix`

3. **too sparse for its current shape**  
   The node shrinks to the next smaller representation.

Shrinkage is the inverse of growth:

```rust
pub(crate) fn shrink<V>(node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE256 => Node256::shrink(node),
        KIND_NODE48 => Node48::shrink(node),
        KIND_NODE16 => Node16::shrink(node),
        _ => node,
    }
}
```

Insertion expands the tree just enough; deletion compresses it again.

---

## 4. Range scan

The range scan is the most subtle operation. A full in-order walk is easy; the
challenge is to skip entire subtrees that are known to be outside the bounds.

The public API has two forms:

- `range(lo, hi)`, which collects into a `Vec`,
- `range_iter(lo, hi)`, which yields lazily.

The iterator is stack-based:

```rust
pub(crate) struct RangeFrame<'a, V> {
    node: NodePtr<V>,
    depth: usize,
    lo: Option<&'a [u8]>,
    hi: Option<&'a [u8]>,
}

pub struct RangeIter<'a, V> {
    stack: Vec<RangeFrame<'a, V>>,
    _marker: std::marker::PhantomData<&'a V>,
}

impl<'a, V> RangeIter<'a, V> {
    pub(crate) fn new(root: NodePtr<V>, lo: Option<&'a [u8]>, hi: Option<&'a [u8]>) -> Self {
        let mut stack = Vec::new();
        if !root.is_null() {
            stack.push(RangeFrame {
                node: root,
                depth: 0,
                lo,
                hi,
            });
        }
        RangeIter {
            stack,
            _marker: std::marker::PhantomData,
        }
    }
}
```

Each stack frame says:

- which node to visit,
- how many key bytes are already fixed on the path above it,
- whether the subtree is still constrained by the lower and upper bounds.

The hard part of range scan is not the stack. It is the boundary bookkeeping:
after comparing a subtree prefix with `lo` or `hi`, the iterator needs to know
whether that subtree is:

- impossible and should be pruned,
- fully inside the range and now unconstrained,
- still exactly on one bound,
- or, for the upper bound, only allowed to yield the node’s own value.

Instead of encoding that as ad-hoc booleans, the implementation names those
states:

```rust
enum LowerBound<'a> {
    Prune,
    Free,
    Follow(&'a [u8]),
}

enum UpperBound<'a> {
    Prune,
    Free,
    Follow(&'a [u8]),
    OwnOnly(&'a [u8]),
}
```

### The core loop

```rust
impl<'a, V> Iterator for RangeIter<'a, V> {
    type Item = (&'a [u8], &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.pop()?;
            let node = frame.node;
            let depth = frame.depth;
            let mut lo = frame.lo;
            let mut hi = frame.hi;

            if node.is_leaf() {
                let leaf = unsafe { &*((node.0 & !TAG_MASK) as *const Leaf<V>) };
                let kb = &leaf.key[..];
                if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
                    return Some((kb, &leaf.value));
                }
                continue;
            }

            let prefix = unsafe { inner_prefix_raw(node) };
            let plen = prefix.len();
            let nd = depth + plen;

            let lo_on = match lower_bound(prefix, depth, lo) {
                LowerBound::Prune => continue,
                LowerBound::Free => {
                    lo = None;
                    false
                }
                LowerBound::Follow(lo_bytes) => {
                    lo = Some(lo_bytes);
                    true
                }
            };

            let hi_on = match upper_bound(prefix, depth, hi) {
                UpperBound::Prune => continue,
                UpperBound::Free => {
                    hi = None;
                    false
                }
                UpperBound::Follow(hi_bytes) => {
                    hi = Some(hi_bytes);
                    true
                }
                UpperBound::OwnOnly(hi_bytes) => {
                    if let Some((kb, v)) =
                        unsafe { inner_value_in_lex_range(node, lo, Some(hi_bytes)) }
                    {
                        return Some((kb, v));
                    }
                    continue;
                }
            };

            let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
            let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

            push_range_children_rev(
                node,
                nd + 1,
                lo_byte,
                hi_byte,
                lo_on,
                lo,
                hi_on,
                hi,
                &mut self.stack,
            );

            if let Some((kb, v)) = unsafe { inner_value_in_lex_range(node, lo, hi) } {
                return Some((kb, v));
            }
        }
    }
}
```

### What the boundary logic is doing

The best way to read the range scan now is: **compare the node prefix to each
bound, get back a named state, then act on that state**.

The two helper functions are:

```rust
fn lower_bound<'a>(prefix: &[u8], depth: usize, lo: Option<&'a [u8]>) -> LowerBound<'a> {
    let Some(lo_bytes) = lo else {
        return LowerBound::Free;
    };
    let lo_avail = lo_bytes.len().saturating_sub(depth);
    if lo_avail == 0 {
        return LowerBound::Free;
    }
    if prefix.is_empty() {
        return LowerBound::Follow(lo_bytes);
    }

    let cn = prefix.len().min(lo_avail);
    let pp = &prefix[..cn];
    let lp = &lo_bytes[depth..depth + cn];
    if pp < lp {
        LowerBound::Prune
    } else if pp > lp || cn < prefix.len() || lo_avail == prefix.len() {
        LowerBound::Free
    } else {
        LowerBound::Follow(lo_bytes)
    }
}

fn upper_bound<'a>(prefix: &[u8], depth: usize, hi: Option<&'a [u8]>) -> UpperBound<'a> {
    let Some(hi_bytes) = hi else {
        return UpperBound::Free;
    };
    let hi_avail = hi_bytes.len().saturating_sub(depth);
    if hi_avail == 0 {
        return UpperBound::OwnOnly(hi_bytes);
    }
    if prefix.is_empty() {
        return UpperBound::Follow(hi_bytes);
    }

    let cn = prefix.len().min(hi_avail);
    let pp = &prefix[..cn];
    let hp = &hi_bytes[depth..depth + cn];
    if pp > hp || cn < prefix.len() {
        UpperBound::Prune
    } else if pp < hp {
        UpperBound::Free
    } else if hi_avail == prefix.len() {
        UpperBound::OwnOnly(hi_bytes)
    } else {
        UpperBound::Follow(hi_bytes)
    }
}
```

At an inner node, the iterator compares the node’s compressed prefix with the
remaining bytes of `lo` and `hi`.

That comparison yields one of three outcomes for each bound:

1. **the entire subtree is outside the bound**  
   abort this frame immediately.

2. **the subtree is strictly inside the bound**  
   drop that bound for the subtree (`lo = None` or `hi = None`).

3. **the subtree is still exactly on the bound edge**  
   keep carrying that bound downward.

The upper bound has one extra outcome:

4. **only the node’s own value can still match**  
   yield the inner-node value if it is in range, but do not descend to children.

Once the prefix is handled, only child bytes inside the admissible range are
pushed.

### Pushing only the children that can still match

```rust
fn push_range_children_rev<'a, V>(
    node: NodePtr<V>,
    child_depth: usize,
    lo_byte: i16,
    hi_byte: i16,
    lo_on: bool,
    lo: Option<&'a [u8]>,
    hi_on: bool,
    hi: Option<&'a [u8]>,
    stack: &mut Vec<RangeFrame<'a, V>>,
) {
    let mut push = |byte: u8, child: NodePtr<V>| {
        let b = byte as i16;
        if b < lo_byte || b > hi_byte {
            return;
        }
        let child_lo = if lo_on && b == lo_byte { lo } else { None };
        let child_hi = if hi_on && b == hi_byte { hi } else { None };
        stack.push(RangeFrame {
            node: child,
            depth: child_depth,
            lo: child_lo,
            hi: child_hi,
        });
    };

    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            for i in (0..n.count as usize).rev() {
                push(n.keys[i], n.children[i]);
            }
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            for i in (0..n.count as usize).rev() {
                push(n.keys[i], n.children[i]);
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48();
            for b in (0..256usize).rev() {
                if n.index[b] != 0xFF {
                    push(b as u8, n.slots[n.index[b] as usize]);
                }
            }
        }
        KIND_NODE256 => {
            let n = node.as_node256();
            for b in (0..256usize).rev() {
                if !n.children[b].is_null() {
                    push(b as u8, n.children[b]);
                }
            }
        }
        _ => unreachable!(),
    }
}
```

The reverse push order is deliberate. Because the traversal pops from a stack,
reverse push means **smallest key comes out first**, so the iterator yields
sorted results.

### Inner-node values participate in ranges too

A prefix key stored on an inner node must also obey the bounds:

```rust
pub(crate) unsafe fn inner_value_in_lex_range<'a, V>(
    node: NodePtr<V>,
    lo: Option<&'a [u8]>,
    hi: Option<&'a [u8]>,
) -> Option<(&'a [u8], &'a V)> {
    let (key, value) = inner_value_raw(node)?;
    if lo.map_or(true, |lo| key >= lo) && hi.map_or(true, |hi| key <= hi) {
        Some((key, value))
    } else {
        None
    }
}
```

That detail matters for ranges such as `[ab, abd]`, where `ab` itself is a key
and must be yielded before `abc` and `abd`.

---

## The shape of the whole design

Seen operation by operation, the implementation reduces to four habits:

1. **walk by bytes**
2. **consume compressed prefixes eagerly**
3. **store prefix keys on inner nodes**
4. **repair shape after mutation**

`get` is the minimal read-only walk. `put` introduces splitting and growth.
`delete` introduces compaction and shrinkage. Range scan adds the last piece:
**prune entire subtrees as soon as a bound proves they cannot contribute**.

That is the whole ART in operational form.
