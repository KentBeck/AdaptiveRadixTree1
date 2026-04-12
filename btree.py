"""B-Tree implementation with put/get/delete/items API.

Used as a baseline for benchmarking against the Adaptive Radix Tree.
"""

import bisect


class _Node:
    __slots__ = ("keys", "values", "children")

    def __init__(self):
        self.keys = []
        self.values = []
        self.children = []       # empty → leaf

    @property
    def is_leaf(self):
        return not self.children


class BTree:
    """Ordered key-value store backed by a B-Tree.

    Parameters
    ----------
    order : int
        Maximum number of children per node (max keys = order - 1).
    """

    def __init__(self, order=256):
        self._max_keys = order - 1
        self._min_keys = (order - 1) // 2
        self._root = _Node()
        self._len = 0

    def __len__(self):
        return self._len

    def __contains__(self, key):
        return self._find(key) is not None

    # -- put ---------------------------------------------------------------

    def put(self, key, value):
        root = self._root
        if len(root.keys) >= self._max_keys:
            new_root = _Node()
            new_root.children.append(root)
            self._split_child(new_root, 0)
            self._root = new_root
        if self._insert_nonfull(self._root, key, value):
            self._len += 1

    # -- get ---------------------------------------------------------------

    def get(self, key):
        r = self._find(key)
        return r if r is not None else None

    # -- delete ------------------------------------------------------------

    def delete(self, key):
        if self._root.keys or not self._root.is_leaf:
            deleted = self._delete(self._root, key)
        else:
            deleted = False
        if deleted:
            self._len -= 1
            if not self._root.keys and not self._root.is_leaf:
                self._root = self._root.children[0]
        return deleted

    # -- items -------------------------------------------------------------

    def items(self, from_key=None, to_key=None):
        yield from self._iter(self._root, from_key, to_key)

    # ======================================================================
    # Internal helpers
    # ======================================================================

    def _find(self, key):
        node = self._root
        while True:
            i = bisect.bisect_left(node.keys, key)
            if i < len(node.keys) and node.keys[i] == key:
                return node.values[i]
            if node.is_leaf:
                return None
            node = node.children[i]

    # -- insertion ---------------------------------------------------------

    def _split_child(self, parent, idx):
        child = parent.children[idx]
        mid = len(child.keys) // 2

        right = _Node()
        right.keys = child.keys[mid + 1:]
        right.values = child.values[mid + 1:]
        if not child.is_leaf:
            right.children = child.children[mid + 1:]

        med_k = child.keys[mid]
        med_v = child.values[mid]

        child.keys = child.keys[:mid]
        child.values = child.values[:mid]
        if not child.is_leaf:
            child.children = child.children[:mid + 1]

        parent.keys.insert(idx, med_k)
        parent.values.insert(idx, med_v)
        parent.children.insert(idx + 1, right)

    def _insert_nonfull(self, node, key, value):
        i = bisect.bisect_left(node.keys, key)
        if i < len(node.keys) and node.keys[i] == key:
            node.values[i] = value
            return False

        if node.is_leaf:
            node.keys.insert(i, key)
            node.values.insert(i, value)
            return True

        if len(node.children[i].keys) >= self._max_keys:
            self._split_child(node, i)
            if key == node.keys[i]:
                node.values[i] = value
                return False
            if key > node.keys[i]:
                i += 1
        return self._insert_nonfull(node.children[i], key, value)

    # -- deletion ----------------------------------------------------------

    def _delete(self, node, key):
        i = bisect.bisect_left(node.keys, key)

        if i < len(node.keys) and node.keys[i] == key:
            # Found the key in this node
            if node.is_leaf:
                node.keys.pop(i)
                node.values.pop(i)
                return True

            left = node.children[i]
            right = node.children[i + 1]

            if len(left.keys) > self._min_keys:
                pk, pv = self._max_key(left)
                node.keys[i], node.values[i] = pk, pv
                return self._delete(left, pk)

            if len(right.keys) > self._min_keys:
                sk, sv = self._min_key(right)
                node.keys[i], node.values[i] = sk, sv
                return self._delete(right, sk)

            self._merge(node, i)
            return self._delete(node.children[i], key)

        # Key not in this node
        if node.is_leaf:
            return False

        if len(node.children[i].keys) <= self._min_keys:
            self._ensure_min(node, i)
            return self._delete(node, key)

        return self._delete(node.children[i], key)

    @staticmethod
    def _max_key(node):
        while not node.is_leaf:
            node = node.children[-1]
        return node.keys[-1], node.values[-1]

    @staticmethod
    def _min_key(node):
        while not node.is_leaf:
            node = node.children[0]
        return node.keys[0], node.values[0]

    def _merge(self, parent, idx):
        left = parent.children[idx]
        right = parent.children[idx + 1]
        left.keys.append(parent.keys.pop(idx))
        left.values.append(parent.values.pop(idx))
        left.keys.extend(right.keys)
        left.values.extend(right.values)
        left.children.extend(right.children)
        parent.children.pop(idx + 1)

    def _ensure_min(self, parent, idx):
        if (idx > 0
                and len(parent.children[idx - 1].keys) > self._min_keys):
            self._rotate_right(parent, idx)
        elif (idx < len(parent.children) - 1
              and len(parent.children[idx + 1].keys) > self._min_keys):
            self._rotate_left(parent, idx)
        elif idx > 0:
            self._merge(parent, idx - 1)
        else:
            self._merge(parent, idx)

    @staticmethod
    def _rotate_right(parent, idx):
        child = parent.children[idx]
        left = parent.children[idx - 1]
        child.keys.insert(0, parent.keys[idx - 1])
        child.values.insert(0, parent.values[idx - 1])
        parent.keys[idx - 1] = left.keys.pop()
        parent.values[idx - 1] = left.values.pop()
        if not child.is_leaf:
            child.children.insert(0, left.children.pop())

    @staticmethod
    def _rotate_left(parent, idx):
        child = parent.children[idx]
        right = parent.children[idx + 1]
        child.keys.append(parent.keys[idx])
        child.values.append(parent.values[idx])
        parent.keys[idx] = right.keys.pop(0)
        parent.values[idx] = right.values.pop(0)
        if not child.is_leaf:
            child.children.append(right.children.pop(0))

    # -- iteration ---------------------------------------------------------

    def _iter(self, node, lo, hi):
        start = bisect.bisect_left(node.keys, lo) if lo is not None else 0

        for i in range(start, len(node.keys)):
            if not node.is_leaf:
                yield from self._iter(node.children[i], lo, hi)
            k = node.keys[i]
            if hi is not None and k > hi:
                return
            yield k, node.values[i]

        if not node.is_leaf:
            yield from self._iter(node.children[len(node.keys)], lo, hi)
