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
}
