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

    fn from_leaf(ptr: *mut Leaf<V>) -> Self {
        let raw = ptr as usize;
        debug_assert!(raw & TAG_MASK == 0, "leaf pointer not aligned");
        NodePtr(raw | TAG_LEAF, std::marker::PhantomData)
    }

    // from_node* constructors removed — use alloc_node4/16/48/256 instead

    // -- accessors --

    fn as_leaf(&self) -> &Leaf<V> {
        debug_assert!(self.is_leaf());
        unsafe { &*((self.0 & !TAG_MASK) as *const Leaf<V>) }
    }

    fn leaf_ptr(self) -> *mut Leaf<V> {
        debug_assert!(self.is_leaf());
        (self.0 & !TAG_MASK) as *mut Leaf<V>
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
    fn as_node16(&self) -> &Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &*(self.inner_ptr() as *const Node16<V>) }
    }
    fn as_node16_mut(&mut self) -> &mut Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &mut *(self.inner_ptr() as *mut Node16<V>) }
    }
    fn as_node48(&self) -> &Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &*(self.inner_ptr() as *const Node48<V>) }
    }
    fn as_node48_mut(&mut self) -> &mut Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &mut *(self.inner_ptr() as *mut Node48<V>) }
    }
    fn as_node256(&self) -> &Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &*(self.inner_ptr() as *const Node256<V>) }
    }
    fn as_node256_mut(&mut self) -> &mut Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &mut *(self.inner_ptr() as *mut Node256<V>) }
    }
    /// Free the memory behind this pointer (recursively for inner nodes).
    unsafe fn drop_recursive(mut self) {
        if self.is_null() {
            return;
        }
        if self.is_leaf() {
            drop_leaf(self);
            return;
        }
        // Drop the value if present
        match self.kind() {
            KIND_NODE4 => {
                let n = self.as_node4_mut();
                std::ptr::drop_in_place(&mut n.value);
                for i in 0..n.count as usize { n.children[i].drop_recursive(); }
            }
            KIND_NODE16 => {
                let n = self.as_node16_mut();
                std::ptr::drop_in_place(&mut n.value);
                for i in 0..n.count as usize { n.children[i].drop_recursive(); }
            }
            KIND_NODE48 => {
                let n = self.as_node48_mut();
                std::ptr::drop_in_place(&mut n.value);
                for i in 0..256usize {
                    if n.index[i] != 0xFF { n.slots[n.index[i] as usize].drop_recursive(); }
                }
            }
            KIND_NODE256 => {
                let n = self.as_node256_mut();
                std::ptr::drop_in_place(&mut n.value);
                for i in 0..256usize {
                    if !n.children[i].is_null() { n.children[i].drop_recursive(); }
                }
            }
            _ => unreachable!(),
        }
        dealloc_inner_shell(self);
    }
}

// ---------------------------------------------------------------------------
// Leaf — key bytes stored inline after the struct (no Vec indirection)
// ---------------------------------------------------------------------------

#[repr(C)]
struct Leaf<V> {
    key_len: u32,
    value: V,
    // [u8; key_len] follows immediately in memory
}

impl<V> Leaf<V> {
    fn key(&self) -> &[u8] {
        unsafe {
            let base = (self as *const Self as *const u8).add(std::mem::size_of::<Self>());
            std::slice::from_raw_parts(base, self.key_len as usize)
        }
    }
}

fn leaf_layout<V>(key_len: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(
        std::mem::size_of::<Leaf<V>>() + key_len,
        std::mem::align_of::<Leaf<V>>(),
    )
    .unwrap()
}

/// Allocate a Leaf with inline key bytes, return a tagged NodePtr.
fn alloc_leaf<V>(key: &[u8], value: V) -> NodePtr<V> {
    unsafe {
        let layout = leaf_layout::<V>(key.len());
        let ptr = std::alloc::alloc(layout) as *mut Leaf<V>;
        assert!(!ptr.is_null(), "allocation failed");
        std::ptr::write(&mut (*ptr).key_len, key.len() as u32);
        std::ptr::write(&mut (*ptr).value, value);
        let key_dst = (ptr as *mut u8).add(std::mem::size_of::<Leaf<V>>());
        std::ptr::copy_nonoverlapping(key.as_ptr(), key_dst, key.len());
        NodePtr::from_leaf(ptr)
    }
}

