# ART by Operation

This is a second literate walkthrough of the same implementation described in
[ART.md](ART.md).  That document follows the data structures top-down; this one
follows the **operations**—what happens when you read, write, remove, or scan.

All code below is copied from `src/lib.rs` (the crate root next to this file).
If the implementation changes, refresh these excerpts to match.

---

## Get: one walk, three ideas

`ARTMap::get` is a thin `unsafe` wrapper around `get_inner`.  The loop carries
`depth` (how many bytes of the query key are already accounted for), compares each
inner node’s path-compressed **prefix**, handles **prefix keys** stored on inner
nodes when the query ends mid-route, then uses **`inner_find`** to follow the
next branch byte.

### Public entry and full walk

```rust
impl<V> ARTMap<V> {
    pub fn get(&self, key: &[u8]) -> Option<&V> {
        unsafe { self.get_inner(key) }
    }

    unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
        let mut node = self.root;
        let mut depth = 0;

        while !node.is_null() {
            if node.is_leaf() {
                // Raw deref: returned refs must outlive the copied `NodePtr` local, not `as_leaf(&node)`.
                let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
                if *leaf.key == *key {
                    return Some(&leaf.value);
                }
                return None;
            }

            // Inner node: check prefix
            let prefix = inner_prefix_raw(node);
            let plen = prefix.len();
            if key.len() < depth + plen || key[depth..depth + plen] != *prefix {
                return None;
            }
            depth += plen;

            if depth == key.len() {
                // Key exhausted at this inner node
                return inner_value_raw(node).map(|(_, v)| v);
            }

            let b = key[depth];
            node = inner_find(node, b);
            depth += 1;
        }
        None
    }
}
```

### Prefix bytes and prefix-key payload on inner nodes

```rust
unsafe fn inner_prefix_raw<'a, V>(node: NodePtr<V>) -> &'a [u8] {
    let ptr = node.inner_ptr();
    match node.kind() {
        KIND_NODE4 => (*(ptr as *const Node4<V>)).prefix.as_slice(),
        KIND_NODE16 => (*(ptr as *const Node16<V>)).prefix.as_slice(),
        KIND_NODE48 => (*(ptr as *const Node48<V>)).prefix.as_slice(),
        KIND_NODE256 => (*(ptr as *const Node256<V>)).prefix.as_slice(),
        _ => unreachable!(),
    }
}

unsafe fn inner_value_raw<'a, V>(node: NodePtr<V>) -> Option<(&'a [u8], &'a V)> {
    let ptr = node.inner_ptr();
    let opt: &Option<(Box<[u8]>, V)> = match node.kind() {
        KIND_NODE4 => &(*(ptr as *const Node4<V>)).value,
        KIND_NODE16 => &(*(ptr as *const Node16<V>)).value,
        KIND_NODE48 => &(*(ptr as *const Node48<V>)).value,
        KIND_NODE256 => &(*(ptr as *const Node256<V>)).value,
        _ => unreachable!(),
    };
    opt.as_ref().map(|(k, v)| (&**k, v))
}
```

### Get at a leaf

A leaf stores the **full** key.  The walk only gets you to a candidate; you must
compare `leaf.key == query`.  Returning `&V` uses a raw pointer derived from the
tag-stripped bits so the borrow is not tied to the local `NodePtr` copy.

```rust
struct Leaf<V> {
    key: Box<[u8]>,
    value: V,
}
```

The leaf case is the first arm of the `while` in `get_inner`.  Signature and
body match `src/lib.rs` (the crate merges this with `get` in one `impl` block):

```rust
impl<V> ARTMap<V> {
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
                return inner_value_raw(node).map(|(_, v)| v);
            }

            let b = key[depth];
            node = inner_find(node, b);
            depth += 1;
        }
        None
    }
}
```

### Get at Node4, Node16, Node48, Node256 (`inner_find`)

