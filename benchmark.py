#!/usr/bin/env python3
"""Benchmark: Adaptive Radix Tree vs B-Tree.

Compares put, get, delete, full iteration, and range-query performance
at 100 K, 1 M, and 10 M keys.

Usage
-----
    python benchmark.py              # run all sizes
    python benchmark.py 100000       # run one size
"""

import gc
import random
import sys
import time

from adaptive_radix_tree import AdaptiveRadixTree
from btree import BTree


# ── helpers ───────────────────────────────────────────────────────────────

def _make_keys(n):
    """Return *n* zero-padded string keys and a shuffled copy."""
    keys = [f"key{i:012d}" for i in range(n)]
    shuffled = keys[:]
    random.shuffle(shuffled)
    return keys, shuffled


def _time(fn):
    """Run *fn*, return elapsed wall-clock seconds."""
    gc.disable()
    t0 = time.perf_counter()
    result = fn()
    elapsed = time.perf_counter() - t0
    gc.enable()
    return elapsed, result


# ── individual benchmarks ────────────────────────────────────────────────

def bench_put(tree, keys):
    for i, k in enumerate(keys):
        tree.put(k, i)


def bench_get_hit(tree, keys):
    for k in keys:
        tree.get(k)


def bench_get_miss(tree, n):
    for i in range(n):
        tree.get(f"miss{i:012d}")


def bench_iterate(tree):
    count = 0
    for _ in tree.items():
        count += 1
    return count


def bench_range(tree, lo, hi):
    count = 0
    for _ in tree.items(from_key=lo, to_key=hi):
        count += 1
    return count


def bench_delete(tree, keys):
    for k in keys:
        tree.delete(k)


# ── driver ────────────────────────────────────────────────────────────────

HEADER = (
    f"{'Operation':<24}{'ART':>12}{'B-tree':>12}{'Ratio':>10}"
)
SEP = "-" * len(HEADER)


def _row(label, t_art, t_bt):
    ratio = t_art / t_bt if t_bt > 0 else float("inf")
    print(f"{label:<24}{t_art:>11.3f}s{t_bt:>11.3f}s{ratio:>9.2f}x")


def run(n):
    print()
    print(f"{'=' * len(HEADER)}")
    print(f"  Benchmark: {n:,} keys")
    print(f"{'=' * len(HEADER)}")
    print(HEADER)
    print(SEP)

    sorted_keys, shuffled_keys = _make_keys(n)

    # --- random put -------------------------------------------------------
    art = AdaptiveRadixTree()
    t_art, _ = _time(lambda: bench_put(art, shuffled_keys))
    bt = BTree(order=256)
    t_bt, _ = _time(lambda: bench_put(bt, shuffled_keys))
    _row("Random put", t_art, t_bt)

    # --- sequential put ---------------------------------------------------
    art_seq = AdaptiveRadixTree()
    t_art_s, _ = _time(lambda: bench_put(art_seq, sorted_keys))
    bt_seq = BTree(order=256)
    t_bt_s, _ = _time(lambda: bench_put(bt_seq, sorted_keys))
    _row("Sequential put", t_art_s, t_bt_s)

    # (use the random-insert trees for the remaining benchmarks)

    # --- random get (hit) -------------------------------------------------
    lookup = shuffled_keys[:]
    random.shuffle(lookup)
    t_art, _ = _time(lambda: bench_get_hit(art, lookup))
    t_bt, _ = _time(lambda: bench_get_hit(bt, lookup))
    _row("Random get (hit)", t_art, t_bt)

    # --- random get (miss) ------------------------------------------------
    t_art, _ = _time(lambda: bench_get_miss(art, n))
    t_bt, _ = _time(lambda: bench_get_miss(bt, n))
    _row("Random get (miss)", t_art, t_bt)

    # --- full iteration ---------------------------------------------------
    t_art, cnt_a = _time(lambda: bench_iterate(art))
    t_bt, cnt_b = _time(lambda: bench_iterate(bt))
    assert cnt_a == cnt_b == n
    _row("Iterate all", t_art, t_bt)

    # --- range query (1 % of key space) -----------------------------------
    lo_idx = n // 2
    hi_idx = lo_idx + n // 100
    lo_key = f"key{lo_idx:012d}"
    hi_key = f"key{hi_idx:012d}"
    expected = hi_idx - lo_idx + 1
    t_art, cnt_a = _time(lambda: bench_range(art, lo_key, hi_key))
    t_bt, cnt_b = _time(lambda: bench_range(bt, lo_key, hi_key))
    assert cnt_a == cnt_b == expected, f"{cnt_a} {cnt_b} {expected}"
    _row(f"Range query (1%)", t_art, t_bt)

    # --- random delete (all keys) -----------------------------------------
    del_order = shuffled_keys[:]
    random.shuffle(del_order)
    # copy trees so we still have them for verification
    t_art, _ = _time(lambda: bench_delete(art, del_order))
    t_bt, _ = _time(lambda: bench_delete(bt, del_order))
    _row("Random delete", t_art, t_bt)

    print(SEP)
    print()


# ── main ──────────────────────────────────────────────────────────────────

SIZES = [100_000, 1_000_000, 10_000_000]

if __name__ == "__main__":
    random.seed(42)

    if len(sys.argv) > 1:
        sizes = [int(s.replace(",", "").replace("_", "")) for s in sys.argv[1:]]
    else:
        sizes = SIZES

    for n in sizes:
        run(n)
