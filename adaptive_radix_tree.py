"""Adaptive Radix Tree (ART) implementation.

An ordered key-value map that uses adaptive node sizes (4, 16, 48, 256)
to balance memory usage and lookup speed.  Path compression collapses
single-child chains into node prefixes.

Public API
----------
    put(key, value)   – insert or update
    get(key)          – retrieve (None if missing)
    delete(key)       – remove; returns True / False
    items(from_key=None, to_key=None) – sorted iteration
"""

import bisect

# Sentinels ----------------------------------------------------------------

_EMPTY = object()          # "no value stored on this inner node"
_MISSING = object()        # internal "key not found" marker


# Helpers ------------------------------------------------------------------

def _to_bytes(key):
    """Encode a key to its internal byte representation."""
    if isinstance(key, str):
        return key.encode("utf-8")
    return bytes(key)


def _prefix_mismatch(a, b, a_off, b_off):
    """Length of the common prefix of *a[a_off:]* and *b[b_off:]*."""
    n = min(len(a) - a_off, len(b) - b_off)
    for i in range(n):
        if a[a_off + i] != b[b_off + i]:
            return i
    return n


# --------------------------------------------------------------------------
# Leaf
# --------------------------------------------------------------------------

class _Leaf:
    __slots__ = ("key", "key_bytes", "value")

    def __init__(self, key, key_bytes, value):
        self.key = key              # original key (for external use)
        self.key_bytes = key_bytes  # encoded bytes (for comparisons)
        self.value = value


# --------------------------------------------------------------------------
# Node types – each manages children differently
# --------------------------------------------------------------------------

class _Node4:
    __slots__ = ("prefix", "key", "key_bytes", "value", "_keys", "_children")
    MAX = 4

    def __init__(self):
        self.prefix = b""
        self.key = self.key_bytes = None
        self.value = _EMPTY
        self._keys = []
        self._children = []

    @property
    def size(self):
        return len(self._keys)

    def is_full(self):
        return len(self._keys) >= self.MAX

    def find(self, b):
        for i, k in enumerate(self._keys):
            if k == b:
                return self._children[i]
        return None

    def replace(self, b, child):
        for i, k in enumerate(self._keys):
            if k == b:
                self._children[i] = child
                return

    def add(self, b, child):
        pos = bisect.bisect_left(self._keys, b)
        self._keys.insert(pos, b)
        self._children.insert(pos, child)

    def remove(self, b):
        for i, k in enumerate(self._keys):
            if k == b:
                self._keys.pop(i)
                self._children.pop(i)
                return

    def children(self):
        """Return ``[(byte, child), ...]`` in sorted byte order."""
        return list(zip(self._keys, self._children))


class _Node16:
    __slots__ = ("prefix", "key", "key_bytes", "value", "_keys", "_children")
    MAX = 16

    def __init__(self):
        self.prefix = b""
        self.key = self.key_bytes = None
        self.value = _EMPTY
        self._keys = []
        self._children = []

    @property
    def size(self):
        return len(self._keys)

    def is_full(self):
        return len(self._keys) >= self.MAX

    def find(self, b):
        i = bisect.bisect_left(self._keys, b)
        if i < len(self._keys) and self._keys[i] == b:
            return self._children[i]
        return None

    def replace(self, b, child):
        i = bisect.bisect_left(self._keys, b)
        if i < len(self._keys) and self._keys[i] == b:
            self._children[i] = child

    def add(self, b, child):
        i = bisect.bisect_left(self._keys, b)
        self._keys.insert(i, b)
        self._children.insert(i, child)

    def remove(self, b):
        i = bisect.bisect_left(self._keys, b)
        if i < len(self._keys) and self._keys[i] == b:
            self._keys.pop(i)
            self._children.pop(i)

    def children(self):
        return list(zip(self._keys, self._children))


