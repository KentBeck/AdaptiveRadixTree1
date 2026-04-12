/// Adaptive Radix Tree (ART) — an ordered key-value map.
///
/// Uses tagged pointers to distinguish leaf vs inner node types,
/// and adaptive node sizes (4, 16, 48, 256) for memory efficiency.
/// Path compression collapses single-child chains into node prefixes.
///
/// Keys are `Vec<u8>`, values are generic `V`.

// ---------------------------------------------------------------------------
// Tagged pointer
// ---------------------------------------------------------------------------
// Bit 0 of the pointer distinguishes leaves (tag=1) from inner nodes (tag=0).
// Inner node pointers have bits [1..3] encoding the node kind:
//   0b000 = Node4, 0b010 = Node16, 0b100 = Node48, 0b110 = Node256

const TAG_LEAF: usize = 1;
const TAG_MASK: usize = 0b111; // low 3 bits
const KIND_NODE4: usize = 0b000;
const KIND_NODE16: usize = 0b010;
const KIND_NODE48: usize = 0b100;
const KIND_NODE256: usize = 0b110;

/// A tagged pointer that is either null, a Leaf, or one of four inner node types.
/// Manual Copy/Clone so we don't require V: Copy.
struct NodePtr<V>(usize, std::marker::PhantomData<V>);

impl<V> Clone for NodePtr<V> {
    fn clone(&self) -> Self {
        NodePtr(self.0, std::marker::PhantomData)
    }
}
impl<V> Copy for NodePtr<V> {}

impl<V> NodePtr<V> {
    const NULL: Self = NodePtr(0, std::marker::PhantomData);

    fn is_null(self) -> bool {
        self.0 == 0
    }

    fn is_leaf(self) -> bool {
        !self.is_null() && (self.0 & TAG_LEAF) != 0
    }

    // -- constructors --

    fn from_leaf(leaf: Box<Leaf<V>>) -> Self {
        let raw = Box::into_raw(leaf) as usize;
        debug_assert!(raw & TAG_MASK == 0, "leaf pointer not aligned");
        NodePtr(raw | TAG_LEAF, std::marker::PhantomData)
    }

    fn from_node4(node: Box<Node4<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE4, std::marker::PhantomData)
    }

    fn from_node16(node: Box<Node16<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE16, std::marker::PhantomData)
    }

    fn from_node48(node: Box<Node48<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE48, std::marker::PhantomData)
    }

    fn from_node256(node: Box<Node256<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE256, std::marker::PhantomData)
    }

    // -- accessors --

    fn as_leaf(&self) -> &Leaf<V> {
        debug_assert!(self.is_leaf());
        unsafe { &*((self.0 & !TAG_MASK) as *const Leaf<V>) }
    }

    fn as_leaf_mut(&mut self) -> &mut Leaf<V> {
        debug_assert!(self.is_leaf());
        unsafe { &mut *((self.0 & !TAG_MASK) as *mut Leaf<V>) }
    }

    fn into_leaf_box(self) -> Box<Leaf<V>> {
        debug_assert!(self.is_leaf());
        unsafe { Box::from_raw((self.0 & !TAG_MASK) as *mut Leaf<V>) }
    }

    fn kind(&self) -> usize {
        debug_assert!(!self.is_null() && !self.is_leaf());
        self.0 & TAG_MASK
    }

    fn inner_ptr(&self) -> *mut u8 {
        (self.0 & !TAG_MASK) as *mut u8
    }

    fn as_node4(&self) -> &Node4<V> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { &*(self.inner_ptr() as *const Node4<V>) }
    }
    fn as_node4_mut(&mut self) -> &mut Node4<V> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { &mut *(self.inner_ptr() as *mut Node4<V>) }
    }
    fn into_node4_box(self) -> Box<Node4<V>> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node4<V>) }
    }

    fn as_node16(&self) -> &Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &*(self.inner_ptr() as *const Node16<V>) }
    }
    fn as_node16_mut(&mut self) -> &mut Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &mut *(self.inner_ptr() as *mut Node16<V>) }
    }
    fn into_node16_box(self) -> Box<Node16<V>> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node16<V>) }
    }

    fn as_node48(&self) -> &Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &*(self.inner_ptr() as *const Node48<V>) }
    }
    fn as_node48_mut(&mut self) -> &mut Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &mut *(self.inner_ptr() as *mut Node48<V>) }
    }
    fn into_node48_box(self) -> Box<Node48<V>> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node48<V>) }
    }

    fn as_node256(&self) -> &Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &*(self.inner_ptr() as *const Node256<V>) }
    }
    fn as_node256_mut(&mut self) -> &mut Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &mut *(self.inner_ptr() as *mut Node256<V>) }
    }
    fn into_node256_box(self) -> Box<Node256<V>> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node256<V>) }
    }

    /// Free the memory behind this pointer (recursively for inner nodes).
    unsafe fn drop_recursive(self) {
        if self.is_null() {
            return;
        }
        if self.is_leaf() {
            drop(self.into_leaf_box());
            return;
        }
        match self.kind() {
            KIND_NODE4 => {
                let mut b = self.into_node4_box();
                for i in 0..b.count as usize {
                    b.children[i].drop_recursive();
                }
                drop(b);
            }
            KIND_NODE16 => {
                let mut b = self.into_node16_box();
                for i in 0..b.count as usize {
                    b.children[i].drop_recursive();
                }
                drop(b);
            }
            KIND_NODE48 => {
                let mut b = self.into_node48_box();
                for i in 0..256usize {
                    let idx = b.index[i];
                    if idx != 0xFF {
                        b.slots[idx as usize].drop_recursive();
                    }
                }
                drop(b);
            }
            KIND_NODE256 => {
                let mut b = self.into_node256_box();
                for i in 0..256usize {
                    if !b.children[i].is_null() {
                        b.children[i].drop_recursive();
                    }
                }
                drop(b);
            }
            _ => unreachable!(),
        }
    }
}