All four shapes share one dispatch.  **Node4:** linear scan up to four keys.
**Node16:** `binary_search` on the used prefix of `keys`.  **Node48:** `index[b]`
then `slots`.  **Node256:** direct `children[b]` (possibly null).

```rust
fn inner_find<V>(node: NodePtr<V>, b: u8) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            for i in 0..n.count as usize {
                if n.keys[i] == b {
                    return n.children[i];
                }
            }
            NodePtr::NULL
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            let cnt = n.count as usize;
            match n.keys[..cnt].binary_search(&b) {
                Ok(i) => n.children[i],
                Err(_) => NodePtr::NULL,
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48();
            let idx = n.index[b as usize];
            if idx == 0xFF {
                NodePtr::NULL
            } else {
                n.slots[idx as usize]
            }
        }
        KIND_NODE256 => {
            let n = node.as_node256();
            n.children[b as usize]
        }
        _ => unreachable!(),
    }
}
```

---

## Put: splits, prefix keys, growth

### Public API

```rust
impl<V> ARTMap<V> {
    pub fn put(&mut self, key: &[u8], value: V) {
        let (new_root, added, _) = put_recursive(self.root, key, value, 0);
        self.root = new_root;
        if added {
            self.len += 1;
        }
    }
}
```

### Shared helper: first differing byte

Used when splitting a leaf against an incoming key, and when comparing an inner
prefix to the key.

```rust
fn prefix_mismatch(a: &[u8], a_off: usize, b: &[u8], b_off: usize) -> usize {
    let n = (a.len() - a_off).min(b.len() - b_off);
    for i in 0..n {
        if a[a_off + i] != b[b_off + i] {
            return i;
        }
    }
    n
}
```

### `put_recursive` (entire function)

```rust
fn put_recursive<V>(node: NodePtr<V>, key: &[u8], value: V, depth: usize) -> (NodePtr<V>, bool, V) {
    // Empty slot -> new leaf
    if node.is_null() {
        let leaf = Box::new(Leaf { key: Box::from(key), value });
        return (NodePtr::from_leaf(leaf), true, unsafe { std::mem::zeroed() });
    }

    // Leaf
    if node.is_leaf() {
        let existing = node.as_leaf();
        if *existing.key == *key {
            // Update existing leaf
            let mut leaf_box = node.into_leaf_box();
            let old_value = std::mem::replace(&mut leaf_box.value, value);
            return (NodePtr::from_leaf(leaf_box), false, old_value);
        }

        // Mismatch: create Node4 to hold both
        let ekb = &existing.key;
        let common = prefix_mismatch(key, depth, ekb, depth);
        let sd = depth + common; // split depth

        let mut nn = Box::new(Node4::<V>::new());
        nn.prefix = Prefix::from_slice(&key[depth..sd]);

        let mut nn_ptr = NodePtr::from_node4(nn);

        if sd == key.len() {
            // New key is prefix of existing
            inner_set_value(&mut nn_ptr, Box::from(key), value);
            inner_add_child(&mut nn_ptr, ekb[sd], node);
        } else if sd == ekb.len() {
            // Existing key is prefix of new key
            let existing_box = node.into_leaf_box();
            inner_set_value(&mut nn_ptr, existing_box.key, existing_box.value);
            let new_leaf = Box::new(Leaf { key: Box::from(key), value });
            inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
        } else {
            let new_leaf = Box::new(Leaf { key: Box::from(key), value });
            let new_b = key[sd];
            let old_b = ekb[sd];
            inner_add_child(&mut nn_ptr, new_b, NodePtr::from_leaf(new_leaf));
            inner_add_child(&mut nn_ptr, old_b, node);
        }
        return (nn_ptr, true, unsafe { std::mem::zeroed() });
    }

    // Inner node
    let prefix = unsafe { inner_prefix_raw(node) }.to_vec();
    let plen = prefix.len();
    let ml = prefix_mismatch(key, depth, &prefix, 0);

    if ml < plen {
        // Partial prefix match -> split this node
        let mut nn = Box::new(Node4::<V>::new());
        nn.prefix = Prefix::from_slice(&prefix[..ml]);
        let mut nn_ptr = NodePtr::from_node4(nn);

        let mut old_node = node;
        inner_set_prefix(&mut old_node, Prefix::from_slice(&prefix[ml + 1..]));
        inner_add_child(&mut nn_ptr, prefix[ml], old_node);

        let nd = depth + ml;
        if nd == key.len() {
            inner_set_value(&mut nn_ptr, Box::from(key), value);
        } else {
            let new_leaf = Box::new(Leaf { key: Box::from(key), value });
            inner_add_child(&mut nn_ptr, key[nd], NodePtr::from_leaf(new_leaf));
        }
        return (nn_ptr, true, unsafe { std::mem::zeroed() });
    }

    // Full prefix match
    let nd = depth + plen;
    let mut node = node;

    if nd == key.len() {
        let added = !inner_has_value(&node);
        inner_set_value(&mut node, Box::from(key), value);
        return (node, added, unsafe { std::mem::zeroed() });
    }

    let b = key[nd];
    let child = inner_find(node, b);

    if child.is_null() {
        if inner_is_full(&node) {
            node = grow(node);
        }
        let new_leaf = Box::new(Leaf { key: Box::from(key), value });
        inner_add_child(&mut node, b, NodePtr::from_leaf(new_leaf));
        return (node, true, unsafe { std::mem::zeroed() });
    }

    let (new_child, added, old_v) = put_recursive(child, key, value, nd + 1);
    if new_child.0 != child.0 {
        inner_replace_child(&mut node, b, new_child);
    }
    (node, added, old_v)
}
```