/// Read the value out of a leaf and deallocate it. Returns the value.
unsafe fn consume_leaf<V>(node: NodePtr<V>) -> V {
    debug_assert!(node.is_leaf());
    let ptr = (node.0 & !TAG_MASK) as *mut Leaf<V>;
    let key_len = (*ptr).key_len as usize;
    let value = std::ptr::read(&(*ptr).value);
    let layout = leaf_layout::<V>(key_len);
    std::alloc::dealloc(ptr as *mut u8, layout);
    value
}

/// Deallocate a leaf, dropping the value.
unsafe fn drop_leaf<V>(node: NodePtr<V>) {
    debug_assert!(node.is_leaf());
    let ptr = (node.0 & !TAG_MASK) as *mut Leaf<V>;
    let key_len = (*ptr).key_len as usize;
    std::ptr::drop_in_place(&mut (*ptr).value);
    let layout = leaf_layout::<V>(key_len);
    std::alloc::dealloc(ptr as *mut u8, layout);
}

// ---------------------------------------------------------------------------
// Inner node header (common fields)
// ---------------------------------------------------------------------------
// Each inner node stores: prefix (inline trailing bytes), optional value
// (for prefix-key storage), and children keyed by a single byte.

/// Optional value on an inner node (when a key is a prefix of longer keys).
type InnerValue<V> = Option<(Vec<u8>, V)>; // (full_key, value)

// ---------------------------------------------------------------------------
// Node types — prefix bytes stored inline after each struct (#[repr(C)])
// ---------------------------------------------------------------------------

macro_rules! define_node {
    ($name:ident { $($field:ident : $ty:ty = $init:expr),* $(,)? }) => {
        #[repr(C)]
        struct $name<V> {
            prefix_len: u16,
            value: InnerValue<V>,
            count: u8,
            $($field: $ty,)*
            // [u8; prefix_len] follows immediately in memory
        }

        impl<V> $name<V> {
            fn prefix(&self) -> &[u8] {
                unsafe {
                    let base = (self as *const Self as *const u8)
                        .add(std::mem::size_of::<Self>());
                    std::slice::from_raw_parts(base, self.prefix_len as usize)
                }
            }
        }
    };
}

define_node!(Node4 { keys: [u8; 4] = [0; 4], children: [NodePtr<V>; 4] = [NodePtr::NULL; 4] });
define_node!(Node16 { keys: [u8; 16] = [0; 16], children: [NodePtr<V>; 16] = [NodePtr::NULL; 16] });
define_node!(Node48 { index: [u8; 256] = [0xFF; 256], slots: [NodePtr<V>; 48] = [NodePtr::NULL; 48] });

// Node256 has u16 count (> 48 children possible), so define it separately.
#[repr(C)]
struct Node256<V> {
    prefix_len: u16,
    value: InnerValue<V>,
    count: u16,
    children: [NodePtr<V>; 256],
    // [u8; prefix_len] follows
}
impl<V> Node256<V> {
    fn prefix(&self) -> &[u8] {
        unsafe {
            let base = (self as *const Self as *const u8)
                .add(std::mem::size_of::<Self>());
            std::slice::from_raw_parts(base, self.prefix_len as usize)
        }
    }
}

// ---------------------------------------------------------------------------
// Inner node allocation helpers
// ---------------------------------------------------------------------------

fn node_layout<T>(prefix_len: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(
        std::mem::size_of::<T>() + prefix_len,
        std::mem::align_of::<T>(),
    )
    .unwrap()
}

/// Allocate an inner node with the given prefix bytes. Initializes all
/// fixed fields to zero/null/None; caller must set children/count/etc.
macro_rules! alloc_node {
    ($name:ident, $kind:expr, $prefix:expr $(, $field:ident = $val:expr)*) => {{
        let prefix: &[u8] = $prefix;
        let layout = node_layout::<$name<V>>(prefix.len());
        unsafe {
            let ptr = std::alloc::alloc(layout) as *mut $name<V>;
            assert!(!ptr.is_null(), "allocation failed");
            std::ptr::write(&mut (*ptr).prefix_len, prefix.len() as u16);
            std::ptr::write(&mut (*ptr).value, None);
            std::ptr::write(&mut (*ptr).count, 0);
            $(std::ptr::write(&mut (*ptr).$field, $val);)*
            // Copy prefix bytes
            let dst = (ptr as *mut u8).add(std::mem::size_of::<$name<V>>());
            std::ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
            NodePtr(ptr as usize | $kind, std::marker::PhantomData)
        }
    }};
}

