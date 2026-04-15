use crate::prefix::Prefix;
use crate::raw::{
    free_inner_node_shell, InnerValue, Leaf, Node16, Node256, Node4, Node48, NodePtr, KIND_NODE16,
    KIND_NODE256, KIND_NODE4, KIND_NODE48,
};

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
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            for i in 0..n.header.count as usize {
                if n.keys[i] == b {
                    return n.children[i];
                }
            }
            NodePtr::NULL
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            let cnt = n.header.count as usize;
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
        KIND_NODE256 => node.as_node256().children[b as usize],
        _ => unreachable!(),
    }
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
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4_mut();
            let cnt = n.header.count as usize;
            let pos = n.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
            for i in (pos..cnt).rev() {
                n.keys[i + 1] = n.keys[i];
                n.children[i + 1] = n.children[i];
            }
            n.keys[pos] = b;
            n.children[pos] = child;
            n.header.count += 1;
        }
        KIND_NODE16 => {
            let n = node.as_node16_mut();
            let cnt = n.header.count as usize;
            let pos = n.keys[..cnt].iter().position(|&k| k > b).unwrap_or(cnt);
            for i in (pos..cnt).rev() {
                n.keys[i + 1] = n.keys[i];
                n.children[i + 1] = n.children[i];
            }
            n.keys[pos] = b;
            n.children[pos] = child;
            n.header.count += 1;
        }
        KIND_NODE48 => {
            let n = node.as_node48_mut();
            let slot = (0u8..48).find(|&j| n.slots[j as usize].is_null()).unwrap();
            n.index[b as usize] = slot;
            n.slots[slot as usize] = child;
            n.header.count += 1;
        }
        KIND_NODE256 => {
            let n = node.as_node256_mut();
            n.children[b as usize] = child;
            n.header.count += 1;
        }
        _ => unreachable!(),
    }
}

pub(crate) fn inner_replace_child<V>(node: &mut NodePtr<V>, b: u8, child: NodePtr<V>) {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4_mut();
            for i in 0..n.header.count as usize {
                if n.keys[i] == b {
                    n.children[i] = child;
                    return;
                }
            }
        }
        KIND_NODE16 => {
            let n = node.as_node16_mut();
            let cnt = n.header.count as usize;
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

pub(crate) fn inner_move_header<V>(src: &mut NodePtr<V>, dst: &mut NodePtr<V>) {
    let prefix = inner_take_prefix(src);
    let value = inner_clear_value(src);
    inner_set_prefix(dst, prefix);
    dst.header_mut().value = value;
}

pub(crate) fn grow<V>(mut node: NodePtr<V>) -> NodePtr<V> {
    match node.kind() {
        KIND_NODE4 => {
            let mut new_ptr = NodePtr::from_node16(Box::new(Node16::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node4();
            let cnt = old.header.count as usize;
            {
                let dst = new_ptr.as_node16_mut();
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.header.count = cnt as u16;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE16 => {
            let mut new_ptr = NodePtr::from_node48(Box::new(Node48::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
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
                dst.header.count = cnt;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        _ => unreachable!("Node256 cannot grow"),
    }
}

pub(crate) fn shrink<V>(mut node: NodePtr<V>) -> NodePtr<V> {
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
                dst.header.count = slot as u16;
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
                dst.header.count = cnt as u16;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        KIND_NODE16 => {
            let mut new_ptr = NodePtr::from_node4(Box::new(Node4::<V>::new()));
            inner_move_header(&mut node, &mut new_ptr);
            let old = node.as_node16();
            let cnt = old.header.count as usize;
            {
                let dst = new_ptr.as_node4_mut();
                dst.keys[..cnt].copy_from_slice(&old.keys[..cnt]);
                dst.children[..cnt].copy_from_slice(&old.children[..cnt]);
                dst.header.count = cnt as u16;
            }
            free_inner_node_shell(node);
            new_ptr
        }
        _ => node,
    }
}

pub(crate) fn inner_remove_child<V>(node: &mut NodePtr<V>, b: u8) {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4_mut();
            let cnt = n.header.count as usize;
            if let Some(pos) = n.keys[..cnt].iter().position(|&k| k == b) {
                for i in pos..cnt - 1 {
                    n.keys[i] = n.keys[i + 1];
                    n.children[i] = n.children[i + 1];
                }
                n.children[cnt - 1] = NodePtr::NULL;
                n.header.count -= 1;
            }
        }
        KIND_NODE16 => {
            let n = node.as_node16_mut();
            let cnt = n.header.count as usize;
            if let Ok(pos) = n.keys[..cnt].binary_search(&b) {
                for i in pos..cnt - 1 {
                    n.keys[i] = n.keys[i + 1];
                    n.children[i] = n.children[i + 1];
                }
                n.children[cnt - 1] = NodePtr::NULL;
                n.header.count -= 1;
            }
        }
        KIND_NODE48 => {
            let n = node.as_node48_mut();
            let idx = n.index[b as usize];
            if idx != 0xFF {
                n.slots[idx as usize] = NodePtr::NULL;
                n.index[b as usize] = 0xFF;
                n.header.count -= 1;
            }
        }
        KIND_NODE256 => {
            let n = node.as_node256_mut();
            if !n.children[b as usize].is_null() {
                n.children[b as usize] = NodePtr::NULL;
                n.header.count -= 1;
            }
        }
        _ => unreachable!(),
    }
}

pub(crate) fn inner_children<V>(node: &NodePtr<V>) -> Vec<(u8, NodePtr<V>)> {
    match node.kind() {
        KIND_NODE4 => {
            let n = node.as_node4();
            let cnt = n.header.count as usize;
            (0..cnt).map(|i| (n.keys[i], n.children[i])).collect()
        }
        KIND_NODE16 => {
            let n = node.as_node16();
            let cnt = n.header.count as usize;
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
