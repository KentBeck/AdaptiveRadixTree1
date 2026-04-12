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

/// Copy header (prefix + value) from one inner node ptr to another.
fn inner_copy_header<V>(src: &NodePtr<V>, dst: &mut NodePtr<V>) {
    let (prefix, value) = match src.kind() {
        KIND_NODE4 => {
            let s = src.as_node4();
            (s.prefix.clone(), s.value.take_from_ref())
        }
        KIND_NODE16 => {
            let s = src.as_node16();
            (s.prefix.clone(), s.value.take_from_ref())
        }
        KIND_NODE48 => {
            let s = src.as_node48();
            (s.prefix.clone(), s.value.take_from_ref())
        }
        KIND_NODE256 => {
            let s = src.as_node256();
            (s.prefix.clone(), s.value.take_from_ref())
        }
        _ => unreachable!(),
    };
    inner_set_prefix(dst, prefix);
    match dst.kind() {
        KIND_NODE4 => dst.as_node4_mut().value = value,
        KIND_NODE16 => dst.as_node16_mut().value = value,
        KIND_NODE48 => dst.as_node48_mut().value = value,
        KIND_NODE256 => dst.as_node256_mut().value = value,
        _ => unreachable!(),
    }
}

// We can't "take" from a shared ref without unsafe. For inner_copy_header we need
// to read prefix/value from the source and write to destination. Since this is called
// during grow/shrink where the source is about to be dropped, we use ptr::read.
trait TakeFromRef<T> {
    fn take_from_ref(&self) -> T;
}
impl<V> TakeFromRef<InnerValue<V>> for InnerValue<V> {
    fn take_from_ref(&self) -> InnerValue<V> {
        unsafe { std::ptr::read(self) }
    }
}

// ---------------------------------------------------------------------------
// Node growth
// ---------------------------------------------------------------------------

fn grow<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => {
            let mut new = Box::new(Node16::<V>::new());
            let mut new_ptr = NodePtr::from_node16(new);
            inner_copy_header(&node, &mut new_ptr);
            let old = node.as_node4();
            let cnt = old.count as usize;
            // Copy children (already sorted in Node4)
            {
                let dst = new_ptr.as_node16_mut();
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.count = cnt as u8;
            }
            // Free old node without dropping children
            let mut old_box = node.into_node4_box();
            old_box.count = 0; // prevent child cleanup
            old_box.value = None;
            drop(old_box);
            new_ptr
        }
        KIND_NODE16 => {
            let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
            inner_copy_header(&node, &mut new_ptr);
            let old = node.as_node16();
            let cnt = old.count as usize;
            {
                let dst = new_ptr.as_node48_mut();
                for i in 0..cnt {
                    let b = old.keys[i];
                    // Find free slot (they're all free in new node)
                    dst.index[b as usize] = i as u8;
                    dst.slots[i] = old.children[i];
                }
                dst.count = cnt as u8;
            }
            let mut old_box = node.into_node16_box();
            old_box.count = 0;
            old_box.value = None;
            drop(old_box);
            new_ptr
        }
        KIND_NODE48 => {
            let mut new_ptr = NodePtr::from_node256(Box::new(Node256::<V>::new()));
            inner_copy_header(&node, &mut new_ptr);
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
            let mut old_box = node.into_node48_box();
            old_box.count = 0;
            old_box.value = None;
            // Clear index so drop_recursive won't touch children
            old_box.index = [0xFF; 256];
            drop(old_box);
            new_ptr
        }
        _ => unreachable!("Node256 cannot grow"),
    }
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
}