fn alloc_node4<V>(prefix: &[u8]) -> NodePtr<V> {
    alloc_node!(Node4, KIND_NODE4, prefix,
        keys = [0u8; 4],
        children = [NodePtr::NULL; 4])
}

fn alloc_node16<V>(prefix: &[u8]) -> NodePtr<V> {
    alloc_node!(Node16, KIND_NODE16, prefix,
        keys = [0u8; 16],
        children = [NodePtr::NULL; 16])
}

fn alloc_node48<V>(prefix: &[u8]) -> NodePtr<V> {
    alloc_node!(Node48, KIND_NODE48, prefix,
        index = [0xFFu8; 256],
        slots = [NodePtr::NULL; 48])
}

fn alloc_node256<V>(prefix: &[u8]) -> NodePtr<V> {
    unsafe {
        let layout = node_layout::<Node256<V>>(prefix.len());
        let ptr = std::alloc::alloc(layout) as *mut Node256<V>;
        assert!(!ptr.is_null(), "allocation failed");
        std::ptr::write(&mut (*ptr).prefix_len, prefix.len() as u16);
        std::ptr::write(&mut (*ptr).value, None);
        std::ptr::write(&mut (*ptr).count, 0);
        std::ptr::write(&mut (*ptr).children, [NodePtr::NULL; 256]);
        let dst = (ptr as *mut u8).add(std::mem::size_of::<Node256<V>>());
        std::ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
        NodePtr(ptr as usize | KIND_NODE256, std::marker::PhantomData)
    }
}

/// Deallocate an inner node shell (no child recursion, no value drop).
/// Used after children/value have been moved out.
fn dealloc_inner_shell<V>(node: NodePtr<V>) {
    unsafe {
        let ptr = node.inner_ptr();
        let (size_of, align_of, plen) = match node.kind() {
            KIND_NODE4 => (std::mem::size_of::<Node4<V>>(), std::mem::align_of::<Node4<V>>(), (*(ptr as *const Node4<V>)).prefix_len as usize),
            KIND_NODE16 => (std::mem::size_of::<Node16<V>>(), std::mem::align_of::<Node16<V>>(), (*(ptr as *const Node16<V>)).prefix_len as usize),
            KIND_NODE48 => (std::mem::size_of::<Node48<V>>(), std::mem::align_of::<Node48<V>>(), (*(ptr as *const Node48<V>)).prefix_len as usize),
            KIND_NODE256 => (std::mem::size_of::<Node256<V>>(), std::mem::align_of::<Node256<V>>(), (*(ptr as *const Node256<V>)).prefix_len as usize),
            _ => unreachable!(),
        };
        let layout = std::alloc::Layout::from_size_align_unchecked(size_of + plen, align_of);
        std::alloc::dealloc(ptr, layout);
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
                if leaf.key() == key {
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

    /// Iterate all (key, value) pairs in sorted key order (collects into Vec).
    pub fn items(&self) -> Vec<(&[u8], &V)> {
        self.iter().collect()
    }

    /// Iterate (key, value) pairs in [from_key, to_key] (collects into Vec).
    pub fn range<'a>(
        &'a self,
        from_key: Option<&'a [u8]>,
        to_key: Option<&'a [u8]>,
    ) -> Vec<(&'a [u8], &'a V)> {
        self.range_iter(from_key, to_key).collect()
    }

    /// Lazy iterator over all (key, value) pairs in sorted order.
    pub fn iter(&self) -> Iter<'_, V> {
        let mut stack = Vec::new();
        if !self.root.is_null() {
            stack.push(self.root);
        }
        Iter { stack, _marker: std::marker::PhantomData }
    }

    /// Lazy iterator over (key, value) pairs in [lo, hi] with O(log n + k) pruning.
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

// ---------------------------------------------------------------------------
// Lazy iterator (full scan)
// ---------------------------------------------------------------------------

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
                return Some((leaf.key(), &leaf.value));
            }
            // Inner node: push children in reverse byte order (smallest on top)
            push_children_rev(node, &mut self.stack);
            // Yield own value if present
            if let Some((k, v)) = unsafe { inner_value_raw(node) } {
                return Some((k.as_slice(), v));
            }
        }
    }
}

