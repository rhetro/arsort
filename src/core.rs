#![allow(unsafe_op_in_unsafe_fn)]

use std::mem::MaybeUninit;
use std::ptr;

pub const MAX_TOPOLOGICAL_LEN: usize = 5000;

/// Trait defining an evaluation key for sorting.
/// Types implementing this trait map their underlying payload to a `u64` sorting key.
pub trait SortKey: Copy {
    fn sort_key(&self) -> u64;
}

impl SortKey for u64 {
    #[inline(always)]
    fn sort_key(&self) -> u64 {
        *self
    }
}

/// Speculatively checks if the slice is already sorted in descending order
/// by sampling 16 evenly spaced elements across the array.
#[inline(always)]
pub fn is_speculative_reversed<T: SortKey>(data: &[T]) -> bool {
    let n = data.len();
    if n < 32 {
        return false;
    }
    let step = n / 16;
    let keys = [
        data[0].sort_key(),
        data[step].sort_key(),
        data[step * 2].sort_key(),
        data[step * 3].sort_key(),
        data[step * 4].sort_key(),
        data[step * 5].sort_key(),
        data[step * 6].sort_key(),
        data[step * 7].sort_key(),
        data[step * 8].sort_key(),
        data[step * 9].sort_key(),
        data[step * 10].sort_key(),
        data[step * 11].sort_key(),
        data[step * 12].sort_key(),
        data[step * 13].sort_key(),
        data[step * 14].sort_key(),
        data[n - 1].sort_key(),
    ];
    let mut violation = 0u32;
    for i in 0..15 {
        violation |= (keys[i] < keys[i + 1]) as u32;
    }
    violation == 0 && (keys[0] > keys[15])
}

/// Speculatively checks if the slice is already sorted in ascending order
/// by sampling 16 evenly spaced elements across the array.
#[inline(always)]
pub fn is_speculative_sorted<T: SortKey>(data: &[T]) -> bool {
    let n = data.len();
    if n < 32 {
        return false;
    }
    let step = n / 16;
    let keys = [
        data[0].sort_key(),
        data[step].sort_key(),
        data[step * 2].sort_key(),
        data[step * 3].sort_key(),
        data[step * 4].sort_key(),
        data[step * 5].sort_key(),
        data[step * 6].sort_key(),
        data[step * 7].sort_key(),
        data[step * 8].sort_key(),
        data[step * 9].sort_key(),
        data[step * 10].sort_key(),
        data[step * 11].sort_key(),
        data[step * 12].sort_key(),
        data[step * 13].sort_key(),
        data[step * 14].sort_key(),
        data[n - 1].sort_key(),
    ];
    let mut violation = 0u32;
    for i in 0..15 {
        violation |= (keys[i] > keys[i + 1]) as u32;
    }
    violation == 0 && (keys[0] <= keys[15])
}

/// Low-overhead insertion sort for micro-slices (N <= 16).
/// Employs raw pointer operations (`ptr::read`/`ptr::write` and `ptr::copy_nonoverlapping`)
/// to bypass slice swapping overhead and promote register-level element shifts.
#[inline(always)]
pub fn insertion_sort<T: SortKey>(arr: &mut [T]) {
    let len = arr.len();
    if len <= 1 {
        return;
    }

    if len == 2 {
        if arr[0].sort_key() > arr[1].sort_key() {
            arr.swap(0, 1);
        }
        return;
    }

    let base_ptr = arr.as_mut_ptr();
    for i in 1..len {
        unsafe {
            let val = ptr::read(base_ptr.add(i));
            let val_key = val.sort_key();
            let mut j = i;

            while j > 0 {
                let prev_ptr = base_ptr.add(j - 1);
                if (*prev_ptr).sort_key() > val_key {
                    ptr::copy_nonoverlapping(prev_ptr, base_ptr.add(j), 1);
                    j -= 1;
                } else {
                    break;
                }
            }
            ptr::write(base_ptr.add(j), val);
        }
    }
}

/// Single-pass minimum and maximum value boundary discovery.
#[inline(always)]
fn find_bounds<T: SortKey>(arr: &[T]) -> (u64, u64) {
    let mut min = u64::MAX;
    let mut max = u64::MIN;
    for item in arr {
        let k = item.sort_key();
        if k < min {
            min = k;
        }
        if k > max {
            max = k;
        }
    }
    (min, max)
}

