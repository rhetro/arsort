use arsort::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Helper function to verify Arsort correctness against std::sort_unstable
fn verify_sort(mut data: Vec<u64>) {
    let mut expected = data.clone();
    expected.sort_unstable();
    arsort(&mut data);
    assert_eq!(
        data,
        expected,
        "Sort verification failed for array length: {}",
        data.len()
    );
}

#[test]
fn test_edge_cases() {
    verify_sort(vec![]); // Empty slice
    verify_sort(vec![42]); // Single element
    verify_sort(vec![7, 7, 7, 7, 7]); // All elements identical
    verify_sort(vec![1, 2, 3, 4, 5, 6, 7, 8]); // Already sorted ascending
    verify_sort(vec![8, 7, 6, 5, 4, 3, 2, 1]); // Already sorted descending
}

#[test]
fn test_boundary_and_speculative_thresholds() {
    // Tests boundary around speculative checks (threshold N = 32)
    let sizes = [31, 32, 33, 63, 64, 65, 5000, 5001];

    for &size in &sizes {
        // Ascending order (Triggers speculative sorted path for N >= 32)
        verify_sort((0..size as u64).collect());

        // Descending order (Triggers speculative reversed path for N >= 32)
        verify_sort((0..size as u64).rev().collect());
    }
}

#[test]
fn test_fuzzing_various_sizes_and_distributions() {
    let mut rng = StdRng::seed_from_u64(1234);
    let sizes = [15, 16, 17, 100, 255, 256, 1000, 2000, 5000];

    for &size in &sizes {
        // 1. Uniformly random data
        verify_sort((0..size).map(|_| rng.random()).collect());

        // 2. Low Cardinality (Dense range 0..10)
        verify_sort((0..size).map(|_| rng.random_range(0..10)).collect());

        // 3. Perfect Hash Fast-Path candidate range (0..10000)
        verify_sort((0..size).map(|_| rng.random_range(0..10000)).collect());
    }
}

// Custom struct test payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TestItem {
    id: u32,
    key: u64,
}

impl SortKey for TestItem {
    #[inline(always)]
    fn sort_key(&self) -> u64 {
        self.key
    }
}

#[test]
fn test_custom_struct_payload() {
    let mut data = vec![
        TestItem { id: 10, key: 500 },
        TestItem { id: 20, key: 100 },
        TestItem { id: 30, key: 300 },
        TestItem { id: 40, key: 200 },
    ];

    arsort(&mut data);

    assert_eq!(data[0], TestItem { id: 20, key: 100 });
    assert_eq!(data[1], TestItem { id: 40, key: 200 });
    assert_eq!(data[2], TestItem { id: 30, key: 300 });
    assert_eq!(data[3], TestItem { id: 10, key: 500 });
}