// ---------------------------------------------------------------------------
// Leaf
// ---------------------------------------------------------------------------

struct Leaf<V> {
    key: Vec<u8>,
    value: V,
}

// ---------------------------------------------------------------------------
// Inner node header (common fields)
// ---------------------------------------------------------------------------
// Each inner node stores: prefix, optional value (for prefix-key storage),
// and children keyed by a single byte.

/// Optional value on an inner node (when a key is a prefix of longer keys).
type InnerValue<V> = Option<(Vec<u8>, V)>; // (full_key, value)

// ---------------------------------------------------------------------------
// Node4
// ---------------------------------------------------------------------------

struct Node4<V> {
    prefix: Vec<u8>,
    value: InnerValue<V>,
    count: u8,
    keys: [u8; 4],
    children: [NodePtr<V>; 4],
}

impl<V> Node4<V> {
    fn new() -> Self {
        Node4 {
            prefix: Vec::new(),
            value: None,
            count: 0,
            keys: [0; 4],
            children: [NodePtr::NULL; 4],
        }
    }
}

// ---------------------------------------------------------------------------
// Node16
// ---------------------------------------------------------------------------

struct Node16<V> {
    prefix: Vec<u8>,
    value: InnerValue<V>,
    count: u8,
    keys: [u8; 16],
    children: [NodePtr<V>; 16],
}

impl<V> Node16<V> {
    fn new() -> Self {
        Node16 {
            prefix: Vec::new(),
            value: None,
            count: 0,
            keys: [0; 16],
            children: [NodePtr::NULL; 16],
        }
    }
}

// ---------------------------------------------------------------------------
// Node48
// ---------------------------------------------------------------------------

struct Node48<V> {
    prefix: Vec<u8>,
    value: InnerValue<V>,
    count: u8,
    index: [u8; 256],       // byte -> slot index (0xFF = empty)
    slots: [NodePtr<V>; 48],
}

impl<V> Node48<V> {
    fn new() -> Self {
        Node48 {
            prefix: Vec::new(),
            value: None,
            count: 0,
            index: [0xFF; 256],
            slots: [NodePtr::NULL; 48],
        }
    }
}

// ---------------------------------------------------------------------------
// Node256
// ---------------------------------------------------------------------------

struct Node256<V> {
    prefix: Vec<u8>,
    value: InnerValue<V>,
    count: u16,
    children: [NodePtr<V>; 256],
}