/// Attempts topological fallback sort for highly duplicated datasets (<= 16 unique keys).
/// Routes to AVX2 SIMD acceleration if target hardware supports it.
#[inline(always)]
pub fn try_topological_sort<T: SortKey>(arr: &mut [T], primary_buffer: &mut [T]) -> bool {
    let n = arr.len();
    if n <= 100 || n > MAX_TOPOLOGICAL_LEN {
        return false;
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { try_topological_avx2(arr, primary_buffer) };
        }
    }

    try_topological_scalar(arr, primary_buffer)
}

/// AVX2-accelerated low-cardinality topological sort path.
/// Matches input keys against up to 16 unique active register nodes in parallel via SIMD vector operations.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2", enable = "bmi1")]
unsafe fn try_topological_avx2<T: SortKey>(arr: &mut [T], primary_buffer: &mut [T]) -> bool {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let n = arr.len();
    debug_assert!(
        n <= MAX_TOPOLOGICAL_LEN,
        "Topological sort buffer bound exceeded"
    );

    let mut nodes = [0u64; 16];
    let mut weights = [0usize; 16];
    let mut unique_count = 0u32;

    let mut node_indices_buf = MaybeUninit::<[u8; MAX_TOPOLOGICAL_LEN]>::uninit();
    let node_indices_ptr = node_indices_buf.as_mut_ptr() as *mut u8;

    let mut n0 = _mm256_setzero_si256();
    let mut n1 = _mm256_setzero_si256();
    let mut n2 = _mm256_setzero_si256();
    let mut n3 = _mm256_setzero_si256();
    let mut valid_mask = 0u16;

    for (i, item) in arr.iter().enumerate() {
        let k = item.sort_key();
        let key_vec = _mm256_set1_epi64x(k as i64);

        let cmp0 = _mm256_cmpeq_epi64(key_vec, n0);
        let cmp1 = _mm256_cmpeq_epi64(key_vec, n1);
        let cmp2 = _mm256_cmpeq_epi64(key_vec, n2);
        let cmp3 = _mm256_cmpeq_epi64(key_vec, n3);

        let m0 = _mm256_movemask_pd(_mm256_castsi256_pd(cmp0)) as u16;
        let m1 = _mm256_movemask_pd(_mm256_castsi256_pd(cmp1)) as u16;
        let m2 = _mm256_movemask_pd(_mm256_castsi256_pd(cmp2)) as u16;
        let m3 = _mm256_movemask_pd(_mm256_castsi256_pd(cmp3)) as u16;

        let match_mask = m0 | (m1 << 4) | (m2 << 8) | (m3 << 12);
        let active_match = match_mask & valid_mask;

        if active_match != 0 {
            let node_id = active_match.trailing_zeros() as usize;
            *weights.get_unchecked_mut(node_id) += 1;
            *node_indices_ptr.add(i) = node_id as u8;
        } else {
            if unique_count == 16 {
                return false;
            }
            let node_id = unique_count as usize;
            *nodes.get_unchecked_mut(node_id) = k;
            *weights.get_unchecked_mut(node_id) = 1;
            *node_indices_ptr.add(i) = node_id as u8;

            match node_id / 4 {
                0 => n0 = _mm256_loadu_si256(nodes.as_ptr().add(0) as *const __m256i),
                1 => n1 = _mm256_loadu_si256(nodes.as_ptr().add(4) as *const __m256i),
                2 => n2 = _mm256_loadu_si256(nodes.as_ptr().add(8) as *const __m256i),
                3 => n3 = _mm256_loadu_si256(nodes.as_ptr().add(12) as *const __m256i),
                _ => std::hint::unreachable_unchecked(),
            }

            unique_count += 1;
            valid_mask = ((1u32 << unique_count) - 1) as u16;
        }
    }

    if unique_count <= 1 {
        return true;
    }

    let k_len = unique_count as usize;
    let mut offsets = [0usize; 16];
    for (i, offset_slot) in offsets.iter_mut().take(k_len).enumerate() {
        let mut offset = 0;
        let node_i = *nodes.get_unchecked(i);
        for j in 0..k_len {
            if *nodes.get_unchecked(j) < node_i {
                offset += *weights.get_unchecked(j);
            }
        }
        *offset_slot = offset;
    }

    let mut scatter_offsets = offsets;
    let dst_ptr = primary_buffer.as_mut_ptr();

    for (i, item) in arr.iter().enumerate() {
        let node_id = *node_indices_ptr.add(i) as usize;
        let dest_idx = *scatter_offsets.get_unchecked(node_id);

        ptr::copy_nonoverlapping(item, dst_ptr.add(dest_idx), 1);
        *scatter_offsets.get_unchecked_mut(node_id) += 1;
    }

    ptr::copy_nonoverlapping(dst_ptr, arr.as_mut_ptr(), n);

    true
}