### Mutations `put` relies on

```rust
fn inner_set_value<V>(node: &mut NodePtr<V>, key: Box<[u8]>, value: V) {
    let val = Some((key, value));
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().value = val,
        KIND_NODE16 => node.as_node16_mut().value = val,
        KIND_NODE48 => node.as_node48_mut().value = val,
        KIND_NODE256 => node.as_node256_mut().value = val,
        _ => unreachable!(),
    }
}

fn inner_set_prefix<V>(node: &mut NodePtr<V>, prefix: Prefix) {
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().prefix = prefix,
        KIND_NODE16 => node.as_node16_mut().prefix = prefix,
        KIND_NODE48 => node.as_node48_mut().prefix = prefix,
        KIND_NODE256 => node.as_node256_mut().prefix = prefix,
        _ => unreachable!(),
    }
}

fn inner_has_value<V>(node: &NodePtr<V>) -> bool {
    match node.kind() {
        KIND_NODE4 => node.as_node4().value.is_some(),
        KIND_NODE16 => node.as_node16().value.is_some(),
        KIND_NODE48 => node.as_node48().value.is_some(),
        KIND_NODE256 => node.as_node256().value.is_some(),
        _ => unreachable!(),
    }
}

fn inner_is_full<V>(node: &NodePtr<V>) -> bool {
    match node.kind() {
        KIND_NODE4 => node.as_node4().count >= 4,
        KIND_NODE16 => node.as_node16().count >= 16,
        KIND_NODE48 => node.as_node48().count >= 48,
        KIND_NODE256 => false,
        _ => unreachable!(),
    }
}

fn inner_add_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    match node.kind() {
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
        KIND_NODE16 => {
            let n = node.as_node16_mut();
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
        KIND_NODE48 => {
            let n = node.as_node48_mut();
            let slot = (0u8..48).find(|&j| n.slots[j as usize].is_null()).unwrap();
            n.index[b as usize] = slot;
            n.slots[slot as usize] = child;
            n.count += 1;
        }
        KIND_NODE256 => {
            let n = node.as_node256_mut();
            n.children[b as usize] = child;
            n.count += 1;
        }
        _ => unreachable!(),
    }
}

fn inner_replace_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4_mut();
            for i in 0..n.count as usize {
                if n.keys[i] == b {
                    n.children[i] = child;
                    return;
                }
            }
        }
        KIND_NODE16 => {
            let n = node.as_node16_mut();
            let cnt = n.count as usize;
            if let Ok(i) = n.keys[..cnt].binary_search(&b) {
                n.children[i] = child;
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48_mut();
            let idx = n.index[b as usize];
            n.slots[idx as usize] = child;
        }
        KIND_NODE256 => {
            let n = node.as_node256_mut();
            n.children[b as usize] = child;
        }
        _ => unreachable!(),
    }
}
```

