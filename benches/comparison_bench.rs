use arsort::arsort;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use voracious_radix_sort::RadixSort;

/// Generates uniformly distributed random u64 integers.
fn generate_data_random(size: usize) -> Vec<u64> {
    let mut rng = StdRng::seed_from_u64(42);
    (0..size).map(|_| rng.random()).collect()
}

fn bench_world_class(c: &mut Criterion) {
    let mut group = c.benchmark_group("Arsort_vs_WorldClass");

    // Primary target domain for Arsort: L1/L2 cache bandwidth boundary (1000 <= N <= 5000)
    let sizes = [1000, 2000, 5000];

    for size in sizes {
        let data = generate_data_random(size);

        // Closure helper to eliminate Criterion iteration boilerplate
        let mut bench_target = |id: &str, sort_fn: &mut dyn FnMut(&mut [u64])| {
            group.bench_with_input(BenchmarkId::new(id, size), &data, |b, input| {
                b.iter_batched(
                    || input.clone(),
                    |mut d| {
                        sort_fn(&mut d);
                        black_box(&mut d);
                    },
                    criterion::BatchSize::SmallInput,
                );
            });
        };

        // 1. Rust standard library (ipnsort: state-of-the-art branchless comparison sort)
        bench_target("std_sort_unstable_ipnsort", &mut |d| d.sort_unstable());

        // 2. Voracious Radix Sort (state-of-the-art radix sort)
        bench_target("voracious_radix_sort", &mut |d| d.voracious_sort());

        // 3. Arsort (Arithmetic routing engine)
        bench_target("arsort", &mut |d| arsort(d));
    }

    group.finish();
}

criterion_group!(benches, bench_world_class);
criterion_main!(benches);