impl<V> Node256<V> {
    fn new() -> Self {
        Node256 {
            prefix: Vec::new(),
            value: None,
            count: 0,
            children: [NodePtr::NULL; 256],
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct ARTMap<V> {
    root: NodePtr<V>,
    len: usize,
}

impl<V> ARTMap<V> {
    pub fn new() -> Self {
        ARTMap {
            root: NodePtr::NULL,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn put(&mut self, key: &[u8], value: V) {
        let (new_root, added, _) = put_recursive(self.root, key, value, 0);
        self.root = new_root;
        if added {
            self.len += 1;
        }
    }

    pub fn delete(&mut self, key: &[u8]) -> bool {
        let (new_root, deleted) = delete_recursive(self.root, key, 0);
        self.root = new_root;
        if deleted {
            self.len -= 1;
        }
        deleted
    }

    pub fn get(&self, key: &[u8]) -> Option<&V> {
        // Safety: all NodePtrs stored in the tree point to valid heap allocations
        // that live as long as &self. We use unsafe to tie the returned reference
        // lifetime to &self rather than to a local stack copy of the pointer.
        unsafe { self.get_inner(key) }
    }

    unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
        let mut node = self.root;
        let mut depth = 0;

        while !node.is_null() {
            if node.is_leaf() {
                let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
                if leaf.key == key {
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

    /// Iterate all (key, value) pairs in sorted key order.
    pub fn items(&self) -> Vec<(&[u8], &V)> {
        let mut result = Vec::new();
        unsafe { iter_all(self.root, &mut result) };
        result
    }

    /// Iterate (key, value) pairs in sorted order within [from_key, to_key].
    /// Pass None for either bound to leave it unconstrained.
    pub fn range(&self, from_key: Option<&[u8]>, to_key: Option<&[u8]>) -> Vec<(&[u8], &V)> {
        let mut result = Vec::new();
        unsafe { iter_range(self.root, 0, from_key, to_key, &mut result) };
        result
    }
}

/// Recursively collect all (key, value) pairs in sorted order.
unsafe fn iter_all<'a, V>(node: NodePtr<V>, out: &mut Vec<(&'a [u8], &'a V)>) {
    if node.is_null() {
        return;
    }
    if node.is_leaf() {
        let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
        out.push((&leaf.key, &leaf.value));
        return;
    }
    // Inner node: own value first (shorter key sorts before children)
    if let Some((k, v)) = inner_value_raw(node) {
        out.push((k.as_slice(), v));
    }
    for (_, child) in inner_children(&node) {
        iter_all(child, out);
    }
}

/// O(log n + k) range iteration. Tracks boundary paths to prune branches.
///
/// At every inner node:
/// 1. Compares prefix with remaining bound bytes to prune or relax constraints
/// 2. Uses next bound byte to skip children before lo or after hi
/// 3. Passes bound only to children on the exact boundary byte
unsafe fn iter_range<'a, V>(
    node: NodePtr<V>,
    depth: usize,
    lo: Option<&[u8]>,
    hi: Option<&[u8]>,
    out: &mut Vec<(&'a [u8], &'a V)>,
) {
    if node.is_null() {
        return;
    }

    if node.is_leaf() {
        let leaf = &*((node.0 & !TAG_MASK) as *const Leaf<V>);
        let kb = &leaf.key[..];
        if let Some(lo) = lo {
            if kb < lo { return; }
        }
        if let Some(hi) = hi {
            if kb > hi { return; }
        }
        out.push((kb, &leaf.value));
        return;
    }

    // Inner node
    let p = inner_prefix_raw(node);
    let plen = p.len();
    let nd = depth + plen; // depth after consuming prefix

    // -- lo boundary analysis --
    let mut lo = lo;
    let mut lo_on = false;
    if let Some(lo_bytes) = lo {
        let lo_avail = if lo_bytes.len() > depth { lo_bytes.len() - depth } else { 0 };
        if lo_avail == 0 {
            lo = None; // lo already consumed, everything here >= lo
        } else if plen == 0 {
            lo_on = true; // decide at child level
        } else {
            let cn = plen.min(lo_avail);
            let pp = &p[..cn];
            let lp = &lo_bytes[depth..depth + cn];
            if pp < lp {
                return; // whole subtree < lo
            }
            if pp > lp {
                lo = None; // past lo, no lower constraint
            } else if cn < plen {
                lo = None; // lo exhausted inside prefix
            } else if lo_avail > plen {
                lo_on = true; // lo has more bytes, check children
            } else {
                lo = None; // lo exhausted exactly at nd
            }
        }
    }

    // -- hi boundary analysis --
    let mut hi = hi;
    let mut hi_on = false;
    if let Some(hi_bytes) = hi {
        let hi_avail = if hi_bytes.len() > depth { hi_bytes.len() - depth } else { 0 };
        if hi_avail == 0 {
            // hi exhausted: only this node's own value could match
            if let Some((k, v)) = inner_value_raw(node) {
                let kb = k.as_slice();
                if (lo.is_none() || kb >= lo.unwrap()) && kb <= hi_bytes {
                    out.push((kb, v));
                }
            }
            return;
        } else if plen == 0 {
            hi_on = true;
        } else {
            let cn = plen.min(hi_avail);
            let pp = &p[..cn];
            let hp = &hi_bytes[depth..depth + cn];
            if pp > hp {
                return; // whole subtree > hi
            }
            if pp < hp {
                hi = None; // before hi, no upper constraint
            } else if cn < plen {
                return; // hi exhausted inside prefix, keys > hi
            } else if hi_avail > plen {
                hi_on = true;
            } else {
                // hi exhausted at nd; own value may equal hi, children > hi
                if let Some((k, v)) = inner_value_raw(node) {
                    let kb = k.as_slice();
                    if (lo.is_none() || kb >= lo.unwrap()) && kb <= hi_bytes {
                        out.push((kb, v));
                    }
                }
                return;
            }
        }
    }

    // -- yield own value --
    if let Some((k, v)) = inner_value_raw(node) {
        let kb = k.as_slice();
        let lo_ok = lo.is_none() || kb >= lo.unwrap();
        let hi_ok = hi.is_none() || kb <= hi.unwrap();
        if lo_ok && hi_ok {
            out.push((kb, v));
        }
    }

    // -- visit children with byte-level pruning --
    let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
    let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

    for (byte, child) in inner_children(&node) {
        let b = byte as i16;
        if b < lo_byte { continue; }
        if b > hi_byte { return; }
        let child_lo = if lo_on && b == lo_byte { lo } else { None };
        let child_hi = if hi_on && b == hi_byte { hi } else { None };
        iter_range(child, nd + 1, child_lo, child_hi, out);
    }
}

// ---------------------------------------------------------------------------
// Inner node field accessors (dispatch on kind)
// ---------------------------------------------------------------------------

/// Get prefix from inner node. Caller must ensure node is a valid inner node pointer.
unsafe fn inner_prefix_raw<'a, V>(node: NodePtr<V>) -> &'a [u8] {
    let ptr = node.inner_ptr();
    match node.kind() {
        KIND_NODE4 => &(*(ptr as *const Node4<V>)).prefix,
        KIND_NODE16 => &(*(ptr as *const Node16<V>)).prefix,
        KIND_NODE48 => &(*(ptr as *const Node48<V>)).prefix,
        KIND_NODE256 => &(*(ptr as *const Node256<V>)).prefix,
        _ => unreachable!(),
    }
}

/// Get value stored on inner node. Caller must ensure node is valid.
unsafe fn inner_value_raw<'a, V>(node: NodePtr<V>) -> Option<(&'a Vec<u8>, &'a V)> {
    let ptr = node.inner_ptr();
    let opt: &Option<(Vec<u8>, V)> = match node.kind() {
        KIND_NODE4 => &(*(ptr as *const Node4<V>)).value,
        KIND_NODE16 => &(*(ptr as *const Node16<V>)).value,
        KIND_NODE48 => &(*(ptr as *const Node48<V>)).value,
        KIND_NODE256 => &(*(ptr as *const Node256<V>)).value,
        _ => unreachable!(),
    };
    opt.as_ref().map(|(k, v)| (k, v))
}

/// Find child by byte key in inner node. Returns NULL if not found.
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

// ---------------------------------------------------------------------------
// Mutation helpers for inner nodes
// ---------------------------------------------------------------------------

fn prefix_mismatch(a: &[u8], a_off: usize, b: &[u8], b_off: usize) -> usize {
    let n = (a.len() - a_off).min(b.len() - b_off);
    for i in 0..n {
        if a[a_off + i] != b[b_off + i] {
            return i;
        }
    }
    n
}

/// Add a child to an inner node. Caller must ensure node is not full.
fn inner_add_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4_mut();
            let cnt = n.count as usize;
            // Insert sorted
            let pos = n.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
            // Shift right
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
            // Find a free slot
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

/// Replace a child in an inner node.
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

fn inner_is_full<V>(node: &NodePtr<V>) -> bool {
    match node.kind() {
        KIND_NODE4 => node.as_node4().count >= 4,
        KIND_NODE16 => node.as_node16().count >= 16,
        KIND_NODE48 => node.as_node48().count >= 48,
        KIND_NODE256 => false,
        _ => unreachable!(),
    }
}

fn inner_count<V>(node: &NodePtr<V>) -> usize {
    match node.kind() {
        KIND_NODE4 => node.as_node4().count as usize,
        KIND_NODE16 => node.as_node16().count as usize,
        KIND_NODE48 => node.as_node48().count as usize,
        KIND_NODE256 => node.as_node256().count as usize,
        _ => unreachable!(),
    }
}

fn inner_set_prefix<V>(node: &mut NodePtr<V>, prefix: Vec<u8>) {
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().prefix = prefix,
        KIND_NODE16 => node.as_node16_mut().prefix = prefix,
        KIND_NODE48 => node.as_node48_mut().prefix = prefix,
        KIND_NODE256 => node.as_node256_mut().prefix = prefix,
        _ => unreachable!(),
    }
}

fn inner_set_value<V>(node: &mut NodePtr<V>, key: Vec<u8>, value: V) {
    let val = Some((key, value));
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().value = val,
        KIND_NODE16 => node.as_node16_mut().value = val,
        KIND_NODE48 => node.as_node48_mut().value = val,
        KIND_NODE256 => node.as_node256_mut().value = val,
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

/// Move header (prefix + value) from src to dst. Src's prefix/value become empty/None.
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

// ---------------------------------------------------------------------------
// Node growth
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Node shrinkage
// ---------------------------------------------------------------------------

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
        _ => node, // Node4 cannot shrink further
    }
}

// ---------------------------------------------------------------------------
// Remove child from inner node
// ---------------------------------------------------------------------------

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

/// Return (byte, child) pairs in sorted byte order.
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

fn inner_clear_value<V>(node: &mut NodePtr<V>) -> InnerValue<V> {
    match node.kind() {
        KIND_NODE4 => node.as_node4_mut().value.take(),
        KIND_NODE16 => node.as_node16_mut().value.take(),
        KIND_NODE48 => node.as_node48_mut().value.take(),
        KIND_NODE256 => node.as_node256_mut().value.take(),
        _ => unreachable!(),
    }
}

/// Get prefix, consuming it (replacing with empty vec).
fn inner_take_prefix<V>(node: &mut NodePtr<V>) -> Vec<u8> {
    match node.kind() {
        KIND_NODE4 => std::mem::take(&mut node.as_node4_mut().prefix),
        KIND_NODE16 => std::mem::take(&mut node.as_node16_mut().prefix),
        KIND_NODE48 => std::mem::take(&mut node.as_node48_mut().prefix),
        KIND_NODE256 => std::mem::take(&mut node.as_node256_mut().prefix),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Compact after deletion
// ---------------------------------------------------------------------------

/// Collapse a node that became degenerate after a child removal.
fn compact<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    let count = inner_count(&node);

    if count == 0 {
        if inner_has_value(&node) {
            // Convert to leaf
            let val = inner_clear_value(&mut node);
            // Free the inner node
            free_inner_node_shell(node);
            let (k, v) = val.unwrap();
            return NodePtr::from_leaf(Box::new(Leaf { key: k, value: v }));
        }
        // Totally empty — free and return null
        free_inner_node_shell(node);
        return NodePtr::NULL;
    }

    if count == 1 && !inner_has_value(&node) {
        let children = inner_children(&node);
        let (b, child) = children[0];
        if child.is_leaf() {
            // Single leaf child, no value: collapse to just the leaf
            free_inner_node_shell(node);
            return child;
        }
        // Single inner child: merge prefixes: parent.prefix + byte + child.prefix
        let parent_prefix = unsafe { inner_prefix_raw(node) }.to_vec();
        free_inner_node_shell(node);
        let mut child = child;
        let child_prefix = inner_take_prefix(&mut child);
        let mut merged = parent_prefix;
        merged.push(b);
        merged.extend_from_slice(&child_prefix);
        inner_set_prefix(&mut child, merged);
        return child;
    }

    // Check shrink thresholds
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

/// Free an inner node's Box without recursively dropping children.
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

// ---------------------------------------------------------------------------
// Recursive delete
// ---------------------------------------------------------------------------

/// Returns (new_node_or_null, was_deleted)
fn delete_recursive<V>(node: NodePtr<V>, key: &[u8], depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (NodePtr::NULL, false);
    }

    if node.is_leaf() {
        if node.as_leaf().key == key {
            // Free the leaf
            drop(node.into_leaf_box());
            return (NodePtr::NULL, true);
        }
        return (node, false);
    }

    // Inner node
    let prefix = unsafe { inner_prefix_raw(node) }.to_vec();
    let plen = prefix.len();
    if key.len() < depth + plen || key[depth..depth + plen] != prefix[..] {
        return (node, false);
    }

    let nd = depth + plen;
    let mut node = node;

    if nd == key.len() {
        // Deleting the value stored on this inner node
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

// ---------------------------------------------------------------------------
// Recursive put
// ---------------------------------------------------------------------------

/// Returns (new_node, was_new_key)
fn put_recursive<V>(node: NodePtr<V>, key: &[u8], value: V, depth: usize) -> (NodePtr<V>, bool, V) {
    // Empty slot -> new leaf
    if node.is_null() {
        let leaf = Box::new(Leaf { key: key.to_vec(), value });
        return (NodePtr::from_leaf(leaf), true, unsafe { std::mem::zeroed() });
    }

    // Leaf
    if node.is_leaf() {
        let existing = node.as_leaf();
        if existing.key == key {
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
        nn.prefix = key[depth..sd].to_vec();

        let mut nn_ptr = NodePtr::from_node4(nn);

        if sd == key.len() {
            // New key is prefix of existing
            inner_set_value(&mut nn_ptr, key.to_vec(), value);
            inner_add_child(&mut nn_ptr, ekb[sd], node);
        } else if sd == ekb.len() {
            // Existing key is prefix of new key
            let ekb_clone = ekb.to_vec();
            let existing_box = node.into_leaf_box();
            inner_set_value(&mut nn_ptr, ekb_clone, existing_box.value);
            let new_leaf = Box::new(Leaf { key: key.to_vec(), value });
            inner_add_child(&mut nn_ptr, key[sd], NodePtr::from_leaf(new_leaf));
        } else {
            let new_leaf = Box::new(Leaf { key: key.to_vec(), value });
            // Add in sorted order
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
        nn.prefix = prefix[..ml].to_vec();
        let mut nn_ptr = NodePtr::from_node4(nn);

        // Update old node's prefix to suffix after split point
        let new_prefix = prefix[ml + 1..].to_vec();
        let mut old_node = node;
        inner_set_prefix(&mut old_node, new_prefix);
        inner_add_child(&mut nn_ptr, prefix[ml], old_node);

        let nd = depth + ml;
        if nd == key.len() {
            inner_set_value(&mut nn_ptr, key.to_vec(), value);
        } else {
            let new_leaf = Box::new(Leaf { key: key.to_vec(), value });
            inner_add_child(&mut nn_ptr, key[nd], NodePtr::from_leaf(new_leaf));
        }
        return (nn_ptr, true, unsafe { std::mem::zeroed() });
    }

    // Full prefix match
    let nd = depth + plen;
    let mut node = node;

    if nd == key.len() {
        // Key exhausted at this inner node - store value here
        let added = !inner_has_value(&node);
        inner_set_value(&mut node, key.to_vec(), value);
        return (node, added, unsafe { std::mem::zeroed() });
    }

    let b = key[nd];
    let child = inner_find(node, b);

    if child.is_null() {
        // No child for this byte - add new leaf
        if inner_is_full(&node) {
            node = grow(node);
        }
        let new_leaf = Box::new(Leaf { key: key.to_vec(), value });
        inner_add_child(&mut node, b, NodePtr::from_leaf(new_leaf));
        return (node, true, unsafe { std::mem::zeroed() });
    }

    let (new_child, added, old_v) = put_recursive(child, key, value, nd + 1);
    if new_child.0 != child.0 {
        inner_replace_child(&mut node, b, new_child);
    }
    (node, added, old_v)
}

impl<V> Drop for ARTMap<V> {
    fn drop(&mut self) {
        unsafe {
            self.root.drop_recursive();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_get_returns_none() {
        let tree: ARTMap<i32> = ARTMap::new();
        assert!(tree.get(b"anything").is_none());
    }

    #[test]
    fn empty_len_is_zero() {
        let tree: ARTMap<i32> = ARTMap::new();
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }

    #[test]
    fn put_and_get_single() {
        let mut tree = ARTMap::new();
        tree.put(b"hello", 42);
        assert_eq!(tree.get(b"hello"), Some(&42));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn put_overwrite() {
        let mut tree = ARTMap::new();
        tree.put(b"k", 1);
        tree.put(b"k", 2);
        assert_eq!(tree.get(b"k"), Some(&2));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn get_missing() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        assert!(tree.get(b"b").is_none());
        assert!(tree.get(b"ab").is_none());
    }

    #[test]
    fn multiple_independent_keys() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        tree.put(b"c", 3);
        assert_eq!(tree.get(b"a"), Some(&1));
        assert_eq!(tree.get(b"b"), Some(&2));
        assert_eq!(tree.get(b"c"), Some(&3));
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn shared_prefix() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 1);
        tree.put(b"abd", 2);
        tree.put(b"xyz", 3);
        assert_eq!(tree.get(b"abc"), Some(&1));
        assert_eq!(tree.get(b"abd"), Some(&2));
        assert_eq!(tree.get(b"xyz"), Some(&3));
    }

    #[test]
    fn prefix_key_short_then_long() {
        let mut tree = ARTMap::new();
        tree.put(b"ab", 1);
        tree.put(b"abc", 2);
        assert_eq!(tree.get(b"ab"), Some(&1));
        assert_eq!(tree.get(b"abc"), Some(&2));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn prefix_key_long_then_short() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 2);
        tree.put(b"ab", 1);
        assert_eq!(tree.get(b"ab"), Some(&1));
        assert_eq!(tree.get(b"abc"), Some(&2));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn three_level_prefix_chain() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        assert_eq!(tree.get(b"a"), Some(&1));
        assert_eq!(tree.get(b"ab"), Some(&2));
        assert_eq!(tree.get(b"abc"), Some(&3));
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn empty_key() {
        let mut tree = ARTMap::new();
        tree.put(b"", 0);
        tree.put(b"a", 1);
        assert_eq!(tree.get(b""), Some(&0));
        assert_eq!(tree.get(b"a"), Some(&1));
    }

    #[test]
    fn deep_shared_prefix() {
        let mut tree = ARTMap::new();
        tree.put(b"abcdefghij", 1);
        tree.put(b"abcdefghik", 2);
        assert_eq!(tree.get(b"abcdefghij"), Some(&1));
        assert_eq!(tree.get(b"abcdefghik"), Some(&2));
    }

    #[test]
    fn prefix_split_later_insert() {
        let mut tree = ARTMap::new();
        tree.put(b"abcdef", 1);
        tree.put(b"abcxyz", 2);
        tree.put(b"abZZZ", 3);
        assert_eq!(tree.get(b"abcdef"), Some(&1));
        assert_eq!(tree.get(b"abcxyz"), Some(&2));
        assert_eq!(tree.get(b"abZZZ"), Some(&3));
    }

    #[test]
    fn no_false_match_on_partial_prefix() {
        let mut tree = ARTMap::new();
        tree.put(b"abcdef", 1);
        assert!(tree.get(b"abcXXX").is_none());
        assert!(tree.get(b"abc").is_none());
        assert!(tree.get(b"abcdefg").is_none());
    }

    #[test]
    fn four_children_in_node4() {
        let mut tree = ARTMap::new();
        for i in 0..4u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        for i in 0..4u8 {
            assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
        }
        assert_eq!(tree.len(), 4);
    }

    #[test]
    fn node4_to_node16() {
        let mut tree = ARTMap::new();
        for i in 0..5u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        for i in 0..5u8 {
            assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
        }
        assert_eq!(tree.len(), 5);
    }

    #[test]
    fn node16_to_node48() {
        let mut tree = ARTMap::new();
        for i in 0..17u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        for i in 0..17u8 {
            assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
        }
        assert_eq!(tree.len(), 17);
    }

    #[test]
    fn node48_to_node256() {
        let mut tree = ARTMap::new();
        for i in 0..49u8 {
            tree.put(&[b'A' + i], i as i32);
        }
        for i in 0..49u8 {
            assert_eq!(tree.get(&[b'A' + i]), Some(&(i as i32)));
        }
        assert_eq!(tree.len(), 49);
    }

    #[test]
    fn full_byte_range() {
        let mut tree = ARTMap::new();
        for b in 0..=255u8 {
            tree.put(&[b], b as i32);
        }
        assert_eq!(tree.len(), 256);
        for b in 0..=255u8 {
            assert_eq!(tree.get(&[b]), Some(&(b as i32)));
        }
    }

    #[test]
    fn stress_1000_sequential() {
        let mut tree = ARTMap::new();
        let keys: Vec<Vec<u8>> = (0..1000)
            .map(|i| format!("key{:04}", i).into_bytes())
            .collect();
        for (i, k) in keys.iter().enumerate() {
            tree.put(k, i);
        }
        assert_eq!(tree.len(), 1000);
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(tree.get(k), Some(&i));
        }
    }

    // -- delete tests --

    #[test]
    fn delete_single() {
        let mut tree = ARTMap::new();
        tree.put(b"k", 1);
        assert!(tree.delete(b"k"));
        assert!(tree.get(b"k").is_none());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn delete_missing() {
        let mut tree: ARTMap<i32> = ARTMap::new();
        assert!(!tree.delete(b"x"));
    }

    #[test]
    fn delete_missing_after_real_delete() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.delete(b"a");
        assert!(!tree.delete(b"a"));
    }

    #[test]
    fn delete_one_of_many() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        tree.put(b"c", 3);
        tree.delete(b"b");
        assert_eq!(tree.get(b"a"), Some(&1));
        assert!(tree.get(b"b").is_none());
        assert_eq!(tree.get(b"c"), Some(&3));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn delete_prefix_key_middle() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        tree.delete(b"ab");
        assert_eq!(tree.get(b"a"), Some(&1));
        assert!(tree.get(b"ab").is_none());
        assert_eq!(tree.get(b"abc"), Some(&3));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn delete_prefix_key_shortest() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        tree.delete(b"a");
        assert!(tree.get(b"a").is_none());
        assert_eq!(tree.get(b"ab"), Some(&2));
        assert_eq!(tree.get(b"abc"), Some(&3));
    }

    #[test]
    fn delete_prefix_key_longest() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        tree.delete(b"abc");
        assert_eq!(tree.get(b"a"), Some(&1));
        assert_eq!(tree.get(b"ab"), Some(&2));
        assert!(tree.get(b"abc").is_none());
    }

    #[test]
    fn delete_returns_false_for_prefix_of_existing() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 1);
        assert!(!tree.delete(b"ab"));
        assert!(!tree.delete(b"a"));
        assert_eq!(tree.get(b"abc"), Some(&1));
    }

    #[test]
    fn delete_returns_false_for_extension() {
        let mut tree = ARTMap::new();
        tree.put(b"ab", 1);
        assert!(!tree.delete(b"abc"));
        assert_eq!(tree.get(b"ab"), Some(&1));
    }

    #[test]
    fn reinsert_after_delete() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.delete(b"a");
        tree.put(b"a", 2);
        assert_eq!(tree.get(b"a"), Some(&2));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn delete_all_then_reuse() {
        let mut tree = ARTMap::new();
        for i in 0..10u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        for i in 0..10u8 {
            assert!(tree.delete(&[b'a' + i]));
        }
        assert_eq!(tree.len(), 0);
        tree.put(b"fresh", 1);
        assert_eq!(tree.get(b"fresh"), Some(&1));
    }

    #[test]
    fn shrink_node16_to_node4() {
        let mut tree = ARTMap::new();
        for i in 0..5u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        tree.delete(b"e");
        for i in 0..4u8 {
            assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
        }
        assert_eq!(tree.len(), 4);
    }

    #[test]
    fn shrink_to_single_leaf() {
        let mut tree = ARTMap::new();
        for i in 0..5u8 {
            tree.put(&[b'a' + i], i as i32);
        }
        for i in 1..5u8 {
            tree.delete(&[b'a' + i]);
        }
        assert_eq!(tree.get(b"a"), Some(&0));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn prefix_recompression_after_delete() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 1);
        tree.put(b"abd", 2);
        tree.delete(b"abd");
        assert_eq!(tree.get(b"abc"), Some(&1));
        tree.put(b"abc", 99);
        assert_eq!(tree.get(b"abc"), Some(&99));
    }

    #[test]
    fn delete_all_200() {
        let mut tree = ARTMap::new();
        let keys: Vec<Vec<u8>> = (0..200).map(|i| format!("k{}", i).into_bytes()).collect();
        for k in &keys {
            tree.put(k, 0);
        }
        for k in &keys {
            assert!(tree.delete(k));
        }
        assert_eq!(tree.len(), 0);
        tree.put(b"fresh", 1);
        assert_eq!(tree.get(b"fresh"), Some(&1));
    }

    // -- iteration tests --

    #[test]
    fn items_empty() {
        let tree: ARTMap<i32> = ARTMap::new();
        assert!(tree.items().is_empty());
    }

    #[test]
    fn items_sorted_order() {
        let mut tree = ARTMap::new();
        tree.put(b"c", 3);
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        let items: Vec<_> = tree.items().into_iter().map(|(k, &v)| (k, v)).collect();
        assert_eq!(items, vec![(b"a".as_slice(), 1), (b"b".as_slice(), 2), (b"c".as_slice(), 3)]);
    }

    #[test]
    fn items_with_prefix_keys() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"ab".to_vec(), b"abc".to_vec()]);
    }