### Node growth (4 → 16 → 48 → 256)

```rust
fn inner_move_header<V>(src: &mut NodePtr<V>, dst: &mut NodePtr<V>) {
    let prefix = inner_take_prefix(src);
    let value = inner_clear_value(src);
    inner_set_prefix(dst, prefix);
    match dst.kind() {
        KIND_NODE4 => dst.as_node4_mut().value = value,
        KIND_NODE16 => dst.as_node16_mut().value = value,
        KIND_NODE48 => dst.as_node48_mut().value = value,
        KIND_NODE256 => dst.as_node256_mut().value = value,
        _ => unreachable!(),
    }
}

fn grow<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => {
            let mut new_ptr = NodePtr::from_node16(Box::new(Node16::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node4();
            let cnt = old.count as usize;
            {
                let dst = new_ptr.as_node16_mut();
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.count = cnt as u8;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE16 => {
            let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node16();
            let cnt = old.count as usize;
            {
                let dst = new_ptr.as_node48_mut();
                for i in 0..cnt {
                    let b = old.keys[i];
                    dst.index[b as usize] = i as u8;
                    dst.slots[i] = old.children[i];
                }
                dst.count = cnt as u8;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE48 => {
            let mut new_ptr = NodePtr::from_node256(Box::new(Node256::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node48();
            {
                let dst = new_ptr.as_node256_mut();
                let mut cnt = 0u16;
                for b in 0..256usize {
                    let idx = old.index[b];
                    if idx != 0xFF {
                        dst.children[b] = old.slots[idx as usize];
                        cnt += 1;
                    }
                }
                dst.count = cnt;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        _ => unreachable!("Node256 cannot grow"),
    }
}
```

`grow` ends each arm with `free_inner_node_shell` (defined under **Delete** below)
so the old node’s `Box` is dropped without recursively dropping children that were
moved to the new node.

Headers move during `grow` / `shrink` without double-dropping children:

```rust
type InnerValue<V> = Option<(Box<[u8]>, V)>;

fn inner_clear_value<V>(node: &mut NodePtr<V>) -> InnerValue<V> {
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().value.take(),
        KIND_NODE16 => node.as_node16_mut().value.take(),
        KIND_NODE48 => node.as_node48_mut().value.take(),
        KIND_NODE256 => node.as_node256_mut().value.take(),
        _ => unreachable!(),
    }
}

fn inner_take_prefix<V>(node: &mut NodePtr<V>) -> Prefix {
    match node.kind() {
        KIND_NODE4 => std::mem::take(&mut node.as_node4_mut().prefix),
        KIND_NODE16 => std::mem::take(&mut node.as_node16_mut().prefix),
        KIND_NODE48 => std::mem::take(&mut node.as_node48_mut().prefix),
        KIND_NODE256 => std::mem::take(&mut node.as_node256_mut().prefix),
        _ => unreachable!(),
    }
}
```

### Shrink (256 → 48 → 16 → 4), mirror of `grow`

