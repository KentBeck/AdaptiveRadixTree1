//! Benchmark: ARTMap vs BTreeMap at 100K, 1M, and 10M keys.
//!
//! Uses `key{i:012}` format keys to match the Python benchmark.
//! Measures: random put, sequential put, random get (hit), random get (miss),
//! iterate all, 1% range query, random delete.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::collections::BTreeMap;
use std::time::Duration;

use rust_art::ARTMap;

fn make_keys(n: usize) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let sorted: Vec<Vec<u8>> = (0..n)
        .map(|i| format!("key{:012}", i).into_bytes())
        .collect();
    let mut shuffled = sorted.clone();
    let mut rng = StdRng::seed_from_u64(42);
    shuffled.shuffle(&mut rng);
    (sorted, shuffled)
}

fn bench_random_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_put");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                let mut tree = ARTMap::new();
                for (i, k) in shuffled.iter().enumerate() {
                    tree.put(k, i);
                }
                tree
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                let mut map = BTreeMap::new();
                for (i, k) in shuffled.iter().enumerate() {
                    map.insert(k.clone(), i);
                }
                map
            });
        });
    }
    group.finish();
}

fn bench_sequential_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_put");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (sorted, _) = make_keys(n);

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                let mut tree = ARTMap::new();
                for (i, k) in sorted.iter().enumerate() {
                    tree.put(k, i);
                }
                tree
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                let mut map = BTreeMap::new();
                for (i, k) in sorted.iter().enumerate() {
                    map.insert(k.clone(), i);
                }
                map
            });
        });
    }
    group.finish();
}

fn bench_random_get_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_get_hit");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        let mut tree = ARTMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            tree.put(k, i);
        }
        let mut map = BTreeMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            map.insert(k.clone(), i);
        }

        let mut lookup = shuffled.clone();
        let mut rng = StdRng::seed_from_u64(123);
        lookup.shuffle(&mut rng);

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                for k in &lookup {
                    std::hint::black_box(tree.get(k));
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                for k in &lookup {
                    std::hint::black_box(map.get(k));
                }
            });
        });
    }
    group.finish();
}

fn bench_random_get_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_get_miss");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        let mut tree = ARTMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            tree.put(k, i);
        }
        let mut map = BTreeMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            map.insert(k.clone(), i);
        }

        let miss_keys: Vec<Vec<u8>> = (0..n)
            .map(|i| format!("miss{:012}", i).into_bytes())
            .collect();

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                for k in &miss_keys {
                    std::hint::black_box(tree.get(k));
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                for k in &miss_keys {
                    std::hint::black_box(map.get(k));
                }
            });
        });
    }
    group.finish();
}

fn bench_iterate_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("iterate_all");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        let mut tree = ARTMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            tree.put(k, i);
        }
        let mut map = BTreeMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            map.insert(k.clone(), i);
        }

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                let mut count = 0usize;
                for _ in tree.items() {
                    count += 1;
                }
                count
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                let mut count = 0usize;
                for _ in map.iter() {
                    count += 1;
                }
                count
            });
        });
    }
    group.finish();
}

fn bench_range_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_query_1pct");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        let mut tree = ARTMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            tree.put(k, i);
        }
        let mut map = BTreeMap::new();
        for (i, k) in shuffled.iter().enumerate() {
            map.insert(k.clone(), i);
        }

        let lo_idx = n / 2;
        let hi_idx = lo_idx + n / 100;
        let lo_key = format!("key{:012}", lo_idx).into_bytes();
        let hi_key = format!("key{:012}", hi_idx).into_bytes();

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter(|| {
                let items = tree.range(Some(&lo_key), Some(&hi_key));
                std::hint::black_box(items.len())
            });
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter(|| {
                let mut count = 0usize;
                for (k, _) in map.range(lo_key.clone()..=hi_key.clone()) {
                    std::hint::black_box(k);
                    count += 1;
                }
                count
            });
        });
    }
    group.finish();
}

fn bench_random_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_delete");
    for &n in &[100_000, 1_000_000, 10_000_000] {
        let (_, shuffled) = make_keys(n);

        let mut del_order = shuffled.clone();
        let mut rng = StdRng::seed_from_u64(999);
        del_order.shuffle(&mut rng);

        group.bench_with_input(BenchmarkId::new("ART", n), &n, |b, _| {
            b.iter_batched(
                || {
                    let mut tree = ARTMap::new();
                    for (i, k) in shuffled.iter().enumerate() {
                        tree.put(k, i);
                    }
                    tree
                },
                |mut tree| {
                    for k in &del_order {
                        tree.delete(k);
                    }
                },
                criterion::BatchSize::PerIteration,
            );
        });

        group.bench_with_input(BenchmarkId::new("BTreeMap", n), &n, |b, _| {
            b.iter_batched(
                || {
                    let mut map = BTreeMap::new();
                    for (i, k) in shuffled.iter().enumerate() {
                        map.insert(k.clone(), i);
                    }
                    map
                },
                |mut map| {
                    for k in &del_order {
                        map.remove(k);
                    }
                },
                criterion::BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(2));
    targets = bench_random_put, bench_sequential_put, bench_random_get_hit,
              bench_random_get_miss, bench_iterate_all, bench_range_query,
              bench_random_delete
}
criterion_main!(benches);
