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
