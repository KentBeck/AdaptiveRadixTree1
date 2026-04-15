use crate::prefix::Prefix;

pub(crate) const TAG_LEAF: usize = 1;
pub(crate) const TAG_MASK: usize = 0b111;
pub(crate) const KIND_NODE4: usize = 0b000;
pub(crate) const KIND_NODE16: usize = 0b010;
pub(crate) const KIND_NODE48: usize = 0b100;
pub(crate) const KIND_NODE256: usize = 0b110;

pub(crate) struct NodePtr<V>(pub(crate) usize, std::marker::PhantomData<V>);

impl<V> Clone for NodePtr<V> {
    fn clone(&self) -> Self {
        NodePtr(self.0, std::marker::PhantomData)
    }
}

impl<V> Copy for NodePtr<V> {}

impl<V> NodePtr<V> {
    pub(crate) const NULL: Self = NodePtr(0, std::marker::PhantomData);

    pub(crate) fn is_null(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn is_leaf(self) -> bool {
        !self.is_null() && (self.0 & TAG_LEAF) != 0
    }

    pub(crate) fn from_leaf(leaf: Box<Leaf<V>>) -> Self {
        let raw = Box::into_raw(leaf) as usize;
        debug_assert!(raw & TAG_MASK == 0, "leaf pointer not aligned");
        NodePtr(raw | TAG_LEAF, std::marker::PhantomData)
    }

    pub(crate) fn from_node4(node: Box<Node4<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE4, std::marker::PhantomData)
    }

    pub(crate) fn from_node16(node: Box<Node16<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE16, std::marker::PhantomData)
    }

    pub(crate) fn from_node48(node: Box<Node48<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE48, std::marker::PhantomData)
    }

    pub(crate) fn from_node256(node: Box<Node256<V>>) -> Self {
        let raw = Box::into_raw(node) as usize;
        debug_assert!(raw & TAG_MASK == 0);
        NodePtr(raw | KIND_NODE256, std::marker::PhantomData)
    }

    pub(crate) fn as_leaf(&self) -> &Leaf<V> {
        debug_assert!(self.is_leaf());
        unsafe { &*((self.0 & !TAG_MASK) as *const Leaf<V>) }
    }

    pub(crate) fn into_leaf_box(self) -> Box<Leaf<V>> {
        debug_assert!(self.is_leaf());
        unsafe { Box::from_raw((self.0 & !TAG_MASK) as *mut Leaf<V>) }
    }

    pub(crate) fn kind(&self) -> usize {
        debug_assert!(!self.is_null() && !self.is_leaf());
        self.0 & TAG_MASK
    }

    pub(crate) fn inner_ptr(&self) -> *mut u8 {
        (self.0 & !TAG_MASK) as *mut u8
    }

    pub(crate) fn header(&self) -> &NodeHeader<V> {
        debug_assert!(!self.is_null() && !self.is_leaf());
        unsafe { &*(self.inner_ptr() as *const NodeHeader<V>) }
    }

    pub(crate) fn header_mut(&mut self) -> &mut NodeHeader<V> {
        debug_assert!(!self.is_null() && !self.is_leaf());
        unsafe { &mut *(self.inner_ptr() as *mut NodeHeader<V>) }
    }

    pub(crate) fn as_node4(&self) -> &Node4<V> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { &*(self.inner_ptr() as *const Node4<V>) }
    }

    pub(crate) fn as_node4_mut(&mut self) -> &mut Node4<V> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { &mut *(self.inner_ptr() as *mut Node4<V>) }
    }

    pub(crate) fn into_node4_box(self) -> Box<Node4<V>> {
        debug_assert!(self.kind() == KIND_NODE4);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node4<V>) }
    }

    pub(crate) fn as_node16(&self) -> &Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &*(self.inner_ptr() as *const Node16<V>) }
    }

    pub(crate) fn as_node16_mut(&mut self) -> &mut Node16<V> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { &mut *(self.inner_ptr() as *mut Node16<V>) }
    }

    pub(crate) fn into_node16_box(self) -> Box<Node16<V>> {
        debug_assert!(self.kind() == KIND_NODE16);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node16<V>) }
    }

    pub(crate) fn as_node48(&self) -> &Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &*(self.inner_ptr() as *const Node48<V>) }
    }

    pub(crate) fn as_node48_mut(&mut self) -> &mut Node48<V> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { &mut *(self.inner_ptr() as *mut Node48<V>) }
    }

    pub(crate) fn into_node48_box(self) -> Box<Node48<V>> {
        debug_assert!(self.kind() == KIND_NODE48);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node48<V>) }
    }

    pub(crate) fn as_node256(&self) -> &Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &*(self.inner_ptr() as *const Node256<V>) }
    }

    pub(crate) fn as_node256_mut(&mut self) -> &mut Node256<V> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { &mut *(self.inner_ptr() as *mut Node256<V>) }
    }

    pub(crate) fn into_node256_box(self) -> Box<Node256<V>> {
        debug_assert!(self.kind() == KIND_NODE256);
        unsafe { Box::from_raw(self.inner_ptr() as *mut Node256<V>) }
    }

    pub(crate) unsafe fn drop_recursive(self) {
        if self.is_null() {
            return;
        }
        if self.is_leaf() {
            drop(self.into_leaf_box());
            return;
        }
        match self.kind() {
            KIND_NODE4 => {
                let boxed = self.into_node4_box();
                for i in 0..boxed.header.count as usize {
                    boxed.children[i].drop_recursive();
                }
                drop(boxed);
            }
            KIND_NODE16 => {
                let boxed = self.into_node16_box();
                for i in 0..boxed.header.count as usize {
                    boxed.children[i].drop_recursive();
                }
                drop(boxed);
            }
            KIND_NODE48 => {
                let boxed = self.into_node48_box();
                for i in 0..256usize {
                    let idx = boxed.index[i];
                    if idx != 0xFF {
                        boxed.slots[idx as usize].drop_recursive();
                    }
                }
                drop(boxed);
            }
            KIND_NODE256 => {
                let boxed = self.into_node256_box();
                for i in 0..256usize {
                    if !boxed.children[i].is_null() {
                        boxed.children[i].drop_recursive();
                    }
                }
                drop(boxed);
            }
            _ => unreachable!(),
        }
    }
}