```rust
fn shrink<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE256 => {
            let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node256();
            {
                let dst = new_ptr.as_node48_mut();
                let mut slot = 0u8;
                for b in 0..256usize {
                    if !old.children[b].is_null() {
                        dst.index[b] = slot;
                        dst.slots[slot as usize] = old.children[b];
                        slot += 1;
                    }
                }
                dst.count = slot;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE48 => {
            let mut new_ptr = NodePtr::from_node16(Box::new(Node16::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node48();
            {
                let dst = new_ptr.as_node16_mut();
                let mut cnt = 0usize;
                for b in 0..256usize {
                    let idx = old.index[b];
                    if idx != 0xFF {
                        dst.keys[cnt] = b as u8;
                        dst.children[cnt] = old.slots[idx as usize];
                        cnt += 1;
                    }
                }
                dst.count = cnt as u8;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE16 => {
            let mut new_ptr = NodePtr::from_node4(Box::new(Node4::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node16();
            let cnt = old.count as usize;
            {
                let dst = new_ptr.as_node4_mut();
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.count = cnt as u8;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        _ => node,
    }
}
```

---

## Delete: recurse, strip, compact

### Public API

```rust
impl<V> ARTMap<V> {
    pub fn delete(&mut self, key: &[u8]) -> bool {
        let (new_root, deleted) = delete_recursive(self.root, key, 0);
        self.root = new_root;
        if deleted {
            self.len -= 1;
        }
        deleted
    }
}
```

### `delete_recursive`

```rust
fn delete_recursive<V>(node: NodePtr<V>, key: &[u8], depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (NodePtr::NULL, false);
    }

    if node.is_leaf() {
        if *node.as_leaf().key == *key {
            drop(node.into_leaf_box());
            return (NodePtr::NULL, true);
        }
        return (node, false);
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

### Remove child edge

```rust
fn inner_remove_child<V>(node: &mut NodePtr<V>, b: u8) {
    match node.kind() {
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
        KIND_NODE16 => {
            let n = node.as_node16_mut();
            let cnt = n.count as usize;
            if let Ok(pos) = n.keys[..cnt].binary_search(&b) {
                for i in pos..cnt - 1 {
                    n.keys[i] = n.keys[i + 1];
                    n.children[i] = n.children[i + 1];
                }
                n.children[cnt - 1] = NodePtr::NULL;
                n.count -= 1;
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48_mut();
            let idx = n.index[b as usize];
            if idx != 0xFF {
                n.slots[idx as usize] = NodePtr::NULL;
                n.index[b as usize] = 0xFF;
                n.count -= 1;
            }
        }
        KIND_NODE256 => {
            let n = node.as_node256_mut();
            if !n.children[b as usize].is_null() {
                n.children[b as usize] = NodePtr::NULL;
                n.count -= 1;
            }
        }
        _ => unreachable!(),
    }
}
```

### `compact` and freeing the inner shell

```rust
fn inner_count<V>(node: &NodePtr<V>) -> usize {
    match node.kind() {
        KIND_NODE4 => node.as_node4().count as usize,
        KIND_NODE16 => node.as_node16().count as usize,
        KIND_NODE48 => node.as_node48().count as usize,
        KIND_NODE256 => node.as_node256().count as usize,
        _ => unreachable!(),
    }
}

fn inner_children<V>(node: &NodePtr<V>) -> Vec<(u8, NodePtr<V>)> {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            let cnt = n.count as usize;
            (0..cnt).map(|i| (n.keys[i], n.children[i])).collect()
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            let cnt = n.count as usize;
            (0..cnt).map(|i| (n.keys[i], n.children[i])).collect()
        }
        KIND_NODE48 => {
            let n = node.as_node48();
            let mut out = Vec::new();
            for b in 0..256usize {
                let idx = n.index[b];
                if idx != 0xFF {
                    out.push((b as u8, n.slots[idx as usize]));
                }
            }
            out
        }
        KIND_NODE256 => {
            let n = node.as_node256();
            let mut out = Vec::new();
            for b in 0..256usize {
                if !n.children[b].is_null() {
                    out.push((b as u8, n.children[b]));
                }
            }
            out
        }
        _ => unreachable!(),
    }
}

