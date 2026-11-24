// Execution command:
// cargo +nightly miri test --test miri_soundness

use arsort::prelude::*;
use std::num::NonZeroU64;

/// 1. Re-entrancy Safety Verification
///
/// Verifies that if `arsort` is invoked recursively inside `SortKey::sort_key()`
/// (with N > 16 and non-monotonic order), `try_borrow_mut()` gracefully falls back
/// to `insertion_sort` without panicking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReentrantKey {
    val: u64,
}

impl SortKey for ReentrantKey {
    fn sort_key(&self) -> u64 {
        let mut inner = [17u64, 3, 15, 1, 12, 8, 19, 2, 14, 6, 11, 4, 18, 9, 5, 16, 7];
        arsort(&mut inner);
        debug_assert!(inner.windows(2).all(|w| w[0] <= w[1]));
        self.val
    }
}

#[test]
fn test_reentrant_borrow_safe_fallback() {
    let raw = [17u64, 3, 15, 1, 12, 8, 19, 2, 14, 6, 11, 4, 18, 9, 5, 16, 7];
    let mut data: Vec<ReentrantKey> = raw.into_iter().map(|val| ReentrantKey { val }).collect();

    arsort(&mut data);

    assert!(data.windows(2).all(|w| w[0].val <= w[1].val));
}

/// 2. Miri Soundness Verification: Uninitialized Buffer Handling with Niche Types
///
/// Ensures that types with niche validity invariants (e.g., `NonZeroU64`) pass through
/// the uninitialized `MaybeUninit<T>` workspace with zero undefined behavior under Miri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NichePayload {
    key: u64,
    tag: NonZeroU64,
}

impl SortKey for NichePayload {
    fn sort_key(&self) -> u64 {
        self.key
    }
}

#[test]
fn test_uninit_slice_soundness_under_miri() {
    let mut data: Vec<NichePayload> = (0..64)
        .map(|i| NichePayload {
            key: ((i * 7919) % 64) * 1000,
            tag: NonZeroU64::new(i + 1).unwrap(),
        })
        .collect();

    arsort(&mut data);

    assert!(data.windows(2).all(|w| w[0].key <= w[1].key));
}

/// 3. Adversarial Distribution: 17 Unique Keys with Extreme Skew
///
/// Bypasses the 16-unique-key topological fast-path while concentrating almost all elements
/// into a single duplicate key alongside wide-range outliers.
#[test]
fn test_adversarial_distribution() {
    let n = 5000;
    let mut data = Vec::with_capacity(n);

    for i in 0..16u64 {
        data.push(i.wrapping_mul(0x0FFF_FFFF_FFFF_FFFF));
    }

    for i in 0..16u64 {
        data.push(i.wrapping_mul(0x0FFF_FFFF_FFFF_FFFF));
    }

    data.resize(n, 1);
    data.swap(0, n - 1);
    data.swap(1, n / 2);

    let mut expected = data.clone();
    expected.sort_unstable();

    arsort(&mut data);
    assert_eq!(data, expected);
}
