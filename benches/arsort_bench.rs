use arsort::arsort;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// 1. Full-range uniformly random data
fn generate_data_random(size: usize) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(42);
    (0..size).map(|_| rng.random()).collect()
}

// 2. Bounded random data (for verifying the Perfect Hash fast-path)
fn generate_data_range(size: usize, max_val: u64) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(42);
    (0..size).map(|_| rng.random_range(0..max_val)).collect()
}

// 3. Wide-range data with low cardinality (primary target for topological fallback)
fn generate_sparse_low_cardinality(size: usize, unique_count: usize) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(42);
    // Generate a fixed number of distinct, widely separated random values
    let unique_values: Vec<u64> = (0..unique_count).map(|_| rng.random()).collect();
    // Fill the array by randomly sampling from these unique values
    (0..size)
        .map(|_| unique_values[rng.random_range(0..unique_count as u64) as usize])
        .collect()
}

// Helper to register benchmark pairs (ipnsort vs arsort) without code duplication
fn bench_pair<F1, F2>(
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    label: &str,
    size: usize,
    data: &[u64],
    mut sort_ipn: F1,
    mut sort_ars: F2,
) where
    F1: FnMut(&mut [u64]),
    F2: FnMut(&mut [u64]),
{
    group.bench_with_input(
        BenchmarkId::new(format!("ipnsort_{}", label), size),
        data,
        |b, d| {
            b.iter_batched(
                || d.to_vec(),
                |mut data| {
                    sort_ipn(&mut data);
                    black_box(&mut data);
                },
                criterion::BatchSize::SmallInput,
            );
        },
    );

    group.bench_with_input(
        BenchmarkId::new(format!("ARS_{}", label), size),
        data,
        |b, d| {
            b.iter_batched(
                || d.to_vec(),
                |mut data| {
                    sort_ars(&mut data);
                    black_box(&mut data);
                },
                criterion::BatchSize::SmallInput,
            );
        },
    );
}

fn bench_sorts(c: &mut Criterion) {
    let mut group = c.benchmark_group("Arsort_vs_Ipnsort");
    let sizes = [100, 1000, 2000, 5000];

    for size in sizes {
        // 1. Random (Full range) - Standard arithmetic routing
        let data_random = generate_data_random(size);
        bench_pair(
            &mut group,
            "Random",
            size,
            &data_random,
            |d| d.sort_unstable(),
            arsort,
        );

        // 2. LowCard_Dense (0..10) - Target for Perfect Hash fast-path
        let data_dense = generate_data_range(size, 10);
        bench_pair(
            &mut group,
            "LowCardDense",
            size,
            &data_dense,
            |d| d.sort_unstable(),
            arsort,
        );

        // 3. LowCard_Sparse (Wide range, 10 unique values) - Topological sort domain
        let data_sparse = generate_sparse_low_cardinality(size, 10);
        bench_pair(
            &mut group,
            "LowCardSparse",
            size,
            &data_sparse,
            |d| d.sort_unstable(),
            arsort,
        );

        // 4. Fast Path (Range < 16384) - Upper bound boundary test for Perfect Hash
        let data_fast_path = generate_data_range(size, 10000);
        bench_pair(
            &mut group,
            "FastPath",
            size,
            &data_fast_path,
            |d| d.sort_unstable(),
            arsort,
        );

        // 5. Reversed - Measuring speculative O(N) reversal overhead
        let mut data_reversed = data_random.clone();
        data_reversed.sort_unstable_by(|a, b| b.cmp(a));
        bench_pair(
            &mut group,
            "Reversed",
            size,
            &data_reversed,
            |d| d.sort_unstable(),
            arsort,
        );
    }

    group.finish();
}

criterion_group!(benches, bench_sorts);
criterion_main!(benches);