pub(crate) struct Leaf<V> {
    pub(crate) key: Box<[u8]>,
    pub(crate) value: V,
}

pub(crate) type InnerValue<V> = Option<(Box<[u8]>, V)>;

#[repr(C)]
pub(crate) struct NodeHeader<V> {
    pub(crate) prefix: Prefix,
    pub(crate) value: InnerValue<V>,
    pub(crate) count: u16,
}

#[repr(C)]
pub(crate) struct Node4<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) keys: [u8; 4],
    pub(crate) children: [NodePtr<V>; 4],
}

impl<V> Node4<V> {
    pub(crate) fn new() -> Self {
        Node4 {
            header: NodeHeader {
                prefix: Prefix::empty(),
                value: None,
                count: 0,
            },
            keys: [0; 4],
            children: [NodePtr::NULL; 4],
        }
    }
}

#[repr(C)]
pub(crate) struct Node16<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) keys: [u8; 16],
    pub(crate) children: [NodePtr<V>; 16],
}

impl<V> Node16<V> {
    pub(crate) fn new() -> Self {
        Node16 {
            header: NodeHeader {
                prefix: Prefix::empty(),
                value: None,
                count: 0,
            },
            keys: [0; 16],
            children: [NodePtr::NULL; 16],
        }
    }
}

#[repr(C)]
pub(crate) struct Node48<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) index: [u8; 256],
    pub(crate) slots: [NodePtr<V>; 48],
}

impl<V> Node48<V> {
    pub(crate) fn new() -> Self {
        Node48 {
            header: NodeHeader {
                prefix: Prefix::empty(),
                value: None,
                count: 0,
            },
            index: [0xFF; 256],
            slots: [NodePtr::NULL; 48],
        }
    }
}

#[repr(C)]
pub(crate) struct Node256<V> {
    pub(crate) header: NodeHeader<V>,
    pub(crate) children: [NodePtr<V>; 256],
}

impl<V> Node256<V> {
    pub(crate) fn new() -> Self {
        Node256 {
            header: NodeHeader {
                prefix: Prefix::empty(),
                value: None,
                count: 0,
            },
            children: [NodePtr::NULL; 256],
        }
    }
}

pub(crate) fn free_inner_node_shell<V>(node: NodePtr<V>) {
    match node.kind() {
        KIND_NODE4 => {
            let mut boxed = node.into_node4_box();
            boxed.header.count = 0;
            boxed.header.value = None;
            drop(boxed);
        }
        KIND_NODE16 => {
            let mut boxed = node.into_node16_box();
            boxed.header.count = 0;
            boxed.header.value = None;
            drop(boxed);
        }
        KIND_NODE48 => {
            let mut boxed = node.into_node48_box();
            boxed.header.count = 0;
            boxed.header.value = None;
            boxed.index = [0xFF; 256];
            drop(boxed);
        }
        KIND_NODE256 => {
            let mut boxed = node.into_node256_box();
            boxed.header.count = 0;
            boxed.header.value = None;
            for child in boxed.children.iter_mut() {
                *child = NodePtr::NULL;
            }
            drop(boxed);
        }
        _ => unreachable!(),
    }
}
