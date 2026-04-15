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

    pub(crate) fn as_leaf<'a>(self) -> &'a Leaf<V> {
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

impl<V> NodeHeader<V> {
    pub(crate) fn move_to(&mut self, dst: &mut NodeHeader<V>) {
        dst.prefix = std::mem::take(&mut self.prefix);
        dst.value = self.value.take();
    }
}

impl<V> Node4<V> {
    pub(crate) fn find_child(&self, b: u8) -> NodePtr<V> {
        for i in 0..self.header.count as usize {
            if self.keys[i] == b {
                return self.children[i];
            }
        }
        NodePtr::NULL
    }

    pub(crate) fn add_child(&mut self, b: u8, child: NodePtr<V>) {
        let cnt = self.header.count as usize;
        let pos = self.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
        for i in (pos..cnt).rev() {
            self.keys[i + 1] = self.keys[i];
            self.children[i + 1] = self.children[i];
        }
        self.keys[pos] = b;
        self.children[pos] = child;
        self.header.count += 1;
    }

    pub(crate) fn replace_child(&mut self, b: u8, child: NodePtr<V>) {
        for i in 0..self.header.count as usize {
            if self.keys[i] == b {
                self.children[i] = child;
                return;
            }
        }
    }

    pub(crate) fn remove_child(&mut self, b: u8) {
        let cnt = self.header.count as usize;
        if let Some(pos) = self.keys[..cnt].iter().position(|&k| k == b) {
            for i in pos..cnt - 1 {
                self.keys[i] = self.keys[i + 1];
                self.children[i] = self.children[i + 1];
            }
            self.children[cnt - 1] = NodePtr::NULL;
            self.header.count -= 1;
        }
    }

    pub(crate) fn get_children(&self) -> Vec<(u8, NodePtr<V>)> {
        let cnt = self.header.count as usize;
        (0..cnt).map(|i| (self.keys[i], self.children[i])).collect()
    }

    pub(crate) fn grow(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node16(Box::new(Node16::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
        let old = node.as_node4();
        let cnt = old.header.count as usize;
        {
            let dst = new_ptr.as_node16_mut();
            dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
            dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
            dst.header.count = cnt as u16;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }
}

impl<V> Node16<V> {
    pub(crate) fn find_child(&self, b: u8) -> NodePtr<V> {
        let cnt = self.header.count as usize;
        match self.keys[..cnt].binary_search(&b) {
            Ok(i) => self.children[i],
            Err(_) => NodePtr::NULL,
        }
    }

    pub(crate) fn add_child(&mut self, b: u8, child: NodePtr<V>) {
        let cnt = self.header.count as usize;
        let pos = self.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
        for i in (pos..cnt).rev() {
            self.keys[i + 1] = self.keys[i];
            self.children[i + 1] = self.children[i];
        }
        self.keys[pos] = b;
        self.children[pos] = child;
        self.header.count += 1;
    }

    pub(crate) fn replace_child(&mut self, b: u8, child: NodePtr<V>) {
        let cnt = self.header.count as usize;
        if let Ok(i) = self.keys[..cnt].binary_search(&b) {
            self.children[i] = child;
        }
    }

    pub(crate) fn remove_child(&mut self, b: u8) {
        let cnt = self.header.count as usize;
        if let Ok(pos) = self.keys[..cnt].binary_search(&b) {
            for i in pos..cnt - 1 {
                self.keys[i] = self.keys[i + 1];
                self.children[i] = self.children[i + 1];
            }
            self.children[cnt - 1] = NodePtr::NULL;
            self.header.count -= 1;
        }
    }

    pub(crate) fn get_children(&self) -> Vec<(u8, NodePtr<V>)> {
        let cnt = self.header.count as usize;
        (0..cnt).map(|i| (self.keys[i], self.children[i])).collect()
    }

    pub(crate) fn grow(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
        let old = node.as_node16();
        let cnt = old.header.count as usize;
        {
            let dst = new_ptr.as_node48_mut();
            for i in 0..cnt {
                let b = old.keys[i];
                dst.index[b as usize] = i as u8;
                dst.slots[i] = old.children[i];
            }
            dst.header.count = cnt as u16;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }

    pub(crate) fn shrink(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node4(Box::new(Node4::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
        let old = node.as_node16();
        let cnt = old.header.count as usize;
        {
            let dst = new_ptr.as_node4_mut();
            dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
            dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
            dst.header.count = cnt as u16;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }
}

impl<V> Node48<V> {
    pub(crate) fn find_child(&self, b: u8) -> NodePtr<V> {
        let idx = self.index[b as usize];
        if idx == 0xFF {
            NodePtr::NULL
        } else {
            self.slots[idx as usize]
        }
    }

    pub(crate) fn add_child(&mut self, b: u8, child: NodePtr<V>) {
        let slot = (0u8..48)
            .find(|&j| self.slots[j as usize].is_null())
            .unwrap();
        self.index[b as usize] = slot;
        self.slots[slot as usize] = child;
        self.header.count += 1;
    }

    pub(crate) fn replace_child(&mut self, b: u8, child: NodePtr<V>) {
        let idx = self.index[b as usize];
        self.slots[idx as usize] = child;
    }

    pub(crate) fn remove_child(&mut self, b: u8) {
        let idx = self.index[b as usize];
        if idx != 0xFF {
            self.slots[idx as usize] = NodePtr::NULL;
            self.index[b as usize] = 0xFF;
            self.header.count -= 1;
        }
    }

    pub(crate) fn get_children(&self) -> Vec<(u8, NodePtr<V>)> {
        let mut out = Vec::new();
        for b in 0..256usize {
            let idx = self.index[b];
            if idx != 0xFF {
                out.push((b as u8, self.slots[idx as usize]));
            }
        }
        out
    }

    pub(crate) fn grow(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node256(Box::new(Node256::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
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
            dst.header.count = cnt;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }

    pub(crate) fn shrink(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node16(Box::new(Node16::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
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
            dst.header.count = cnt as u16;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }
}

impl<V> Node256<V> {
    pub(crate) fn find_child(&self, b: u8) -> NodePtr<V> {
        self.children[b as usize]
    }

    pub(crate) fn add_child(&mut self, b: u8, child: NodePtr<V>) {
        self.children[b as usize] = child;
        self.header.count += 1;
    }

    pub(crate) fn replace_child(&mut self, b: u8, child: NodePtr<V>) {
        self.children[b as usize] = child;
    }

    pub(crate) fn remove_child(&mut self, b: u8) {
        if !self.children[b as usize].is_null() {
            self.children[b as usize] = NodePtr::NULL;
            self.header.count -= 1;
        }
    }

    pub(crate) fn get_children(&self) -> Vec<(u8, NodePtr<V>)> {
        let mut out = Vec::new();
        for b in 0..256usize {
            if !self.children[b].is_null() {
                out.push((b as u8, self.children[b]));
            }
        }
        out
    }

    pub(crate) fn shrink(mut node: NodePtr<V>) -> NodePtr<V> {
        let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
        node.header_mut().move_to(new_ptr.header_mut());
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
            dst.header.count = slot as u16;
        }
        crate::raw::free_inner_node_shell(node);
        new_ptr
    }
}
