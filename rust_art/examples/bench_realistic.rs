//! Benchmark with realistic key distributions: URLs, file paths, log lines.
//!
//! These keys have long shared prefixes, variable lengths, and mixed branching
//! density — the kind of workload where inline prefix storage matters.
//!
//! Run: cargo run --release --example bench_realistic [SIZE...]

use std::collections::BTreeMap;
use std::time::Instant;

use rust_art::ARTMap;

// ---------------------------------------------------------------------------
// Key generators
// ---------------------------------------------------------------------------

/// Simple deterministic LCG for reproducible shuffles without pulling in rand.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn usize(&mut self, bound: usize) -> usize {
        (self.next() >> 33) as usize % bound
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.usize(i + 1);
            v.swap(i, j);
        }
    }
}

/// URL-like keys: /api/v2/users/{id}/orders/{id}/items/{id}
/// Shared prefixes are long (10-20 bytes), total keys 40-80 bytes.
fn make_url_keys(n: usize) -> Vec<Vec<u8>> {
    let services = [
        "users",
        "orders",
        "products",
        "inventory",
        "payments",
        "accounts",
        "sessions",
        "analytics",
        "notifications",
        "settings",
    ];
    let actions = [
        "list", "detail", "create", "update", "delete", "search", "export", "import", "validate",
        "archive",
    ];
    let versions = ["v1", "v2", "v3"];

    let mut keys = Vec::with_capacity(n);
    let mut rng = Lcg::new(123);

    for i in 0..n {
        let ver = versions[i % versions.len()];
        let svc = services[i % services.len()];
        let act = actions[(i / services.len()) % actions.len()];
        let id1 = i;
        let id2 = rng.usize(100_000);
        let key = format!("/api/{ver}/{svc}/{id1:08x}/{act}/{id2:05}",);
        keys.push(key.into_bytes());
    }
    keys
}

/// File-path keys: /home/user/projects/project-N/src/module/file.ext
/// Deep hierarchy, long shared prefixes (20-30 bytes).
fn make_filepath_keys(n: usize) -> Vec<Vec<u8>> {
    let projects = [
        "webapp",
        "backend",
        "cli-tools",
        "shared-lib",
        "infra",
        "data-pipeline",
        "ml-models",
        "docs",
        "tests",
        "scripts",
    ];
    let modules = [
        "auth",
        "api",
        "db",
        "cache",
        "config",
        "logging",
        "middleware",
        "handlers",
        "models",
        "utils",
        "services",
        "controllers",
        "views",
        "templates",
        "static",
    ];
    let extensions = ["rs", "toml", "json", "yaml", "md", "sql", "sh", "py"];

    let mut keys = Vec::with_capacity(n);

    for i in 0..n {
        let proj = projects[i % projects.len()];
        let module = modules[(i / projects.len()) % modules.len()];
        let ext = extensions[(i / (projects.len() * modules.len())) % extensions.len()];
        let file_num = i;
        let key = format!("/home/user/projects/{proj}/src/{module}/file_{file_num:06x}.{ext}",);
        keys.push(key.into_bytes());
    }
    keys
}