/// Scalar fallback for low-cardinality topological sorting when AVX2 is unavailable.
#[inline(always)]
fn try_topological_scalar<T: SortKey>(arr: &mut [T], primary_buffer: &mut [T]) -> bool {
    let n = arr.len();
    debug_assert!(
        n <= MAX_TOPOLOGICAL_LEN,
        "Topological sort buffer bound exceeded"
    );

    let mut nodes = [0u64; 16];
    let mut weights = [0usize; 16];
    let mut unique_count = 0u32;
    let mut valid_mask = 0u16;

    let mut node_indices_buf = MaybeUninit::<[u8; MAX_TOPOLOGICAL_LEN]>::uninit();
    let node_indices_ptr = node_indices_buf.as_mut_ptr() as *mut u8;

    let mut n0 = 0u64;
    let mut n1 = 0u64;
    let mut n2 = 0u64;
    let mut n3 = 0u64;
    let mut n4 = 0u64;
    let mut n5 = 0u64;
    let mut n6 = 0u64;
    let mut n7 = 0u64;
    let mut n8 = 0u64;
    let mut n9 = 0u64;
    let mut n10 = 0u64;
    let mut n11 = 0u64;
    let mut n12 = 0u64;
    let mut n13 = 0u64;
    let mut n14 = 0u64;
    let mut n15 = 0u64;

    for (i, item) in arr.iter().enumerate() {
        let k = item.sort_key();

        let mut match_mask = 0u16;
        match_mask |= (n0 == k) as u16;
        match_mask |= ((n1 == k) as u16) << 1;
        match_mask |= ((n2 == k) as u16) << 2;
        match_mask |= ((n3 == k) as u16) << 3;
        match_mask |= ((n4 == k) as u16) << 4;
        match_mask |= ((n5 == k) as u16) << 5;
        match_mask |= ((n6 == k) as u16) << 6;
        match_mask |= ((n7 == k) as u16) << 7;
        match_mask |= ((n8 == k) as u16) << 8;
        match_mask |= ((n9 == k) as u16) << 9;
        match_mask |= ((n10 == k) as u16) << 10;
        match_mask |= ((n11 == k) as u16) << 11;
        match_mask |= ((n12 == k) as u16) << 12;
        match_mask |= ((n13 == k) as u16) << 13;
        match_mask |= ((n14 == k) as u16) << 14;
        match_mask |= ((n15 == k) as u16) << 15;

        let active_match = match_mask & valid_mask;

        if active_match != 0 {
            let node_id = active_match.trailing_zeros() as usize;
            unsafe {
                *weights.get_unchecked_mut(node_id) += 1;
                *node_indices_ptr.add(i) = node_id as u8;
            }
        } else {
            if unique_count == 16 {
                return false;
            }
            let node_id = unique_count as usize;
            unsafe {
                *nodes.get_unchecked_mut(node_id) = k;
                *weights.get_unchecked_mut(node_id) = 1;
                *node_indices_ptr.add(i) = node_id as u8;
            }
            unique_count += 1;
            valid_mask = ((1u32 << unique_count) - 1) as u16;

            n0 = nodes[0];
            n1 = nodes[1];
            n2 = nodes[2];
            n3 = nodes[3];
            n4 = nodes[4];
            n5 = nodes[5];
            n6 = nodes[6];
            n7 = nodes[7];
            n8 = nodes[8];
            n9 = nodes[9];
            n10 = nodes[10];
            n11 = nodes[11];
            n12 = nodes[12];
            n13 = nodes[13];
            n14 = nodes[14];
            n15 = nodes[15];
        }
    }

    if unique_count <= 1 {
        return true;
    }

    let k_len = unique_count as usize;
    let mut offsets = [0usize; 16];
    for (i, offset_slot) in offsets.iter_mut().take(k_len).enumerate() {
        let mut offset = 0;
        let node_i = unsafe { *nodes.get_unchecked(i) };
        for j in 0..k_len {
            if unsafe { *nodes.get_unchecked(j) } < node_i {
                offset += unsafe { *weights.get_unchecked(j) };
            }
        }
        *offset_slot = offset;
    }

    let mut scatter_offsets = offsets;
    let dst_ptr = primary_buffer.as_mut_ptr();

    for (i, item) in arr.iter().enumerate() {
        unsafe {
            let node_id = *node_indices_ptr.add(i) as usize;
            let dest_idx = *scatter_offsets.get_unchecked(node_id);

            ptr::copy_nonoverlapping(item, dst_ptr.add(dest_idx), 1);
            *scatter_offsets.get_unchecked_mut(node_id) += 1;
        }
    }

    unsafe {
        ptr::copy_nonoverlapping(dst_ptr, arr.as_mut_ptr(), n);
    }

    true
}

