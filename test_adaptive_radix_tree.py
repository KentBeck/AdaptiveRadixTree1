"""Comprehensive tests for the Adaptive Radix Tree."""

import random
import string

import pytest

from adaptive_radix_tree import AdaptiveRadixTree


# ── helpers ───────────────────────────────────────────────────────────────

def tree_from(*pairs):
    t = AdaptiveRadixTree()
    for k, v in pairs:
        t.put(k, v)
    return t


# ── empty tree ────────────────────────────────────────────────────────────

class TestEmptyTree:
    def test_len_is_zero(self):
        assert len(AdaptiveRadixTree()) == 0

    def test_get_returns_none(self):
        assert AdaptiveRadixTree().get("x") is None

    def test_delete_returns_false(self):
        assert AdaptiveRadixTree().delete("x") is False

    def test_items_yields_nothing(self):
        assert list(AdaptiveRadixTree().items()) == []

    def test_contains_is_false(self):
        assert "x" not in AdaptiveRadixTree()


# ── single-key operations ────────────────────────────────────────────────

class TestSingleKey:
    def test_put_and_get(self):
        t = AdaptiveRadixTree()
        t.put("hello", 42)
        assert t.get("hello") == 42
        assert len(t) == 1

    def test_overwrite(self):
        t = AdaptiveRadixTree()
        t.put("k", 1)
        t.put("k", 2)
        assert t.get("k") == 2
        assert len(t) == 1

    def test_delete(self):
        t = AdaptiveRadixTree()
        t.put("k", 1)
        assert t.delete("k") is True
        assert t.get("k") is None
        assert len(t) == 0

    def test_delete_missing_after_real_delete(self):
        t = tree_from(("a", 1))
        t.delete("a")
        assert t.delete("a") is False


# ── multiple keys ────────────────────────────────────────────────────────

class TestMultipleKeys:
    def test_independent_keys(self):
        t = tree_from(("a", 1), ("b", 2), ("c", 3))
        assert t.get("a") == 1
        assert t.get("b") == 2
        assert t.get("c") == 3
        assert len(t) == 3

    def test_get_missing(self):
        t = tree_from(("a", 1), ("c", 3))
        assert t.get("b") is None

    def test_delete_one_of_many(self):
        t = tree_from(("a", 1), ("b", 2), ("c", 3))
        t.delete("b")
        assert t.get("a") == 1
        assert t.get("b") is None
        assert t.get("c") == 3
        assert len(t) == 2


# ── contains / None values ───────────────────────────────────────────────

class TestContains:
    def test_present(self):
        t = tree_from(("x", 10))
        assert "x" in t

    def test_absent(self):
        t = tree_from(("x", 10))
        assert "y" not in t

    def test_none_value_still_contained(self):
        t = AdaptiveRadixTree()
        t.put("n", None)
        assert "n" in t

    def test_get_none_value(self):
        t = AdaptiveRadixTree()
        t.put("n", None)
        assert t.get("n") is None  # indistinguishable from missing via get
        assert "n" in t            # but __contains__ distinguishes


# ── iteration ────────────────────────────────────────────────────────────

class TestIteration:
    def test_sorted_order(self):
        t = tree_from(("c", 3), ("a", 1), ("b", 2))
        assert list(t.items()) == [("a", 1), ("b", 2), ("c", 3)]

    def test_from_key(self):
        t = tree_from(*[(c, ord(c)) for c in "abcde"])
        assert [k for k, _ in t.items(from_key="c")] == ["c", "d", "e"]

    def test_to_key(self):
        t = tree_from(*[(c, ord(c)) for c in "abcde"])
        assert [k for k, _ in t.items(to_key="c")] == ["a", "b", "c"]

    def test_from_and_to(self):
        t = tree_from(*[(c, ord(c)) for c in "abcde"])
        assert [k for k, _ in t.items(from_key="b", to_key="d")] == ["b", "c", "d"]

    def test_empty_range(self):
        t = tree_from(("a", 1), ("z", 26))
        assert list(t.items(from_key="m", to_key="n")) == []

    def test_from_key_beyond_all(self):
        t = tree_from(("a", 1), ("b", 2))
        assert list(t.items(from_key="z")) == []

    def test_to_key_before_all(self):
        t = tree_from(("m", 1), ("n", 2))
        assert list(t.items(to_key="a")) == []

    def test_exact_bounds(self):
        t = tree_from(("a", 1), ("b", 2), ("c", 3))
        assert list(t.items(from_key="b", to_key="b")) == [("b", 2)]

    def test_items_after_deletes(self):
        t = tree_from(("a", 1), ("b", 2), ("c", 3))
        t.delete("b")
        assert list(t.items()) == [("a", 1), ("c", 3)]


