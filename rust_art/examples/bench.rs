//! Quick benchmark: ARTMap vs BTreeMap at 100K, 1M, and 10M keys.
//!
//! Run: cargo run --release --example bench [SIZE...]

use std::collections::BTreeMap;
use std::time::Instant;

use rust_art::ARTMap;

fn make_keys(n: usize) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let sorted: Vec<Vec<u8>> = (0..n)
        .map(|i| format!("key{:012}", i).into_bytes())
        .collect();
    let mut shuffled = sorted.clone();
    // Fisher-Yates with simple LCG for determinism
    let mut rng: u64 = 42;
    for i in (1..shuffled.len()).rev() {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (rng >> 33) as usize % (i + 1);
        shuffled.swap(i, j);
    }
    (sorted, shuffled)
}

fn row(label: &str, t_art: f64, t_bt: f64) {
    let ratio = if t_bt > 0.0 {
        t_art / t_bt
    } else {
        f64::INFINITY
    };
    println!(
        "{:<24}{:>11.3}s{:>11.3}s{:>9.2}x",
        label, t_art, t_bt, ratio
    );
}

fn run(n: usize) {
    let header = format!(
        "{:<24}{:>12}{:>12}{:>10}",
        "Operation", "ART", "BTreeMap", "Ratio"
    );
    let sep = "-".repeat(header.len());

    println!();
    println!("{}", "=".repeat(header.len()));
    println!("  Benchmark: {:>12} keys", format_with_commas(n));
    println!("{}", "=".repeat(header.len()));
    println!("{}", header);
    println!("{}", sep);

    let (sorted_keys, shuffled_keys) = make_keys(n);

    // -- random put --
    let t_art = {
        let mut tree = ARTMap::new();
        let t = Instant::now();
        for (i, k) in shuffled_keys.iter().enumerate() {
            tree.put(k, i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(tree); // avoid timing deallocation
        elapsed
    };
    let t_bt = {
        let mut map = BTreeMap::new();
        let t = Instant::now();
        for (i, k) in shuffled_keys.iter().enumerate() {
            map.insert(k.clone(), i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(map);
        elapsed
    };
    row("Random put", t_art, t_bt);

    // -- sequential put --
    let t_art = {
        let mut tree = ARTMap::new();
        let t = Instant::now();
        for (i, k) in sorted_keys.iter().enumerate() {
            tree.put(k, i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(tree);
        elapsed
    };
    let t_bt = {
        let mut map = BTreeMap::new();
        let t = Instant::now();
        for (i, k) in sorted_keys.iter().enumerate() {
            map.insert(k.clone(), i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(map);
        elapsed
    };
    row("Sequential put", t_art, t_bt);

    // Build trees for remaining benchmarks
    let mut art = ARTMap::new();
    for (i, k) in shuffled_keys.iter().enumerate() {
        art.put(k, i);
    }
    let mut btree: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for (i, k) in shuffled_keys.iter().enumerate() {
        btree.insert(k.clone(), i);
    }

    // -- random get (hit) --
    let mut lookup = shuffled_keys.clone();
    {
        let mut rng: u64 = 123;
        for i in (1..lookup.len()).rev() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng >> 33) as usize % (i + 1);
            lookup.swap(i, j);
        }
    }

    let t_art = {
        let t = Instant::now();
        for k in &lookup {
            std::hint::black_box(art.get(k));
        }
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        for k in &lookup {
            std::hint::black_box(btree.get(k));
        }
        t.elapsed().as_secs_f64()
    };
    row("Random get (hit)", t_art, t_bt);

    // -- random get (miss) --
    let miss_keys: Vec<Vec<u8>> = (0..n)
        .map(|i| format!("miss{:012}", i).into_bytes())
        .collect();
    let t_art = {
        let t = Instant::now();
        for k in &miss_keys {
            std::hint::black_box(art.get(k));
        }
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        for k in &miss_keys {
            std::hint::black_box(btree.get(k));
        }
        t.elapsed().as_secs_f64()
    };
    row("Random get (miss)", t_art, t_bt);

    // -- iterate all --
    let t_art = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in art.iter() {
            std::hint::black_box(entry);
            count += 1;
        }
        std::hint::black_box(count);
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in btree.iter() {
            std::hint::black_box(entry);
            count += 1;
        }
        std::hint::black_box(count);
        t.elapsed().as_secs_f64()
    };
    row("Iterate all", t_art, t_bt);

    // -- range query (1% of key space) --
    let lo_idx = n / 2;
    let hi_idx = lo_idx + n / 100;
    let lo_key = format!("key{:012}", lo_idx).into_bytes();
    let hi_key = format!("key{:012}", hi_idx).into_bytes();
    let expected = hi_idx - lo_idx + 1;

    let t_art = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in art.range_iter(Some(&lo_key), Some(&hi_key)) {
            std::hint::black_box(entry);
            count += 1;
        }
        assert_eq!(count, expected);
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in btree.range(lo_key.clone()..=hi_key.clone()) {
            std::hint::black_box(entry);
            count += 1;
        }
        assert_eq!(count, expected);
        t.elapsed().as_secs_f64()
    };
    row("Range query (1%)", t_art, t_bt);

    // -- random delete --
    let mut del_order = shuffled_keys.clone();
    {
        let mut rng: u64 = 999;
        for i in (1..del_order.len()).rev() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng >> 33) as usize % (i + 1);
            del_order.swap(i, j);
        }
    }
    let t_art = {
        let t = Instant::now();
        for k in &del_order {
            art.delete(k);
        }
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        for k in &del_order {
            btree.remove(k);
        }
        t.elapsed().as_secs_f64()
    };
    row("Random delete", t_art, t_bt);

    println!("{}", sep);
}

fn format_with_commas(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sizes: Vec<usize> = if args.is_empty() {
        vec![100_000, 1_000_000, 10_000_000]
    } else {
        args.iter()
            .map(|s| s.replace(['_', ','], "").parse().unwrap())
            .collect()
    };
    for n in sizes {
        run(n);
    }
}