class _Node48:
    __slots__ = ("prefix", "key", "key_bytes", "value",
                 "_index", "_slots", "_count")
    MAX = 48
    _NONE = 0xFF

    def __init__(self):
        self.prefix = b""
        self.key = self.key_bytes = None
        self.value = _EMPTY
        self._index = bytearray([self._NONE] * 256)
        self._slots = [None] * 48
        self._count = 0

    @property
    def size(self):
        return self._count

    def is_full(self):
        return self._count >= self.MAX

    def find(self, b):
        i = self._index[b]
        return None if i == self._NONE else self._slots[i]

    def replace(self, b, child):
        self._slots[self._index[b]] = child

    def add(self, b, child):
        for j in range(48):
            if self._slots[j] is None:
                self._index[b] = j
                self._slots[j] = child
                self._count += 1
                return

    def remove(self, b):
        j = self._index[b]
        self._index[b] = self._NONE
        self._slots[j] = None
        self._count -= 1

    def children(self):
        out = []
        for b in range(256):
            j = self._index[b]
            if j != self._NONE:
                out.append((b, self._slots[j]))
        return out


class _Node256:
    __slots__ = ("prefix", "key", "key_bytes", "value",
                 "_children", "_count")
    MAX = 256

    def __init__(self):
        self.prefix = b""
        self.key = self.key_bytes = None
        self.value = _EMPTY
        self._children = [None] * 256
        self._count = 0

    @property
    def size(self):
        return self._count

    def is_full(self):
        return False

    def find(self, b):
        return self._children[b]

    def replace(self, b, child):
        self._children[b] = child

    def add(self, b, child):
        self._children[b] = child
        self._count += 1

    def remove(self, b):
        self._children[b] = None
        self._count -= 1

    def children(self):
        out = []
        for b in range(256):
            c = self._children[b]
            if c is not None:
                out.append((b, c))
        return out


# --------------------------------------------------------------------------
# Node growth / shrinkage
# --------------------------------------------------------------------------

_SHRINK_AT = {_Node256: 48, _Node48: 16, _Node16: 4}


def _copy_header(src, dst):
    dst.prefix = src.prefix
    dst.key = src.key
    dst.key_bytes = src.key_bytes
    dst.value = src.value


def _grow(node):
    if isinstance(node, _Node4):
        n = _Node16()
    elif isinstance(node, _Node16):
        n = _Node48()
    elif isinstance(node, _Node48):
        n = _Node256()
    else:
        raise RuntimeError("Node256 cannot grow")
    _copy_header(node, n)
    for b, c in node.children():
        n.add(b, c)
    return n


def _shrink(node):
    if isinstance(node, _Node256):
        n = _Node48()
    elif isinstance(node, _Node48):
        n = _Node16()
    elif isinstance(node, _Node16):
        n = _Node4()
    else:
        return node
    _copy_header(node, n)
    for b, c in node.children():
        n.add(b, c)
    return n


# --------------------------------------------------------------------------
# Recursive helpers
# --------------------------------------------------------------------------

def _put(node, key, kb, value, depth):
    """Insert *key* → *value*.  Returns ``(node, was_new_key)``."""

    # --- empty slot → new leaf ---
    if node is None:
        return _Leaf(key, kb, value), True

    # --- leaf ---
    if isinstance(node, _Leaf):
        if node.key_bytes == kb:
            node.key, node.value = key, value
            return node, False

        ekb = node.key_bytes
        common = _prefix_mismatch(kb, ekb, depth, depth)
        sd = depth + common  # split depth

        nn = _Node4()
        nn.prefix = kb[depth:sd]

        if sd == len(kb):
            # new key is a prefix of the existing key
            nn.key, nn.key_bytes, nn.value = key, kb, value
            nn.add(ekb[sd], node)
        elif sd == len(ekb):
            # existing key is a prefix of the new key
            nn.key, nn.key_bytes, nn.value = node.key, ekb, node.value
            nn.add(kb[sd], _Leaf(key, kb, value))
        else:
            nn.add(kb[sd], _Leaf(key, kb, value))
            nn.add(ekb[sd], node)
        return nn, True

    # --- inner node ---
    p = node.prefix
    plen = len(p)
    ml = _prefix_mismatch(kb, p, depth, 0)

    if ml < plen:
        # partial prefix match → split this node
        nn = _Node4()
        nn.prefix = p[:ml]

        node.prefix = p[ml + 1:]
        nn.add(p[ml], node)

        nd = depth + ml
        if nd == len(kb):
            nn.key, nn.key_bytes, nn.value = key, kb, value
        else:
            nn.add(kb[nd], _Leaf(key, kb, value))
        return nn, True

    # full prefix match
    nd = depth + plen

    if nd == len(kb):
        added = node.value is _EMPTY
        node.key, node.key_bytes, node.value = key, kb, value
        return node, added

    b = kb[nd]
    child = node.find(b)

    if child is None:
        if node.is_full():
            node = _grow(node)
        node.add(b, _Leaf(key, kb, value))
        return node, True

    new_child, added = _put(child, key, kb, value, nd + 1)
    if new_child is not child:
        node.replace(b, new_child)
    return node, added


