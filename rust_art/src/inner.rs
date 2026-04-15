use crate::prefix::Prefix;
use crate::raw::{
    free_inner_node_shell, InnerValue, Leaf, Node16, Node256, Node4, Node48, NodePtr, KIND_NODE16,
    KIND_NODE256, KIND_NODE4, KIND_NODE48,
};

macro_rules! dispatch {
    ($node:expr, $method:ident $(, $arg:expr)*) => {
        match $node.kind() {
            KIND_NODE4 => $node.as_node4().$method($($arg),*),
            KIND_NODE16 => $node.as_node16().$method($($arg),*),
            KIND_NODE48 => $node.as_node48().$method($($arg),*),
            KIND_NODE256 => $node.as_node256().$method($($arg),*),
            _ => unreachable!(),
        }
    };
}

macro_rules! dispatch_mut {
    ($node:expr, $method:ident $(, $arg:expr)*) => {
        match $node.kind() {
            KIND_NODE4 => $node.as_node4_mut().$method($($arg),*),
            KIND_NODE16 => $node.as_node16_mut().$method($($arg),*),
            KIND_NODE48 => $node.as_node48_mut().$method($($arg),*),
            KIND_NODE256 => $node.as_node256_mut().$method($($arg),*),
            _ => unreachable!(),
        }
    };
}

pub(crate) unsafe fn inner_prefix_raw<'a, V>(node: NodePtr<V>) -> &'a [u8] {
    let header = &*(node.inner_ptr() as *const crate::raw::NodeHeader<V>);
    header.prefix.as_slice()
}

pub(crate) unsafe fn inner_value_raw<'a, V>(node: NodePtr<V>) -> Option<(&'a [u8], &'a V)> {
    let header = &*(node.inner_ptr() as *const crate::raw::NodeHeader<V>);
    let opt = &header.value;
    opt.as_ref().map(|(key, value)| (&**key, value))
}

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

pub(crate) fn inner_find<V>(node: NodePtr<V>, b: u8) -> NodePtr<V> {
    dispatch!(node, find_child, b)
}

pub(crate) fn prefix_mismatch(a: &[u8], a_off: usize, b: &[u8], b_off: usize) -> usize {
    let n = (a.len() - a_off).min(b.len() - b_off);
    for i in 0..n {
        if a[a_off + i] != b[b_off + i] {
            return i;
        }
    }
    n
}

pub(crate) fn inner_add_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    dispatch_mut!(node, add_child, b, child)
}

pub(crate) fn inner_replace_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    dispatch_mut!(node, replace_child, b, child)
}

pub(crate) fn inner_is_full<V>(node: &NodePtr<V>) -> bool {
    match node.kind() {
        KIND_NODE4 => node.header().count >= 4,
        KIND_NODE16 => node.header().count >= 16,
        KIND_NODE48 => node.header().count >= 48,
        KIND_NODE256 => false,
        _ => unreachable!(),
    }
}

pub(crate) fn inner_count<V>(node: &NodePtr<V>) -> usize {
    node.header().count as usize
}

pub(crate) fn inner_set_prefix<V>(node: &mut NodePtr<V>, prefix: Prefix) {
    node.header_mut().prefix = prefix;
}

pub(crate) fn inner_set_value<V>(node: &mut NodePtr<V>, key: Box<[u8]>, value: V) {
    node.header_mut().value = Some((key, value));
}

pub(crate) fn inner_has_value<V>(node: &NodePtr<V>) -> bool {
    node.header().value.is_some()
}

pub(crate) fn inner_clear_value<V>(node: &mut NodePtr<V>) -> InnerValue<V> {
    node.header_mut().value.take()
}

pub(crate) fn inner_take_prefix<V>(node: &mut NodePtr<V>) -> Prefix {
    std::mem::take(&mut node.header_mut().prefix)
}

pub(crate) fn grow<V>(node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => Node4::grow(node),
        KIND_NODE16 => Node16::grow(node),
        KIND_NODE48 => Node48::grow(node),
        _ => unreachable!("Node256 cannot grow"),
    }
}

pub(crate) fn shrink<V>(node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE256 => Node256::shrink(node),
        KIND_NODE48 => Node48::shrink(node),
        KIND_NODE16 => Node16::shrink(node),
        _ => node,
    }
}

pub(crate) fn inner_remove_child<V>(node: &mut NodePtr<V>, b: u8) {
    dispatch_mut!(node, remove_child, b)
}

pub(crate) fn inner_children<V>(node: &NodePtr<V>) -> Vec<(u8, NodePtr<V>)> {
    dispatch!(node, get_children)
}

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
