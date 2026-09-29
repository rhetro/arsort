use arsort::arsort;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use voracious_radix_sort::RadixSort;

// Generate full-range uniformly random data using a fixed seed
fn generate_data_random(size: usize) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(42);
    (0..size).map(|_| rng.random()).collect()
}

fn bench_large_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("Arsort_LargeScale_Boundaries");

    // Limits verification range: N = 10,000 to 50,000
    // Evaluating the performance curve past the L1/L2 cache boundary (5,000)
    let sizes = [10000, 20000, 50000];

    for size in sizes {
        let data = generate_data_random(size);

        // 1. std::sort_unstable (ipnsort: O(N log N) branchless comparison)
        group.bench_with_input(
            BenchmarkId::new("std_sort_unstable_ipnsort", size),
            &data,
            |b, d| {
                b.iter_batched(
                    || d.clone(),
                    |mut data| {
                        data.sort_unstable();
                        black_box(&mut data);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        // 2. voracious_radix_sort (State-of-the-art radix sort for large integer datasets)
        group.bench_with_input(
            BenchmarkId::new("voracious_radix_sort", size),
            &data,
            |b, d| {
                b.iter_batched(
                    || d.clone(),
                    |mut data| {
                        data.voracious_sort();
                        black_box(&mut data);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        // 3. arsort (L1/L2 optimized fractional arithmetic routing)
        group.bench_with_input(BenchmarkId::new("arsort", size), &data, |b, d| {
            b.iter_batched(
                || d.clone(),
                |mut data| {
                    arsort(&mut data);
                    black_box(&mut data);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_large_scale);
criterion_main!(benches);