/// Core arithmetic routing sort engine.
/// Evaluates bounds and dynamically routes elements through counting sort (Perfect Hash fast-path)
/// or fractional multiplier bucket scatter passes.
pub fn arsort_recurse<T: SortKey>(
    arr: &mut [T],
    primary_buffer: &mut [T],
    counts: &mut [u32],
    offsets: &mut [usize],
    scatter_workspace: &mut [usize],
    depth: usize,
) {
    let n = arr.len();
    if n <= 16 {
        insertion_sort(arr);
        return;
    }

    if depth >= 16 {
        insertion_sort(arr);
        return;
    }

    let (min, max) = find_bounds(arr);
    if min == max {
        return;
    }

    let range = (max - min) as u128;

    // Fast-path: Low-cardinality topological detection (<= 16 unique values)
    if try_topological_sort(arr, primary_buffer) {
        return;
    }

    if range < 16384 && (range as usize) < n * 8 {
        let k = (range + 1) as usize;

        counts[..k].fill(0);
        for item in arr.iter() {
            let idx = (item.sort_key() - min) as usize;
            counts[idx] += 1;
        }

        let mut current_offset = 0;
        for (i, count) in counts[..k].iter().enumerate() {
            offsets[i] = current_offset;
            current_offset += *count as usize;
        }

        let (scatter_offsets, _) = scatter_workspace.split_at_mut(k);
        scatter_offsets.copy_from_slice(&offsets[..k]);

        unsafe {
            let dst_ptr = primary_buffer.as_mut_ptr();
            for item in arr.iter() {
                let idx = (item.sort_key() - min) as usize;
                let dest_idx = scatter_offsets[idx];
                ptr::copy_nonoverlapping(item, dst_ptr.add(dest_idx), 1);
                scatter_offsets[idx] += 1;
            }
            ptr::copy_nonoverlapping(dst_ptr, arr.as_mut_ptr(), n);
        }
        return;
    }

    // General-path: Arithmetic routing using a fixed-point 64-bit precision multiplier
    let target_k = (n / 4).max(1);
    let k = target_k.next_power_of_two().clamp(16, 16384);

    let range_plus_one = range + 1;
    let multiplier: u128 = ((k as u128) << 64).checked_div(range_plus_one).unwrap_or(0);

    macro_rules! calc_idx {
        ($val:expr) => {{
            let offset = $val.sort_key().wrapping_sub(min) as u128;
            let idx = ((offset * multiplier) >> 64) as usize;
            idx & (k - 1)
        }};
    }

    counts[..k].fill(0);
    for item in arr.iter() {
        counts[calc_idx!(item)] += 1;
    }

    let mut current_offset = 0;
    for (i, count) in counts[..k].iter().enumerate() {
        offsets[i] = current_offset;
        current_offset += *count as usize;
    }

    let (scatter_offsets, next_scatter) = scatter_workspace.split_at_mut(k);
    scatter_offsets.copy_from_slice(&offsets[..k]);

    unsafe {
        let dst_ptr = primary_buffer.as_mut_ptr();
        for item in arr.iter() {
            let idx = calc_idx!(item);
            let dest_idx = scatter_offsets[idx];
            ptr::copy_nonoverlapping(item, dst_ptr.add(dest_idx), 1);
            scatter_offsets[idx] += 1;
        }
        ptr::copy_nonoverlapping(dst_ptr, arr.as_mut_ptr(), n);
    }

    // Recursively sort target buckets
    let mut start = 0;
    for &end in &scatter_offsets[..k] {
        let bucket_len = end - start;

        if bucket_len > 16 {
            arsort_recurse(
                &mut arr[start..end],
                &mut primary_buffer[start..end],
                counts,
                offsets,
                next_scatter,
                depth + 1,
            );
        } else if bucket_len > 1 {
            insertion_sort(&mut arr[start..end]);
        }
        start = end;
    }
}
