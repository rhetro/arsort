#![doc = include_str!("../README.md")]

//! # Arsort (Arithmetic Routing Sort)
//!
//! A specialized, high-performance micro-engine designed for small-scale datasets (`N <= 5000`)
//! that fit cleanly within the L1/L2 cache hierarchy. It utilizes branchless arithmetic routing
//! and a zero-allocation thread-local workspace to eliminate pipeline stalls and heap overhead.

pub mod core;
pub mod prelude;

use crate::core::{
    arsort_recurse, insertion_sort, is_speculative_reversed, is_speculative_sorted, SortKey,
};
use std::cell::RefCell;
use std::mem::MaybeUninit;

/// Maximum power-of-two bucket capacity allocated per depth pass.
const MAX_K: usize = 16384;

/// Maximum recursive depth limit before falling back to insertion sort.
const MAX_DEPTH: usize = 16;

thread_local! {
    // Thread-local temporary workspace buffers to achieve zero runtime heap allocation.
    static COUNTS: RefCell<Vec<u32>> = RefCell::new(vec![0; MAX_K]);
    static OFFSETS: RefCell<Vec<usize>> = RefCell::new(vec![0; MAX_K]);
    static SCATTER: RefCell<Vec<usize>> = RefCell::new(vec![0; MAX_K * MAX_DEPTH]);

    static PRIMARY_BUFFER: RefCell<Vec<MaybeUninit<u64>>> = const { RefCell::new(Vec::new()) };
}

/// Sorts the slice in-place using arithmetic routing.
///
/// This function serves as the primary entry point for Arsort. It handles micro-array fast paths,
/// speculative sorting checks, thread-local memory provisioning, and delegates core routing to
/// the recursive engine.
///
/// # Examples
/// ```
/// use arsort::prelude::*;
///
/// let mut data = vec![500u64, 100, 300];
/// arsort(&mut data);
/// assert_eq!(data, vec![100, 300, 500]);
/// ```
pub fn arsort<T: SortKey>(arr: &mut [T]) {
    const {
        assert!(
            std::mem::align_of::<T>() <= std::mem::align_of::<u64>(),
            "Arsort primary buffer requires element alignment <= 8 bytes"
        );
    }

    let n = arr.len();
    if n <= 1 {
        return;
    }

    // Fast-path: Micro-arrays (N <= 16) are handled directly by insertion sort
    if n <= 16 {
        insertion_sort(arr);
        return;
    }

    // Fast-path: Speculative O(N) pass for reversed or pre-sorted slices
    if is_speculative_reversed(arr) {
        let mut is_descending = true;
        for i in 1..n {
            if arr[i - 1].sort_key() < arr[i].sort_key() {
                is_descending = false;
                break;
            }
        }
        if is_descending {
            arr.reverse();
            return;
        }
    } else if is_speculative_sorted(arr) {
        let mut is_ascending = true;
        for i in 1..n {
            if arr[i - 1].sort_key() > arr[i].sort_key() {
                is_ascending = false;
                break;
            }
        }
        if is_ascending {
            return;
        }
    }

    // Provision workspace from Thread-Local Storage (zero OS-level allocation overhead)
    PRIMARY_BUFFER.with(|buf_cell| {
        let mut buf_vec = buf_cell.borrow_mut();
        let elem_size = std::mem::size_of_val(arr);
        let u64_size = std::mem::size_of::<u64>();
        let required_u64_units = elem_size.div_ceil(u64_size);

        if buf_vec.len() < required_u64_units {
            buf_vec.resize(required_u64_units, MaybeUninit::uninit());
        }

        let primary_buffer: &mut [T] =
            unsafe { std::slice::from_raw_parts_mut(buf_vec.as_mut_ptr() as *mut T, n) };

        COUNTS.with(|c| {
            OFFSETS.with(|o| {
                SCATTER.with(|s| {
                    let mut counts = c.borrow_mut();
                    let mut offsets = o.borrow_mut();
                    let mut scatter = s.borrow_mut();

                    arsort_recurse(
                        arr,
                        primary_buffer,
                        &mut counts,
                        &mut offsets,
                        &mut scatter,
                        0,
                    );
                });
            });
        });
    });
}