    #[test]
    fn items_empty_key_first() {
        let mut tree = ARTMap::new();
        tree.put(b"", 0);
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
        assert_eq!(keys, vec![b"".to_vec(), b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn items_after_deletes() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        tree.put(b"c", 3);
        tree.delete(b"b");
        let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn items_after_growth() {
        let mut tree = ARTMap::new();
        let mut keys: Vec<Vec<u8>> = (0..49u8).map(|i| vec![b'A' + i]).collect();
        for (i, k) in keys.iter().enumerate() {
            tree.put(k, i as i32);
        }
        let result: Vec<Vec<u8>> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
        keys.sort();
        assert_eq!(result, keys);
    }

    #[test]
    fn items_1000_sorted() {
        let mut tree = ARTMap::new();
        let mut keys: Vec<Vec<u8>> = (0..1000)
            .map(|i| format!("key{:04}", i).into_bytes())
            .collect();
        for (i, k) in keys.iter().enumerate() {
            tree.put(k, i);
        }
        let result: Vec<Vec<u8>> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
        keys.sort();
        assert_eq!(result, keys);
    }

    // -- range scan tests --

    fn keys_from_range(tree: &ARTMap<i32>, from: Option<&[u8]>, to: Option<&[u8]>) -> Vec<Vec<u8>> {
        tree.range(from, to).into_iter().map(|(k, _)| k.to_vec()).collect()
    }

    #[test]
    fn range_from_key() {
        let mut tree = ARTMap::new();
        for c in b"abcde" {
            tree.put(&[*c], *c as i32);
        }
        assert_eq!(keys_from_range(&tree, Some(b"c"), None),
            vec![b"c".to_vec(), b"d".to_vec(), b"e".to_vec()]);
    }

    #[test]
    fn range_to_key() {
        let mut tree = ARTMap::new();
        for c in b"abcde" {
            tree.put(&[*c], *c as i32);
        }
        assert_eq!(keys_from_range(&tree, None, Some(b"c")),
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn range_from_and_to() {
        let mut tree = ARTMap::new();
        for c in b"abcde" {
            tree.put(&[*c], *c as i32);
        }
        assert_eq!(keys_from_range(&tree, Some(b"b"), Some(b"d")),
            vec![b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]);
    }

    #[test]
    fn range_empty_result() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"z", 26);
        assert!(keys_from_range(&tree, Some(b"m"), Some(b"n")).is_empty());
    }

    #[test]
    fn range_from_beyond_all() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        assert!(keys_from_range(&tree, Some(b"z"), None).is_empty());
    }

    #[test]
    fn range_to_before_all() {
        let mut tree = ARTMap::new();
        tree.put(b"m", 1);
        tree.put(b"n", 2);
        assert!(keys_from_range(&tree, None, Some(b"a")).is_empty());
    }

    #[test]
    fn range_exact_bounds() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        tree.put(b"c", 3);
        let r = tree.range(Some(b"b"), Some(b"b"));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, b"b");
    }