# ── prefix keys (one key is a prefix of another) ─────────────────────────

class TestPrefixKeys:
    def test_short_then_long(self):
        t = tree_from(("ab", 1), ("abc", 2))
        assert t.get("ab") == 1
        assert t.get("abc") == 2

    def test_long_then_short(self):
        t = tree_from(("abc", 2), ("ab", 1))
        assert t.get("ab") == 1
        assert t.get("abc") == 2

    def test_empty_string_key(self):
        t = tree_from(("", 0), ("a", 1))
        assert t.get("") == 0
        assert t.get("a") == 1

    def test_empty_string_only(self):
        t = tree_from(("", 99))
        assert t.get("") == 99
        assert len(t) == 1

    def test_three_level_prefix_chain(self):
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3))
        assert t.get("a") == 1
        assert t.get("ab") == 2
        assert t.get("abc") == 3

    def test_delete_middle_prefix(self):
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3))
        t.delete("ab")
        assert t.get("a") == 1
        assert t.get("ab") is None
        assert t.get("abc") == 3
        assert len(t) == 2

    def test_delete_shortest_prefix(self):
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3))
        t.delete("a")
        assert t.get("a") is None
        assert t.get("ab") == 2
        assert t.get("abc") == 3

    def test_delete_longest_prefix(self):
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3))
        t.delete("abc")
        assert t.get("a") == 1
        assert t.get("ab") == 2
        assert t.get("abc") is None

    def test_iteration_with_prefix_keys(self):
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3))
        assert list(t.items()) == [("a", 1), ("ab", 2), ("abc", 3)]

    def test_empty_key_iteration_order(self):
        t = tree_from(("", 0), ("a", 1), ("b", 2))
        assert [k for k, _ in t.items()] == ["", "a", "b"]


# ── path compression ─────────────────────────────────────────────────────

class TestPathCompression:
    def test_shared_prefix(self):
        t = tree_from(("abc", 1), ("abd", 2), ("xyz", 3))
        assert t.get("abc") == 1
        assert t.get("abd") == 2
        assert t.get("xyz") == 3

    def test_deep_shared_prefix(self):
        t = tree_from(("abcdefghij", 1), ("abcdefghik", 2))
        assert t.get("abcdefghij") == 1
        assert t.get("abcdefghik") == 2

    def test_prefix_split(self):
        """Insert a key that forces a split in a compressed prefix."""
        t = tree_from(("abcdef", 1), ("abcxyz", 2))
        # now insert something that diverges earlier
        t.put("abZZZ", 3)
        assert t.get("abcdef") == 1
        assert t.get("abcxyz") == 2
        assert t.get("abZZZ") == 3

    def test_prefix_recompression_after_delete(self):
        """Delete should re-merge single-child inner nodes."""
        t = tree_from(("abc", 1), ("abd", 2))
        t.delete("abd")
        assert t.get("abc") == 1
        # the remaining leaf should be reachable after compaction
        t.put("abc", 99)
        assert t.get("abc") == 99

    def test_no_false_match_on_partial_prefix(self):
        t = tree_from(("abcdef", 1))
        assert t.get("abcXXX") is None
        assert t.get("abc") is None
        assert t.get("abcdefg") is None


# ── node growth (Node4 → Node16 → Node48 → Node256) ──────────────────────