/// Push children of an inner node in reverse sorted order onto the stack.
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

// ---------------------------------------------------------------------------
// Lazy range iterator with O(log n + k) boundary pruning
// ---------------------------------------------------------------------------

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
                let kb = leaf.key();
                if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
                    return Some((kb, &leaf.value));
                }
                continue;
            }

            // Inner node — boundary analysis
            let p = unsafe { inner_prefix_raw(node) };
            let plen = p.len();
            let nd = depth + plen;

            // lo boundary
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
                    if pp < lp { continue; }       // subtree < lo
                    if pp > lp { lo = None; }
                    else if cn < plen { lo = None; }
                    else if lo_avail > plen { lo_on = true; }
                    else { lo = None; }
                }
            }

            // hi boundary
            let mut hi = hi;
            let mut hi_on = false;
            if let Some(hi_bytes) = hi {
                let hi_avail = hi_bytes.len().saturating_sub(depth);
                if hi_avail == 0 {
                    // hi exhausted: only own value could match
                    if let Some((k, v)) = unsafe { inner_value_raw(node) } {
                        let kb = k.as_slice();
                        if lo.map_or(true, |lo| kb >= lo) && kb <= hi_bytes {
                            return Some((kb, v));
                        }
                    }
                    continue;
                } else if plen == 0 {
                    hi_on = true;
                } else {
                    let cn = plen.min(hi_avail);
                    let pp = &p[..cn];
                    let hp = &hi_bytes[depth..depth + cn];
                    if pp > hp { continue; }       // subtree > hi
                    if pp < hp { hi = None; }
                    else if cn < plen { continue; } // hi exhausted inside prefix
                    else if hi_avail > plen { hi_on = true; }
                    else {
                        // hi exhausted at nd; own value may match, children > hi
                        if let Some((k, v)) = unsafe { inner_value_raw(node) } {
                            let kb = k.as_slice();
                            if lo.map_or(true, |lo| kb >= lo) && kb <= hi_bytes {
                                return Some((kb, v));
                            }
                        }
                        continue;
                    }
                }
            }

            // Push children in range, in reverse byte order
            let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
            let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

            push_range_children_rev(
                node, nd + 1, lo_byte, hi_byte,
                lo_on, lo, hi_on, hi, &mut self.stack,
            );

            // Yield own value if in range
            if let Some((k, v)) = unsafe { inner_value_raw(node) } {
                let kb = k.as_slice();
                if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
                    return Some((kb, v));
                }
            }
        }
    }
}

/// Push in-range children in reverse byte order onto the range stack.
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
    // Closure to compute per-child bounds and push
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

// ---------------------------------------------------------------------------
// Inner node field accessors (dispatch on kind)
// ---------------------------------------------------------------------------