fn compact<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    let count = inner_count(&node);

    if count == 0 {
        if inner_has_value(&node) {
            let val = inner_clear_value(&mut node);
            free_inner_node_shell(node);
            let (k, v) = val.unwrap();
            return NodePtr::from_leaf(Box::new(Leaf { key: k, value: v }));
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

fn free_inner_node_shell<V>(node: NodePtr<V>) {
    match node.kind() {
        KIND_NODE4 => {
            let mut b = node.into_node4_box();
            b.count = 0;
            b.value = None;
            drop(b);
        }
        KIND_NODE16 => {
            let mut b = node.into_node16_box();
            b.count = 0;
            b.value = None;
            drop(b);
        }
        KIND_NODE48 => {
            let mut b = node.into_node48_box();
            b.count = 0;
            b.value = None;
            b.index = [0xFF; 256];
            drop(b);
        }
        KIND_NODE256 => {
            let mut b = node.into_node256_box();
            b.count = 0;
            b.value = None;
            for c in b.children.iter_mut() {
                *c = NodePtr::NULL;
            }
            drop(b);
        }
        _ => unreachable!(),
    }
}
```

---

## Full scan iteration (`iter`)

Lazy DFS via an explicit stack; children are pushed high-to-low so the smallest
byte is popped first.

```rust
impl<V> ARTMap<V> {
    pub fn iter(&self) -> Iter<'_, V> {
        let mut stack = Vec::new();
        if !self.root.is_null() {
            stack.push(self.root);
        }
        Iter { stack, _marker: std::marker::PhantomData }
    }
}

pub struct Iter<'a, V> {
    stack: Vec<NodePtr<V>>,
    _marker: std::marker::PhantomData<&'a V>,
}

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
                return Some((k, v));
            }
        }
    }
}

fn push_children_rev<V>(node: NodePtr<V>, stack: &mut Vec<NodePtr<V>>) {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            for i in (0..n.count as usize).rev() {
                stack.push(n.children[i]);
            }
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            for i in (0..n.count as usize).rev() {
                stack.push(n.children[i]);
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48();
            for b in (0..256usize).rev() {
                if n.index[b] != 0xFF {
                    stack.push(n.slots[n.index[b] as usize]);
                }
            }
        }
        KIND_NODE256 => {
            let n = node.as_node256();
            for b in (0..256usize).rev() {
                if !n.children[b].is_null() {
                    stack.push(n.children[b]);
                }
            }
        }
        _ => unreachable!(),
    }
}
```

---

## Range query (`range_iter`)

### Construction and frame type

```rust
impl<V> ARTMap<V> {
    pub fn range_iter<'a>(
        &'a self,
        lo: Option<&'a [u8]>,
        hi: Option<&'a [u8]>,
    ) -> RangeIter<'a, V> {
        let mut stack = Vec::new();
        if !self.root.is_null() {
            stack.push(RangeFrame { node: self.root, depth: 0, lo, hi });
        }
        RangeIter { stack, _marker: std::marker::PhantomData }
    }
}

struct RangeFrame<'a, V> {
    node: NodePtr<V>,
    depth: usize,
    lo: Option<&'a [u8]>,
    hi: Option<&'a [u8]>,
}

pub struct RangeIter<'a, V> {
    stack: Vec<RangeFrame<'a, V>>,
    _marker: std::marker::PhantomData<&'a V>,
}
```

### Prefix-key in bounds (shared with pruning branches)

```rust
unsafe fn inner_value_in_lex_range<'a, V>(
    node: NodePtr<V>,
    lo: Option<&'a [u8]>,
    hi: Option<&'a [u8]>,
) -> Option<(&'a [u8], &'a V)> {
    let (kb, v) = inner_value_raw(node)?;
    if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
        Some((kb, v))
    } else {
        None
    }
}
```

### `RangeIter::next` and pruned child push

```rust
impl<'a, V> Iterator for RangeIter<'a, V> {
    type Item = (&'a [u8], &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.pop()?;
            let node = frame.node;
            let depth = frame.depth;
            let lo = frame.lo;
            let hi = frame.hi;

            if node.is_leaf() {
                let leaf = unsafe { &*((node.0 & !TAG_MASK) as *const Leaf<V>) };
                let kb = &leaf.key[..];
                if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
                    return Some((kb, &leaf.value));
                }
                continue;
            }