class TestNodeGrowth:
    def test_node4_to_node16(self):
        """5 children at the same level triggers Node4 → Node16."""
        t = AdaptiveRadixTree()
        for i in range(5):
            t.put(chr(ord("a") + i), i)
        for i in range(5):
            assert t.get(chr(ord("a") + i)) == i
        assert len(t) == 5

    def test_node16_to_node48(self):
        """17 children triggers Node16 → Node48."""
        t = AdaptiveRadixTree()
        for i in range(17):
            t.put(chr(ord("a") + i), i)
        for i in range(17):
            assert t.get(chr(ord("a") + i)) == i

    def test_node48_to_node256(self):
        """49 children triggers Node48 → Node256."""
        t = AdaptiveRadixTree()
        for i in range(49):
            t.put(chr(ord("A") + i), i)
        for i in range(49):
            assert t.get(chr(ord("A") + i)) == i

    def test_sorted_iteration_after_growth(self):
        t = AdaptiveRadixTree()
        keys = [chr(ord("A") + i) for i in range(49)]
        for i, k in enumerate(keys):
            t.put(k, i)
        result = [k for k, _ in t.items()]
        assert result == sorted(keys)

    def test_full_byte_range(self):
        """All 256 single-byte keys."""
        t = AdaptiveRadixTree()
        for b in range(256):
            k = bytes([b]).decode("latin-1")
            t.put(k, b)
        assert len(t) == 256
        for b in range(256):
            k = bytes([b]).decode("latin-1")
            assert t.get(k) == b


# ── node shrinkage ────────────────────────────────────────────────────────

class TestNodeShrink:
    def test_shrink_node16_to_node4(self):
        t = AdaptiveRadixTree()
        for i in range(5):
            t.put(chr(ord("a") + i), i)
        # remove one so we're back to 4 → should shrink
        t.delete("e")
        for i in range(4):
            assert t.get(chr(ord("a") + i)) == i
        assert len(t) == 4

    def test_shrink_to_single_leaf(self):
        t = AdaptiveRadixTree()
        for i in range(5):
            t.put(chr(ord("a") + i), i)
        for i in range(1, 5):
            t.delete(chr(ord("a") + i))
        assert t.get("a") == 0
        assert len(t) == 1
        assert list(t.items()) == [("a", 0)]

    def test_shrink_to_empty(self):
        t = AdaptiveRadixTree()
        for i in range(10):
            t.put(chr(ord("a") + i), i)
        for i in range(10):
            t.delete(chr(ord("a") + i))
        assert len(t) == 0
        assert list(t.items()) == []


# ── unicode ───────────────────────────────────────────────────────────────

class TestUnicode:
    def test_accented(self):
        t = tree_from(("café", 1), ("naïve", 2))
        assert t.get("café") == 1
        assert t.get("naïve") == 2

    def test_cjk(self):
        t = tree_from(("日本語", 1), ("中文", 2), ("한국어", 3))
        assert t.get("日本語") == 1
        assert t.get("中文") == 2
        assert t.get("한국어") == 3

    def test_emoji(self):
        t = tree_from(("🌲", 1), ("🌳", 2))
        assert t.get("🌲") == 1
        assert t.get("🌳") == 2

    def test_unicode_sorted_iteration(self):
        keys = ["apple", "café", "banana"]
        t = AdaptiveRadixTree()
        for k in keys:
            t.put(k, k)
        result = [k for k, _ in t.items()]
        assert result == sorted(keys)


# ── stress tests ──────────────────────────────────────────────────────────