/// Get prefix from inner node (reads inline trailing bytes).
unsafe fn inner_prefix_raw<'a, V>(node: NodePtr<V>) -> &'a [u8] {
    let ptr = node.inner_ptr();
    let (size_of, plen) = match node.kind() {
        KIND_NODE4 => (std::mem::size_of::<Node4<V>>(), (*(ptr as *const Node4<V>)).prefix_len as usize),
        KIND_NODE16 => (std::mem::size_of::<Node16<V>>(), (*(ptr as *const Node16<V>)).prefix_len as usize),
        KIND_NODE48 => (std::mem::size_of::<Node48<V>>(), (*(ptr as *const Node48<V>)).prefix_len as usize),
        KIND_NODE256 => (std::mem::size_of::<Node256<V>>(), (*(ptr as *const Node256<V>)).prefix_len as usize),
        _ => unreachable!(),
    };
    std::slice::from_raw_parts(ptr.add(size_of), plen)
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

/// Change the prefix of an inner node. Reallocates and returns a new NodePtr
/// since the inline trailing bytes change size. Frees the old allocation.
fn inner_set_prefix<V>(node: &mut NodePtr<V>, prefix: &[u8]) {
    unsafe {
        let old = *node;
        let new = match old.kind() {
            KIND_NODE4 => {
                let mut n = alloc_node4::<V>(&prefix);
                let src = old.as_node4();
                let dst = n.as_node4_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&src.value));
                dst.count = src.count;
                dst.keys = src.keys;
                dst.children = src.children;
                n
            }
            KIND_NODE16 => {
                let mut n = alloc_node16::<V>(&prefix);
                let src = old.as_node16();
                let dst = n.as_node16_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&src.value));
                dst.count = src.count;
                dst.keys = src.keys;
                dst.children = src.children;
                n
            }
            KIND_NODE48 => {
                let mut n = alloc_node48::<V>(&prefix);
                let src = old.as_node48();
                let dst = n.as_node48_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&src.value));
                dst.count = src.count;
                dst.index = src.index;
                dst.slots = src.slots;
                n
            }
            KIND_NODE256 => {
                let mut n = alloc_node256::<V>(&prefix);
                let src = old.as_node256();
                let dst = n.as_node256_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&src.value));
                dst.count = src.count;
                dst.children = src.children;
                n
            }
            _ => unreachable!(),
        };
        // Free old allocation without dropping value or recursing children
        dealloc_inner_shell(old);
        *node = new;
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

// ---------------------------------------------------------------------------
// Node growth — allocate next size, copy prefix+value+children, free old
// ---------------------------------------------------------------------------

fn grow<V>(node: NodePtr<V>) -> NodePtr<V> {
    unsafe {
        let prefix = inner_prefix_raw(node).to_vec();
        match node.kind() {
            KIND_NODE4 => {
                let mut new_ptr = alloc_node16::<V>(&prefix);
                let old = node.as_node4();
                let cnt = old.count as usize;
                let dst = new_ptr.as_node16_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.count = cnt as u8;
                dealloc_inner_shell(node);
                new_ptr
            }
            KIND_NODE16 => {
                let mut new_ptr = alloc_node48::<V>(&prefix);
                let old = node.as_node16();
                let cnt = old.count as usize;
                let dst = new_ptr.as_node48_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
                for i in 0..cnt {
                    dst.index[old.keys[i] as usize] = i as u8;
                    dst.slots[i] = old.children[i];
                }
                dst.count = cnt as u8;
                dealloc_inner_shell(node);
                new_ptr
            }
            KIND_NODE48 => {
                let mut new_ptr = alloc_node256::<V>(&prefix);
                let old = node.as_node48();
                let dst = new_ptr.as_node256_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
                let mut cnt = 0u16;
                for b in 0..256usize {
                    let idx = old.index[b];
                    if idx != 0xFF {
                        dst.children[b] = old.slots[idx as usize];
                        cnt += 1;
                    }
                }
                dst.count = cnt;
                dealloc_inner_shell(node);
                new_ptr
            }
            _ => unreachable!("Node256 cannot grow"),
        }
    }
}

// ---------------------------------------------------------------------------
// Node shrinkage
// ---------------------------------------------------------------------------