    #[test]
    fn range_exact_bounds_missing() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"c", 3);
        assert!(tree.range(Some(b"b"), Some(b"b")).is_empty());
    }

    #[test]
    fn range_with_shared_prefix() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 1);
        tree.put(b"abd", 2);
        tree.put(b"abe", 3);
        tree.put(b"abf", 4);
        let items = tree.range(Some(b"abd"), Some(b"abe"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, b"abd");
        assert_eq!(items[1].0, b"abe");
    }

    #[test]
    fn range_prefix_keys() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"ab", 2);
        tree.put(b"abc", 3);
        tree.put(b"abd", 4);
        tree.put(b"b", 5);
        let items = tree.range(Some(b"ab"), Some(b"abd"));
        let keys: Vec<_> = items.into_iter().map(|(k, _)| k.to_vec()).collect();
        assert_eq!(keys, vec![b"ab".to_vec(), b"abc".to_vec(), b"abd".to_vec()]);
    }

    #[test]
    fn range_from_is_prefix_of_keys() {
        let mut tree = ARTMap::new();
        tree.put(b"abc", 1);
        tree.put(b"abd", 2);
        tree.put(b"xyz", 3);
        let keys = keys_from_range(&tree, Some(b"ab"), None);
        assert_eq!(keys, vec![b"abc".to_vec(), b"abd".to_vec(), b"xyz".to_vec()]);
    }

    #[test]
    fn range_to_is_prefix_of_keys() {
        let mut tree = ARTMap::new();
        tree.put(b"a", 1);
        tree.put(b"abc", 2);
        tree.put(b"abd", 3);
        tree.put(b"b", 4);
        let keys = keys_from_range(&tree, None, Some(b"ab"));
        assert_eq!(keys, vec![b"a".to_vec()]);
    }

    #[test]
    fn range_with_empty_from_key() {
        let mut tree = ARTMap::new();
        tree.put(b"", 0);
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        let keys = keys_from_range(&tree, Some(b""), None);
        assert_eq!(keys, vec![b"".to_vec(), b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn range_with_empty_to_key() {
        let mut tree = ARTMap::new();
        tree.put(b"", 0);
        tree.put(b"a", 1);
        tree.put(b"b", 2);
        let keys = keys_from_range(&tree, None, Some(b""));
        assert_eq!(keys, vec![b"".to_vec()]);
    }

    #[test]
    fn range_stress_500() {
        let mut tree = ARTMap::new();
        let keys: Vec<Vec<u8>> = (0..500)
            .map(|i| format!("k{:04}", i).into_bytes())
            .collect();
        for k in &keys {
            tree.put(k, 0);
        }
        for &(lo, hi) in &[(50, 100), (0, 10), (490, 499), (200, 200)] {
            let lo_key = format!("k{:04}", lo).into_bytes();
            let hi_key = format!("k{:04}", hi).into_bytes();
            let result = keys_from_range(&tree, Some(&lo_key), Some(&hi_key));
            let expected: Vec<Vec<u8>> = (lo..=hi)
                .map(|i| format!("k{:04}", i).into_bytes())
                .collect();
            assert_eq!(result, expected, "range [{}, {}]", lo, hi);
        }
    }

    #[test]
    fn range_no_overlap() {
        let mut tree = ARTMap::new();
        tree.put(b"aaa", 1);
        tree.put(b"bbb", 2);
        tree.put(b"ccc", 3);
        assert!(tree.range(Some(b"d"), Some(b"z")).is_empty());
        assert!(tree.range(Some(b"0"), Some(b"1")).is_empty());
    }

    #[test]
    fn range_deep_tree() {
        let base = "a".repeat(50);
        let mut tree = ARTMap::new();
        let keys: Vec<Vec<u8>> = (0..10)
            .map(|i| format!("{}{}", base, (b'a' + i) as char).into_bytes())
            .collect();
        for k in &keys {
            tree.put(k, 0);
        }
        let result = keys_from_range(&tree, Some(&keys[3]), Some(&keys[7]));
        assert_eq!(result, keys[3..8].to_vec());
    }

    #[test]
    fn interleaved_insert_delete() {
        use std::collections::HashMap;
        let mut tree = ARTMap::new();
        let mut live: HashMap<Vec<u8>, i32> = HashMap::new();
        // Simple LCG PRNG for determinism
        let mut rng: u64 = 99;
        let mut next = || -> u64 { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); rng >> 33 };
        for _ in 0..2000 {
            let k = format!("k{}", next() % 201).into_bytes();
            if next() % 10 < 7 {
                let v = (next() % 100000) as i32;
                tree.put(&k, v);
                live.insert(k, v);
            } else {
                let existed = live.remove(&k).is_some();
                assert_eq!(tree.delete(&k), existed);
            }
        }
        assert_eq!(tree.len(), live.len());
        for (k, v) in &live {
            assert_eq!(tree.get(k), Some(v));
        }
        let items = tree.items();
        let mut expected: Vec<_> = live.iter().map(|(k, v)| (k.clone(), *v)).collect();
        expected.sort();
        let actual: Vec<_> = items.into_iter().map(|(k, &v)| (k.to_vec(), v)).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn range_matches_full_scan() {
        // Verify range scan matches naive filter of full scan
        let mut tree = ARTMap::new();
        let keys: Vec<Vec<u8>> = (0..200)
            .map(|i| format!("k{:04}", i).into_bytes())
            .collect();
        for k in &keys {
            tree.put(k, 0);
        }
        let lo = b"k0050".to_vec();
        let hi = b"k0150".to_vec();
        let range_result = keys_from_range(&tree, Some(&lo), Some(&hi));
        let full_result: Vec<Vec<u8>> = tree.items().into_iter()
            .filter(|(k, _)| k >= &lo.as_slice() && k <= &hi.as_slice())
            .map(|(k, _)| k.to_vec())
            .collect();
        assert_eq!(range_result, full_result);
    }
}