class TestStress:
    def test_1000_sequential_keys(self):
        t = AdaptiveRadixTree()
        keys = [f"key{i:04d}" for i in range(1000)]
        for i, k in enumerate(keys):
            t.put(k, i)
        assert len(t) == 1000
        for i, k in enumerate(keys):
            assert t.get(k) == i
        result = [k for k, _ in t.items()]
        assert result == sorted(keys)

    def test_1000_random_keys(self):
        rng = random.Random(42)
        t = AdaptiveRadixTree()
        keys = ["".join(rng.choices(string.ascii_letters, k=rng.randint(1, 20)))
                for _ in range(1000)]
        keys = list(set(keys))  # deduplicate
        vals = {k: i for i, k in enumerate(keys)}
        for k, v in vals.items():
            t.put(k, v)
        assert len(t) == len(vals)
        for k, v in vals.items():
            assert t.get(k) == v
        result = [k for k, _ in t.items()]
        assert result == sorted(vals.keys())

    def test_delete_all(self):
        t = AdaptiveRadixTree()
        keys = [f"k{i}" for i in range(200)]
        for k in keys:
            t.put(k, k)
        for k in keys:
            assert t.delete(k) is True
        assert len(t) == 0
        assert list(t.items()) == []
        # tree should be fully usable again
        t.put("fresh", 1)
        assert t.get("fresh") == 1

    def test_interleaved_insert_delete(self):
        rng = random.Random(99)
        t = AdaptiveRadixTree()
        live = {}
        for _ in range(2000):
            k = f"k{rng.randint(0, 200)}"
            if rng.random() < 0.7:
                v = rng.randint(0, 99999)
                t.put(k, v)
                live[k] = v
            else:
                existed = k in live
                assert t.delete(k) == existed
                live.pop(k, None)
        assert len(t) == len(live)
        for k, v in live.items():
            assert t.get(k) == v
        result = list(t.items())
        assert result == sorted(live.items())

    def test_range_query_stress(self):
        t = AdaptiveRadixTree()
        keys = sorted(f"k{i:04d}" for i in range(500))
        for k in keys:
            t.put(k, k)
        # check several ranges
        for lo, hi in [(50, 100), (0, 10), (490, 499), (200, 200)]:
            lo_key = f"k{lo:04d}"
            hi_key = f"k{hi:04d}"
            result = [k for k, _ in t.items(from_key=lo_key, to_key=hi_key)]
            expected = [f"k{i:04d}" for i in range(lo, hi + 1)]
            assert result == expected


# ── edge cases ────────────────────────────────────────────────────────────

class TestEdgeCases:
    def test_overwrite_does_not_change_len(self):
        t = tree_from(("a", 1))
        t.put("a", 2)
        t.put("a", 3)
        assert len(t) == 1

    def test_delete_returns_false_for_prefix_of_existing(self):
        t = tree_from(("abc", 1))
        assert t.delete("ab") is False
        assert t.delete("a") is False
        assert t.get("abc") == 1

    def test_delete_returns_false_for_extension_of_existing(self):
        t = tree_from(("ab", 1))
        assert t.delete("abc") is False
        assert t.get("ab") == 1

    def test_get_with_wrong_prefix(self):
        t = tree_from(("abc", 1))
        assert t.get("axc") is None
        assert t.get("xbc") is None

    def test_reinsert_after_delete(self):
        t = tree_from(("a", 1))
        t.delete("a")
        t.put("a", 2)
        assert t.get("a") == 2
        assert len(t) == 1

    def test_many_prefix_levels(self):
        t = AdaptiveRadixTree()
        for length in range(1, 20):
            k = "a" * length
            t.put(k, length)
        for length in range(1, 20):
            assert t.get("a" * length) == length
        assert len(t) == 19

    def test_single_char_keys_sorted(self):
        t = AdaptiveRadixTree()
        chars = list(string.ascii_lowercase)
        random.Random(7).shuffle(chars)
        for c in chars:
            t.put(c, c)
        result = [k for k, _ in t.items()]
        assert result == sorted(chars)

    def test_values_of_various_types(self):
        t = AdaptiveRadixTree()
        t.put("int", 42)
        t.put("str", "hello")
        t.put("list", [1, 2, 3])
        t.put("dict", {"a": 1})
        t.put("none", None)
        assert t.get("int") == 42
        assert t.get("str") == "hello"
        assert t.get("list") == [1, 2, 3]
        assert t.get("dict") == {"a": 1}
        assert "none" in t

    def test_long_key(self):
        t = AdaptiveRadixTree()
        k = "x" * 10_000
        t.put(k, 1)
        assert t.get(k) == 1
        assert t.delete(k) is True
        assert len(t) == 0

    def test_keys_differing_only_in_last_byte(self):
        t = tree_from(("helloA", 1), ("helloB", 2), ("helloC", 3))
        assert t.get("helloA") == 1
        assert t.get("helloB") == 2
        assert t.get("helloC") == 3
        assert list(t.items()) == [("helloA", 1), ("helloB", 2), ("helloC", 3)]


