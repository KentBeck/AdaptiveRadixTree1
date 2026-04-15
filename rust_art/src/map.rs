use crate::inner::{
    compact, grow, inner_add_child, inner_clear_value, inner_find, inner_has_value, inner_is_full,
    inner_prefix_raw, inner_remove_child, inner_replace_child, inner_set_prefix, inner_set_value,
    prefix_mismatch,
};
use crate::iter::{Iter, RangeIter};
use crate::prefix::Prefix;
use crate::raw::{Leaf, Node4, NodePtr};

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
        let (new_root, added) = put_recursive(self.root, key, value, 0);
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
        unsafe { self.get_inner(key) }
    }

    unsafe fn get_inner(&self, key: &[u8]) -> Option<&V> {
        let mut node = self.root;
        let mut depth = 0;

        while !node.is_null() {
            if node.is_leaf() {
                let leaf = node.as_leaf();
                if leaf.matches(key) {
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

    pub fn items(&self) -> Vec<(&[u8], &V)> {
        self.iter().collect()
    }

    pub fn range<'a>(
        &'a self,
        from_key: Option<&'a [u8]>,
        to_key: Option<&'a [u8]>,
    ) -> Vec<(&'a [u8], &'a V)> {
        self.range_iter(from_key, to_key).collect()
    }

    pub fn iter(&self) -> Iter<'_, V> {
        Iter::new(self.root)
    }

    pub fn range_iter<'a>(
        &'a self,
        lo: Option<&'a [u8]>,
        hi: Option<&'a [u8]>,
    ) -> RangeIter<'a, V> {
        RangeIter::new(self.root, lo, hi)
    }
}

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

fn leaf_ptr<V>(key: &[u8], value: V) -> NodePtr<V> {
    NodePtr::from_leaf(Box::new(Leaf {
        key: Box::from(key),
        value,
    }))
}

fn put_recursive<V>(node: NodePtr<V>, key: &[u8], value: V, depth: usize) -> (NodePtr<V>, bool) {
    if node.is_null() {
        return (leaf_ptr(key, value), true);
    }

    if node.is_leaf() {
        let existing = node.as_leaf();
        if existing.matches(key) {
            let mut leaf_box = node.into_leaf_box();
            leaf_box.value = value;
            return (NodePtr::from_leaf(leaf_box), false);
        }

        let existing_key = &existing.key;
        let common = prefix_mismatch(key, depth, existing_key, depth);
        let sd = depth + common;

        let mut nn = Box::new(Node4::<V>::new());
        nn.header.prefix = Prefix::from_slice(&key[depth..sd]);

        let mut nn_ptr = NodePtr::from_node4(nn);

        if sd == key.len() {
            inner_set_value(&mut nn_ptr, Box::from(key), value);
            inner_add_child(&mut nn_ptr, existing_key[sd], node);
        } else if sd == existing_key.len() {
            let existing_box = node.into_leaf_box();
            inner_set_value(&mut nn_ptr, existing_box.key, existing_box.value);
            inner_add_child(&mut nn_ptr, key[sd], leaf_ptr(key, value));
        } else {
            let new_b = key[sd];
            let old_b = existing_key[sd];
            inner_add_child(&mut nn_ptr, new_b, leaf_ptr(key, value));
            inner_add_child(&mut nn_ptr, old_b, node);
        }
        return (nn_ptr, true);
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
            inner_add_child(&mut nn_ptr, key[nd], leaf_ptr(key, value));
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
        inner_add_child(&mut node, b, leaf_ptr(key, value));
        return (node, true);
    }

    let (new_child, added) = put_recursive(child, key, value, nd + 1);
    if new_child.0 != child.0 {
        inner_replace_child(&mut node, b, new_child);
    }
    (node, added)
}

impl<V> Drop for ARTMap<V> {
    fn drop(&mut self) {
        unsafe {
            self.root.drop_recursive();
        }
    }
}