def _delete(node, kb, depth):
    """Remove *kb*.  Returns ``(node_or_None, was_deleted)``."""

    if node is None:
        return None, False

    if isinstance(node, _Leaf):
        if node.key_bytes == kb:
            return None, True
        return node, False

    # inner node
    p = node.prefix
    plen = len(p)
    if kb[depth:depth + plen] != p:
        return node, False

    nd = depth + plen

    if nd == len(kb):
        # deleting the value stored on this inner node
        if node.value is _EMPTY:
            return node, False
        node.key = node.key_bytes = None
        node.value = _EMPTY
        return _compact(node), True

    b = kb[nd]
    child = node.find(b)
    if child is None:
        return node, False

    new_child, deleted = _delete(child, kb, nd + 1)
    if not deleted:
        return node, False

    if new_child is None:
        node.remove(b)
    else:
        node.replace(b, new_child)

    return _compact(node), True


def _compact(node):
    """Collapse a node that may have become degenerate after deletion."""
    s = node.size

    if s == 0:
        if node.value is not _EMPTY:
            return _Leaf(node.key, node.key_bytes, node.value)
        return None

    if s == 1 and node.value is _EMPTY:
        b, child = node.children()[0]
        if isinstance(child, _Leaf):
            return child
        # merge prefixes: parent.prefix + connecting byte + child.prefix
        child.prefix = node.prefix + bytes([b]) + child.prefix
        return child

    t = _SHRINK_AT.get(type(node))
    if t is not None and s <= t:
        return _shrink(node)

    return node


def _iter_all(node):
    """Yield all ``(key, value)`` pairs under *node* in sorted key order."""
    if node is None:
        return
    if isinstance(node, _Leaf):
        yield node.key, node.value
        return
    # inner node: own value first (shorter key sorts before children)
    if node.value is not _EMPTY:
        yield node.key, node.value
    for _, child in node.children():
        yield from _iter_all(child)