fn shrink<V>(node: NodePtr<V>) -> NodePtr<V> {
    unsafe {
        let prefix = inner_prefix_raw(node).to_vec();
        match node.kind() {
            KIND_NODE256 => {
                let mut new_ptr = alloc_node48::<V>(&prefix);
                let old = node.as_node256();
                let dst = new_ptr.as_node48_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
                let mut slot = 0u8;
                for b in 0..256usize {
                    if !old.children[b].is_null() {
                        dst.index[b] = slot;
                        dst.slots[slot as usize] = old.children[b];
                        slot += 1;
                    }
                }
                dst.count = slot;
                dealloc_inner_shell(node);
                new_ptr
            }
            KIND_NODE48 => {
                let mut new_ptr = alloc_node16::<V>(&prefix);
                let old = node.as_node48();
                let dst = new_ptr.as_node16_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
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
                dealloc_inner_shell(node);
                new_ptr
            }
            KIND_NODE16 => {
                let mut new_ptr = alloc_node4::<V>(&prefix);
                let old = node.as_node16();
                let cnt = old.count as usize;
                let dst = new_ptr.as_node4_mut();
                std::ptr::write(&mut dst.value, std::ptr::read(&old.value));
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.count = cnt as u8;
                dealloc_inner_shell(node);
                new_ptr
            }
            _ => node, // Node4 cannot shrink further
        }
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

/// Read the prefix into a Vec (copies the inline bytes).
fn inner_read_prefix<V>(node: &NodePtr<V>) -> Vec<u8> {
    unsafe { inner_prefix_raw(*node) }.to_vec()
}

// ---------------------------------------------------------------------------
// Compact after deletion
// ---------------------------------------------------------------------------

/// Collapse a node that became degenerate after a child removal.
fn compact<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    let count = inner_count(&node);

    if count == 0 {
        if inner_has_value(&node) {
            let val = inner_clear_value(&mut node);
            dealloc_inner_shell(node);
            let (k, v) = val.unwrap();
            return alloc_leaf(&k, v);
        }
        dealloc_inner_shell(node);
        return NodePtr::NULL;
    }

    if count == 1 && !inner_has_value(&node) {
        let children = inner_children(&node);
        let (b, child) = children[0];
        if child.is_leaf() {
            dealloc_inner_shell(node);
            return child;
        }
        // Single inner child: merge prefixes
        let parent_prefix = inner_read_prefix(&node);
        let child_prefix = inner_read_prefix(&child);
        dealloc_inner_shell(node);
        let mut merged = parent_prefix;
        merged.push(b);
        merged.extend_from_slice(&child_prefix);
        let mut child = child;
        inner_set_prefix(&mut child, &merged);
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

// free_inner_node_shell removed — use dealloc_inner_shell directly

// ---------------------------------------------------------------------------
// Recursive delete
// ---------------------------------------------------------------------------

/// Returns (new_node_or_null, was_deleted)
fn delete_recursive<V>(node: NodePtr<V>, key: &[u8], depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (NodePtr::NULL, false);
    }

    if node.is_leaf() {
        if node.as_leaf().key() == key {
            unsafe { drop_leaf(node); }
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
        return (alloc_leaf(key, value), true, unsafe { std::mem::zeroed() });
    }

    // Leaf
    if node.is_leaf() {
        let existing = node.as_leaf();
        if existing.key() == key {
            // Update existing leaf in place (key unchanged, so allocation stays)
            let leaf = unsafe { &mut *node.leaf_ptr() };
            let old_value = std::mem::replace(&mut leaf.value, value);
            return (node, false, old_value);
        }

        // Mismatch: create Node4 to hold both
        let ekb = existing.key().to_vec(); // copy key before we might consume the leaf
        let common = prefix_mismatch(key, depth, &ekb, depth);
        let sd = depth + common; // split depth

        let mut nn_ptr = alloc_node4::<V>(&key[depth..sd]);

        if sd == key.len() {
            // New key is prefix of existing
            inner_set_value(&mut nn_ptr, key.to_vec(), value);
            inner_add_child(&mut nn_ptr, ekb[sd], node);
        } else if sd == ekb.len() {
            // Existing key is prefix of new key
            let existing_value = unsafe { consume_leaf(node) };
            inner_set_value(&mut nn_ptr, ekb.clone(), existing_value);
            inner_add_child(&mut nn_ptr, key[sd], alloc_leaf(key, value));
        } else {
            let new_b = key[sd];
            let old_b = ekb[sd];
            inner_add_child(&mut nn_ptr, new_b, alloc_leaf(key, value));
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
        let mut nn_ptr = alloc_node4::<V>(&prefix[..ml]);

        // Update old node's prefix to suffix after split point
        let mut old_node = node;
        inner_set_prefix(&mut old_node, &prefix[ml + 1..]);
        inner_add_child(&mut nn_ptr, prefix[ml], old_node);

        let nd = depth + ml;
        if nd == key.len() {
            inner_set_value(&mut nn_ptr, key.to_vec(), value);
        } else {
            inner_add_child(&mut nn_ptr, key[nd], alloc_leaf(key, value));
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
        inner_add_child(&mut node, b, alloc_leaf(key, value));
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