# ── range scan edge cases (optimized _iter_range path) ────────────────────

class TestRangeScan:
    def test_from_only(self):
        t = tree_from(*[(f"k{i:03d}", i) for i in range(100)])
        result = [k for k, _ in t.items(from_key="k090")]
        assert result == [f"k{i:03d}" for i in range(90, 100)]

    def test_to_only(self):
        t = tree_from(*[(f"k{i:03d}", i) for i in range(100)])
        result = [k for k, _ in t.items(to_key="k009")]
        assert result == [f"k{i:03d}" for i in range(10)]

    def test_from_equals_to(self):
        t = tree_from(*[(c, c) for c in "abcde"])
        assert list(t.items(from_key="c", to_key="c")) == [("c", "c")]

    def test_from_equals_to_missing(self):
        t = tree_from(("a", 1), ("c", 3))
        assert list(t.items(from_key="b", to_key="b")) == []

    def test_range_with_shared_prefix(self):
        t = tree_from(("abc", 1), ("abd", 2), ("abe", 3), ("abf", 4))
        assert list(t.items(from_key="abd", to_key="abe")) == [("abd", 2), ("abe", 3)]

    def test_range_prefix_keys(self):
        """Range scan over keys where some are prefixes of others."""
        t = tree_from(("a", 1), ("ab", 2), ("abc", 3), ("abd", 4), ("b", 5))
        assert list(t.items(from_key="ab", to_key="abd")) == [
            ("ab", 2), ("abc", 3), ("abd", 4)
        ]

    def test_range_from_is_prefix_of_keys(self):
        t = tree_from(("abc", 1), ("abd", 2), ("xyz", 3))
        assert [k for k, _ in t.items(from_key="ab")] == ["abc", "abd", "xyz"]

    def test_range_to_is_prefix_of_keys(self):
        t = tree_from(("a", 1), ("abc", 2), ("abd", 3), ("b", 4))
        assert list(t.items(to_key="ab")) == [("a", 1)]

    def test_range_with_empty_from_key(self):
        t = tree_from(("", 0), ("a", 1), ("b", 2))
        assert list(t.items(from_key="")) == [("", 0), ("a", 1), ("b", 2)]

    def test_range_with_empty_to_key(self):
        t = tree_from(("", 0), ("a", 1), ("b", 2))
        assert list(t.items(to_key="")) == [("", 0)]

    def test_range_all_same_prefix(self):
        keys = [f"prefix{chr(ord('a') + i)}" for i in range(26)]
        t = AdaptiveRadixTree()
        for k in keys:
            t.put(k, k)
        result = [k for k, _ in t.items(from_key="prefixm", to_key="prefixp")]
        assert result == ["prefixm", "prefixn", "prefixo", "prefixp"]

    def test_range_deep_tree(self):
        """Range query on deeply nested keys (long shared prefixes)."""
        base = "a" * 50
        keys = sorted(base + chr(ord("a") + i) for i in range(10))
        t = AdaptiveRadixTree()
        for k in keys:
            t.put(k, k)
        result = [k for k, _ in t.items(from_key=keys[3], to_key=keys[7])]
        assert result == keys[3:8]

    def test_range_no_overlap(self):
        t = tree_from(("aaa", 1), ("bbb", 2), ("ccc", 3))
        assert list(t.items(from_key="d", to_key="z")) == []
        assert list(t.items(from_key="0", to_key="1")) == []

    def test_range_matches_full_scan_filter(self):
        """Optimized path must agree with naive scan-and-filter."""
        rng = random.Random(777)
        t = AdaptiveRadixTree()
        keys = sorted(set(
            "".join(rng.choices(string.ascii_lowercase, k=rng.randint(1, 8)))
            for _ in range(500)
        ))
        for k in keys:
            t.put(k, k)
        for _ in range(50):
            i = rng.randint(0, len(keys) - 1)
            j = rng.randint(i, min(i + 50, len(keys) - 1))
            lo, hi = keys[i], keys[j]
            result = list(t.items(from_key=lo, to_key=hi))
            expected = [(k, k) for k in keys if lo <= k <= hi]
            assert result == expected, f"range [{lo!r}, {hi!r}]"