/// Log-line keys: timestamp:host:service:level:request-id
/// Very long keys (60-90 bytes), structured with colons.
fn make_log_keys(n: usize) -> Vec<Vec<u8>> {
    let hosts = [
        "web01", "web02", "web03", "api01", "api02", "db01", "db02", "cache01", "worker01",
        "worker02",
    ];
    let services = [
        "nginx",
        "app",
        "postgres",
        "redis",
        "celery",
        "prometheus",
        "grafana",
        "consul",
        "vault",
        "traefik",
    ];
    let levels = ["DEBUG", "INFO", "WARN", "ERROR"];

    let mut keys = Vec::with_capacity(n);
    let base_ts = 1_700_000_000_000u64; // millisecond timestamp

    for i in 0..n {
        let ts = base_ts + i as u64;
        let host = hosts[i % hosts.len()];
        let svc = services[(i / hosts.len()) % services.len()];
        let level = levels[(i / (hosts.len() * services.len())) % levels.len()];
        let req_id = i;
        let key = format!("{ts}:{host}:{svc}:{level}:req-{req_id:012x}",);
        keys.push(key.into_bytes());
    }
    keys
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

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

fn run_workload(name: &str, keys: &[Vec<u8>]) {
    let n = keys.len();
    let header = format!(
        "{:<24}{:>12}{:>12}{:>10}",
        "Operation", "ART", "BTreeMap", "Ratio"
    );
    let sep = "-".repeat(header.len());

    println!();
    println!("{}", "=".repeat(header.len()));
    println!(
        "  {} ({} keys, avg len {} bytes)",
        name,
        format_with_commas(n),
        keys.iter().map(|k| k.len()).sum::<usize>() / n
    );
    println!("{}", "=".repeat(header.len()));
    println!("{}", header);
    println!("{}", sep);

    let mut shuffled = keys.to_vec();
    let mut rng = Lcg::new(42);
    rng.shuffle(&mut shuffled);

    // -- random put --
    let t_art = {
        let mut tree = ARTMap::new();
        let t = Instant::now();
        for (i, k) in shuffled.iter().enumerate() {
            tree.put(k, i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(tree);
        elapsed
    };
    let t_bt = {
        let mut map = BTreeMap::new();
        let t = Instant::now();
        for (i, k) in shuffled.iter().enumerate() {
            map.insert(k.clone(), i);
        }
        let elapsed = t.elapsed().as_secs_f64();
        std::mem::forget(map);
        elapsed
    };
    row("Random put", t_art, t_bt);

    // Build trees for remaining benchmarks
    let mut art = ARTMap::new();
    for (i, k) in shuffled.iter().enumerate() {
        art.put(k, i);
    }
    let mut btree: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for (i, k) in shuffled.iter().enumerate() {
        btree.insert(k.clone(), i);
    }

    // -- random get (hit) --
    let mut lookup = shuffled.clone();
    rng = Lcg::new(999);
    rng.shuffle(&mut lookup);

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

    // -- random get (miss): keys with same structure but different prefix --
    let miss_keys: Vec<Vec<u8>> = keys
        .iter()
        .map(|k| {
            let mut m = b"MISS".to_vec();
            m.extend_from_slice(k);
            m
        })
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

    // -- range query (1%) --
    let mut sorted_keys: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
    sorted_keys.sort();
    let lo_idx = n / 2;
    let hi_idx = (lo_idx + n / 100).min(n - 1);
    let lo_key = sorted_keys[lo_idx];
    let hi_key = sorted_keys[hi_idx];
    let expected = sorted_keys[lo_idx..=hi_idx].len();

    let t_art = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in art.range_iter(Some(lo_key), Some(hi_key)) {
            std::hint::black_box(entry);
            count += 1;
        }
        assert_eq!(count, expected, "ART range count mismatch");
        t.elapsed().as_secs_f64()
    };
    let t_bt = {
        let t = Instant::now();
        let mut count = 0usize;
        for entry in btree.range(lo_key.to_vec()..=hi_key.to_vec()) {
            std::hint::black_box(entry);
            count += 1;
        }
        assert_eq!(count, expected, "BTreeMap range count mismatch");
        t.elapsed().as_secs_f64()
    };
    row("Range query (1%)", t_art, t_bt);

    // -- random delete --
    let mut del_order = shuffled.clone();
    rng = Lcg::new(777);
    rng.shuffle(&mut del_order);

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
    let n: usize = if args.is_empty() {
        1_000_000
    } else {
        args[0].replace(['_', ','], "").parse().unwrap()
    };

    let url_keys = make_url_keys(n);
    run_workload("URL paths", &url_keys);

    let filepath_keys = make_filepath_keys(n);
    run_workload("File paths", &filepath_keys);

    let log_keys = make_log_keys(n);
    run_workload("Log lines", &log_keys);
}