            let p = unsafe { inner_prefix_raw(node) };
            let plen = p.len();
            let nd = depth + plen;

            let mut lo = lo;
            let mut lo_on = false;
            if let Some(lo_bytes) = lo {
                let lo_avail = lo_bytes.len().saturating_sub(depth);
                if lo_avail == 0 {
                    lo = None;
                } else if plen == 0 {
                    lo_on = true;
                } else {
                    let cn = plen.min(lo_avail);
                    let pp = &p[..cn];
                    let lp = &lo_bytes[depth..depth + cn];
                    if pp < lp { continue; }
                    if pp > lp { lo = None; }
                    else if cn < plen { lo = None; }
                    else if lo_avail > plen { lo_on = true; }
                    else { lo = None; }
                }
            }

            let mut hi = hi;
            let mut hi_on = false;
            if let Some(hi_bytes) = hi {
                let hi_avail = hi_bytes.len().saturating_sub(depth);
                if hi_avail == 0 {
                    if let Some((kb, v)) = unsafe { inner_value_in_lex_range(node, lo, Some(hi_bytes)) }
                    {
                        return Some((kb, v));
                    }
                    continue;
                } else if plen == 0 {
                    hi_on = true;
                } else {
                    let cn = plen.min(hi_avail);
                    let pp = &p[..cn];
                    let hp = &hi_bytes[depth..depth + cn];
                    if pp > hp { continue; }
                    if pp < hp { hi = None; }
                    else if cn < plen { continue; }
                    else if hi_avail > plen { hi_on = true; }
                    else {
                        if let Some((kb, v)) =
                            unsafe { inner_value_in_lex_range(node, lo, Some(hi_bytes)) }
                        {
                            return Some((kb, v));
                        }
                        continue;
                    }
                }
            }

            let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
            let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

            push_range_children_rev(
                node, nd + 1, lo_byte, hi_byte,
                lo_on, lo, hi_on, hi, &mut self.stack,
            );

            if let Some((kb, v)) = unsafe { inner_value_in_lex_range(node, lo, hi) } {
                return Some((kb, v));
            }
        }
    }
}

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
        if b < lo_byte || b > hi_byte { return; }
        let child_lo = if lo_on && b == lo_byte { lo } else { None };
        let child_hi = if hi_on && b == hi_byte { hi } else { None };
        stack.push(RangeFrame { node: child, depth: child_depth, lo: child_lo, hi: child_hi });
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

---

## Speculations for future development

- **API polish:** expose `put`’s old value (already computed internally) and add
  entry APIs akin to `BTreeMap::entry` for read-modify-write workloads.
- **Borrowed keys on insert:** today inserts allocate `Box<[u8]>` for leaves and
  prefix keys; an owned-key / `Cow` API could cut allocations for callers that
  already hold an owned buffer.
- **Deterministic shrinking policy:** shrink thresholds are fixed; workloads
  that oscillate around a boundary might benefit from hysteresis or “shrink only
  when below half capacity” rules to avoid flip-flopping shapes.
- **SIMD / vectorized child search:** Node16’s binary search is already fine, but
  very wide nodes under artificial benchmarks sometimes benefit from explicit
  SIMD scans; real-world sparse nodes rarely need it.
- **Concurrency:** a lock-free ART or reader-writer variant is a large project;
  the current tagged-pointer layout assumes single-threaded mutation.
- **Secondary indexes:** prefix iterators (`starts_with`) are a thin layer atop
  `range_iter`; exposing them explicitly would match common string-key uses.
- **Memory accounting:** optional counters for bytes in prefixes, leaves, and
  node shells would help compare against `BTreeMap` beyond wall-clock time.

If you extend the code, keep the story straight: **get** explains navigation,
**put** explains splits and growth, **delete** explains compaction and shrink,
**iter** explains ordered traversal without recursion, and **range** explains how
bounds flow down the trie without visiting the whole map.
