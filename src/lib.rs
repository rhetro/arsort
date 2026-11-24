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

struct TlsWorkspace {
    counts: Vec<u32>,
    offsets: Vec<usize>,
    scatter: Vec<usize>,
    primary_buffer: Vec<MaybeUninit<u64>>,
}

impl TlsWorkspace {
    fn new() -> Self {
        Self {
            counts: vec![0; MAX_K],
            offsets: vec![0; MAX_K],
            scatter: vec![0; MAX_K * MAX_DEPTH],
            primary_buffer: Vec::new(),
        }
    }
}

thread_local! {
    // Single unified RefCell reduces TLS lookups and borrow flag checks from 4 to 1 per call.
    static WORKSPACE: RefCell<TlsWorkspace> = RefCell::new(TlsWorkspace::new());
}

/// Sorts the slice in-place using arithmetic routing.
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

    WORKSPACE.with(|ws_cell| {
        let Ok(mut ws) = ws_cell.try_borrow_mut() else {
            insertion_sort(arr);
            return;
        };

        let ws = &mut *ws;
        let elem_size = std::mem::size_of_val(arr);
        let u64_size = std::mem::size_of::<u64>();
        let required_u64_units = elem_size.div_ceil(u64_size);

        if ws.primary_buffer.len() < required_u64_units {
            ws.primary_buffer
                .resize(required_u64_units, MaybeUninit::uninit());
        }

        let primary_buffer: &mut [MaybeUninit<T>] = unsafe {
            std::slice::from_raw_parts_mut(ws.primary_buffer.as_mut_ptr() as *mut MaybeUninit<T>, n)
        };

        arsort_recurse(
            arr,
            primary_buffer,
            &mut ws.counts,
            &mut ws.offsets,
            &mut ws.scatter,
            0,
        );
    });
}
