use crate::inner::{inner_prefix_raw, inner_value_in_lex_range, inner_value_raw};
use crate::raw::{Leaf, NodePtr, KIND_NODE16, KIND_NODE256, KIND_NODE4, KIND_NODE48, TAG_MASK};

pub struct Iter<'a, V> {
    stack: Vec<NodePtr<V>>,
    _marker: std::marker::PhantomData<&'a V>,
}

impl<'a, V> Iter<'a, V> {
    pub(crate) fn new(root: NodePtr<V>) -> Self {
        let mut stack = Vec::new();
        if !root.is_null() {
            stack.push(root);
        }
        Iter {
            stack,
            _marker: std::marker::PhantomData,
        }
    }
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
            if let Some((key, value)) = unsafe { inner_value_raw(node) } {
                return Some((key, value));
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

pub(crate) struct RangeFrame<'a, V> {
    node: NodePtr<V>,
    depth: usize,
    lo: Option<&'a [u8]>,
    hi: Option<&'a [u8]>,
}

pub struct RangeIter<'a, V> {
    stack: Vec<RangeFrame<'a, V>>,
    _marker: std::marker::PhantomData<&'a V>,
}

enum LowerBound<'a> {
    Prune,
    Free,
    Follow(&'a [u8]),
}

enum UpperBound<'a> {
    Prune,
    Free,
    Follow(&'a [u8]),
    OwnOnly(&'a [u8]),
}

impl<'a, V> RangeIter<'a, V> {
    pub(crate) fn new(root: NodePtr<V>, lo: Option<&'a [u8]>, hi: Option<&'a [u8]>) -> Self {
        let mut stack = Vec::new();
        if !root.is_null() {
            stack.push(RangeFrame {
                node: root,
                depth: 0,
                lo,
                hi,
            });
        }
        RangeIter {
            stack,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, V> Iterator for RangeIter<'a, V> {
    type Item = (&'a [u8], &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.pop()?;
            let node = frame.node;
            let depth = frame.depth;
            let mut lo = frame.lo;
            let mut hi = frame.hi;

            if node.is_leaf() {
                let leaf = unsafe { &*((node.0 & !TAG_MASK) as *const Leaf<V>) };
                let kb = &leaf.key[..];
                if lo.map_or(true, |lo| kb >= lo) && hi.map_or(true, |hi| kb <= hi) {
                    return Some((kb, &leaf.value));
                }
                continue;
            }

            let prefix = unsafe { inner_prefix_raw(node) };
            let plen = prefix.len();
            let nd = depth + plen;

            let lo_on = match lower_bound(prefix, depth, lo) {
                LowerBound::Prune => continue,
                LowerBound::Free => {
                    lo = None;
                    false
                }
                LowerBound::Follow(lo_bytes) => {
                    lo = Some(lo_bytes);
                    true
                }
            };

            let hi_on = match upper_bound(prefix, depth, hi) {
                UpperBound::Prune => continue,
                UpperBound::Free => {
                    hi = None;
                    false
                }
                UpperBound::Follow(hi_bytes) => {
                    hi = Some(hi_bytes);
                    true
                }
                UpperBound::OwnOnly(hi_bytes) => {
                    if let Some((kb, v)) =
                        unsafe { inner_value_in_lex_range(node, lo, Some(hi_bytes)) }
                    {
                        return Some((kb, v));
                    }
                    continue;
                }
            };

            let lo_byte: i16 = if lo_on { lo.unwrap()[nd] as i16 } else { -1 };
            let hi_byte: i16 = if hi_on { hi.unwrap()[nd] as i16 } else { 256 };

            push_range_children_rev(
                node,
                nd + 1,
                lo_byte,
                hi_byte,
                lo_on,
                lo,
                hi_on,
                hi,
                &mut self.stack,
            );

            if let Some((kb, v)) = unsafe { inner_value_in_lex_range(node, lo, hi) } {
                return Some((kb, v));
            }
        }
    }
}

fn lower_bound<'a>(prefix: &[u8], depth: usize, lo: Option<&'a [u8]>) -> LowerBound<'a> {
    let Some(lo_bytes) = lo else {
        return LowerBound::Free;
    };
    let lo_avail = lo_bytes.len().saturating_sub(depth);
    if lo_avail == 0 {
        return LowerBound::Free;
    }
    if prefix.is_empty() {
        return LowerBound::Follow(lo_bytes);
    }

    let cn = prefix.len().min(lo_avail);
    let pp = &prefix[..cn];
    let lp = &lo_bytes[depth..depth + cn];
    if pp < lp {
        LowerBound::Prune
    } else if pp > lp || cn < prefix.len() || lo_avail == prefix.len() {
        LowerBound::Free
    } else {
        LowerBound::Follow(lo_bytes)
    }
}

fn upper_bound<'a>(prefix: &[u8], depth: usize, hi: Option<&'a [u8]>) -> UpperBound<'a> {
    let Some(hi_bytes) = hi else {
        return UpperBound::Free;
    };
    let hi_avail = hi_bytes.len().saturating_sub(depth);
    if hi_avail == 0 {
        return UpperBound::OwnOnly(hi_bytes);
    }
    if prefix.is_empty() {
        return UpperBound::Follow(hi_bytes);
    }

    let cn = prefix.len().min(hi_avail);
    let pp = &prefix[..cn];
    let hp = &hi_bytes[depth..depth + cn];
    if pp > hp || cn < prefix.len() {
        UpperBound::Prune
    } else if pp < hp {
        UpperBound::Free
    } else if hi_avail == prefix.len() {
        UpperBound::OwnOnly(hi_bytes)
    } else {
        UpperBound::Follow(hi_bytes)
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
        if b < lo_byte || b > hi_byte {
            return;
        }
        let child_lo = if lo_on && b == lo_byte { lo } else { None };
        let child_hi = if hi_on && b == hi_byte { hi } else { None };
        stack.push(RangeFrame {
            node: child,
            depth: child_depth,
            lo: child_lo,
            hi: child_hi,
        });
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