def _iter_range(node, depth, lo, hi):
    """Yield items in ``[lo, hi]`` from *node*'s subtree – O(log n + k).

    *lo* and *hi* are ``bytes`` or ``None`` (unconstrained).

    At every inner node the algorithm:

    1. Compares the node prefix with the remaining bound bytes to decide
       whether the entire subtree is outside the range (prune) or whether
       the bound is still "active" at the child level.
    2. Uses the next bound byte to skip children before *lo* or after *hi*.
    3. Passes the bound only to children on the exact boundary byte; all
       other in-range children are scanned unconditionally.

    This gives O(depth) boundary work plus O(k) for the k results –
    the same complexity class as a B-tree range scan.
    """
    if node is None:
        return

    if isinstance(node, _Leaf):
        kb = node.key_bytes
        if lo is not None and kb < lo:
            return
        if hi is not None and kb > hi:
            return
        yield node.key, node.value
        return

    # ── inner node ──────────────────────────────────────────────────
    p = node.prefix
    plen = len(p)
    nd = depth + plen           # depth after consuming prefix

    # ── lo boundary analysis ────────────────────────────────────────
    lo_on = False               # still tracking the lo boundary?
    if lo is not None:
        lo_avail = len(lo) - depth
        if lo_avail <= 0:
            # lo already consumed → everything here ≥ lo
            lo = None
        elif plen == 0:
            lo_on = True        # decide at child level
        else:
            cn = min(plen, lo_avail)
            pp = p[:cn]
            lp = lo[depth:depth + cn]
            if pp < lp:
                return          # whole subtree < lo
            if pp > lp:
                lo = None       # past lo → no lower constraint
            elif cn < plen:
                lo = None       # lo exhausted inside prefix → past lo
            elif lo_avail > plen:
                lo_on = True    # lo has more bytes → check children
            else:
                lo = None       # lo exhausted exactly at nd

    # ── hi boundary analysis ────────────────────────────────────────
    hi_on = False
    if hi is not None:
        hi_avail = len(hi) - depth
        if hi_avail <= 0:
            # hi exhausted → only this node's own value could match
            if node.value is not _EMPTY:
                kb = node.key_bytes
                if (lo is None or kb >= lo) and kb <= hi:
                    yield node.key, node.value
            return
        elif plen == 0:
            hi_on = True
        else:
            cn = min(plen, hi_avail)
            pp = p[:cn]
            hp = hi[depth:depth + cn]
            if pp > hp:
                return          # whole subtree > hi
            if pp < hp:
                hi = None       # before hi → no upper constraint
            elif cn < plen:
                return          # hi exhausted inside prefix → keys > hi
            elif hi_avail > plen:
                hi_on = True
            else:
                # hi exhausted at nd; own value may equal hi, children > hi
                if node.value is not _EMPTY:
                    kb = node.key_bytes
                    if (lo is None or kb >= lo) and kb <= hi:
                        yield node.key, node.value
                return

    # ── yield own value ─────────────────────────────────────────────
    if node.value is not _EMPTY:
        kb = node.key_bytes
        if (lo is None or kb >= lo) and (hi is None or kb <= hi):
            yield node.key, node.value

    # ── visit children with byte-level pruning ──────────────────────
    lo_byte = lo[nd] if lo_on else -1
    hi_byte = hi[nd] if hi_on else 256

    for byte, child in node.children():
        if byte < lo_byte:
            continue
        if byte > hi_byte:
            return
        child_lo = lo if lo_on and byte == lo_byte else None
        child_hi = hi if hi_on and byte == hi_byte else None
        yield from _iter_range(child, nd + 1, child_lo, child_hi)


# --------------------------------------------------------------------------
# Public API
# --------------------------------------------------------------------------

class AdaptiveRadixTree:
    """Ordered key-value store backed by an Adaptive Radix Tree.

    Keys must be strings.  Values may be any Python object.
    """

    def __init__(self):
        self._root = None
        self._len = 0

    def __len__(self):
        return self._len

    def __contains__(self, key):
        return self._lookup(key) is not _MISSING

    # -- put ---------------------------------------------------------------

    def put(self, key, value):
        """Insert or update *key* → *value*."""
        kb = _to_bytes(key)
        self._root, added = _put(self._root, key, kb, value, 0)
        if added:
            self._len += 1

    # -- get ---------------------------------------------------------------

    def get(self, key):
        """Return the value for *key*, or ``None`` if absent."""
        v = self._lookup(key)
        return None if v is _MISSING else v

    # -- delete ------------------------------------------------------------

    def delete(self, key):
        """Remove *key*.  Return ``True`` if it was present."""
        kb = _to_bytes(key)
        self._root, removed = _delete(self._root, kb, 0)
        if removed:
            self._len -= 1
        return removed

    # -- iterate -----------------------------------------------------------

    def items(self, from_key=None, to_key=None):
        """Yield ``(key, value)`` pairs in sorted key order.

        Parameters
        ----------
        from_key : str, optional
            Only yield keys ``>= from_key``.
        to_key : str, optional
            Only yield keys ``<= to_key``.
        """
        if from_key is None and to_key is None:
            yield from _iter_all(self._root)
        else:
            lo = _to_bytes(from_key) if from_key is not None else None
            hi = _to_bytes(to_key) if to_key is not None else None
            yield from _iter_range(self._root, 0, lo, hi)

    # -- internal ----------------------------------------------------------

    def _lookup(self, key):
        """Return the stored value or ``_MISSING``."""
        kb = _to_bytes(key)
        node = self._root
        depth = 0
        while node is not None:
            if isinstance(node, _Leaf):
                return node.value if node.key_bytes == kb else _MISSING
            p = node.prefix
            plen = len(p)
            if kb[depth:depth + plen] != p:
                return _MISSING
            depth += plen
            if depth == len(kb):
                return _MISSING if node.value is _EMPTY else node.value
            node = node.find(kb[depth])
            depth += 1
        return _MISSING
