# Arsort (Arithmetic Routing Sort)

This crate provides a standalone arithmetic routing-based sorting algorithm specifically designed for small-scale data that fits efficiently within or near the L1 data cache (primary target domain: element count `N <= 5000`). It is not a general-purpose sorting library, but rather a highly specialized micro-engine aimed at maximizing processing efficiency within a specific hardware bandwidth.

## Design Philosophy and Algorithmic Significance

To physically eliminate the pipeline stalls (branch mispredictions) caused by conditional branching in comparison-based sorts (like `std::sort_unstable`), Arsort utilizes an algorithm that determines target buckets by arithmetically processing the values themselves.

At runtime, it utilizes an uninitialized arena (`MaybeUninit`) held in thread-local storage (`thread_local!`) as a temporary buffer. This achieves zero runtime allocations by restricting OS memory allocation requests strictly to the initial thread provisioning, while exposing only a simple `arsort(&mut data)` API to the user.

### Intended Limitations

This crate intentionally omits fallback logic to external sorting algorithms (such as delegating to `ipnsort`) for large datasets, as well as extensive mitigation logic for highly duplicated elements.
This decision prevents instruction cache bloat and avoids hindering compiler optimizations (like loop unrolling) that occur when introducing additional branching. The code structure is entirely dedicated to a single purpose: executing the fastest possible pointer movements using pure arithmetic calculations under specific conditions.

## Trade-offs and Performance Characteristics

Due to its architectural nature, there are specific data ranges where it excels alongside clear hardware bottlenecks.

### ✅ Favorable Conditions (Sweet Spot)
* **Primary Target Range (`1000 <= N <= 5000`):** Optimally utilizes the CPU's L1/L2 cache hierarchy (scalable up to `N = 20,000`). In this environment, branchless arithmetic routing yields a **1.4x - 1.6x speedup** over `ipnsort`.
* **Perfect Hash Fast-Path:** If the difference between the maximum and minimum values (Range) is less than 16,384, a counting sort fast-path activates. This skips arithmetic shifts and calculates the index directly, resolving the sort in a single distribution pass with zero collision risk.

### ❌ Unfavorable Conditions (Bottlenecks)
* **Low Cardinality (Highly Duplicated Data):** Arsort's mandatory 4-pass memory access pattern struggles against `ipnsort`'s in-place partitioning with equal-element filtering. While an AVX2-accelerated topological fallback mitigates worst-case performance for up to 16 unique elements, it still trails `ipnsort`.
* **Micro Arrays (`N <= 100`):** The fixed constant-time setup overhead (boundary discovery and fixed-point multiplier computation) exceeds the pure comparison cost of comparative sorting.
* **Non-x86_64 Architectures (No AVX2 Support):** Core SIMD acceleration and low-cardinality fallbacks rely heavily on `x86_64` AVX2 intrinsics. On non-AVX2 targets (e.g., ARM64, WASM), the engine gracefully falls back to a scalar path, but performance will lag behind standard library routines.

## Benchmarks

The following results were measured using `criterion` comparing Arsort against Rust's standard library `std::sort_unstable` (`ipnsort`). The dataset consists of `u64` integers.

| Dataset / Condition | `std::sort_unstable` | `arsort` | Speedup | Note |
| :--- | :--- | :--- | :--- | :--- |
| **N=100** (Micro Array) | 806.86 ns | 941.12 ns | **0.86x** | Micro-array setup & insertion sort bound. |
| **N=1000** (L1 Sweet Spot) | 11.90 µs | 9.71 µs | **1.23x** | Optimal L1 cache arithmetic routing. |
| **N=2000** (L1 Optimal Range) | 25.61 µs | 20.16 µs | **1.27x** | Peak branchless scatter efficiency. |
| **N=5000** (L1/L2 Saturation) | 73.68 µs | 48.86 µs | **1.51x** | Massive speedup as arithmetic routing dominates. |
| **N=1000** (Fast Path) | 12.07 µs | 9.86 µs | **1.22x** | Perfect Hash fast-path activation. |
| **N=2000** (Fast Path) | 25.63 µs | 24.08 µs | **1.06x** | Range < 16,384 direct bucket distribution. |
| **N=5000** (Fast Path) | 73.96 µs | 39.40 µs | **1.88x** | Peak Perfect Hash performance. |
| **N=1000** (Low Cardinality) | 6.02 µs | 10.45 µs | **0.58x** | `ipnsort` in-place duplicate filtering wins. |
| **N=2000** (Low Cardinality) | 11.13 µs | 20.92 µs | **0.53x** | 4-pass memory scatter throughput limit. |
| **N=5000** (Low Cardinality) | 26.89 µs | 52.92 µs | **0.51x** | Structural memory bandwidth bottleneck. |
| **N=2000** (Reversed) | 3.08 µs | 2.63 µs | **1.17x** | Speculative O(N) reversal pass succeeded. |

*Note: Measurements reflect pure algorithmic hardware scaling using zero-runtime-allocation thread-local workspace (`thread_local!`). OS noise and outliers were mitigated via Criterion batched iterations.*

### Performance Against State-of-the-Art (Sweet Spot)

To demonstrate the true hardware efficiency of this micro-engine within its optimal target domain (1000 <= N <= 5000), Arsort is compared against two of the fastest state-of-the-art sorting implementations available in the Rust ecosystem on fully random `u64` datasets:

* **`std::sort_unstable` (`ipnsort`):** The pinnacle of branchless comparison-based sorting.
* **`voracious_radix_sort`:** A highly optimized, state-of-the-art radix sort that dominates large-scale integer sorting.

| Dataset Size (`N`) | `std::sort_unstable` (`ipnsort`) | `voracious_radix_sort` | **`arsort`** | Speedup vs `std` | Speedup vs `voracious` |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **1000** | 13.12 µs | 15.98 µs | **9.36 µs** | **1.40x** | **1.71x** |
| **2000** | 26.78 µs | 27.80 µs | **18.88 µs** | **1.42x** | **1.47x** |
| **5000** | 75.29 µs | 59.22 µs | **46.05 µs** | **1.64x** | **1.29x** |

#### Analysis of Results
* **vs. `ipnsort`:** While `ipnsort` successfully eliminates pipeline stalls (branch mispredictions), it is ultimately bound by O(N log N) comparison complexity. Arsort's pure O(N) arithmetic routing strictly outperforms it across this entire hardware band, reaching up to **1.64x speedup** at N=5000.
* **vs. `voracious_radix_sort`:** Radix sort is a theoretical powerhouse for integers. However, in the micro-domain (N <= 5000), the overhead of multiple radix passes and histogram allocation drags down its throughput. Arsort's single-pass fractional routing secures a definitive lead within the L1/L2 cache boundary.
* **Trade-off on Low Cardinality:** Due to Arsort's mandatory 4-pass out-of-place memory movements (Read Original -> Write Buffer -> Read Buffer -> Write Original), it physically cannot match `ipnsort` on highly duplicated datasets (N = 1000 - 5000), trailing at ~0.55x speed. `ipnsort` leverages in-place partitioning with immediate equal-element skipping, drastically reducing memory writes.

### Boundary Analysis: Scalability Limit and Large-Scale Recommendation (N > 5000)

While `arsort` is engineered specifically as a zero-runtime-allocation micro-engine for micro-to-small datasets (N <= 5000), its 64-bit precision arithmetic routing scales smoothly into mid-range arrays before reaching physical cache-bandwidth constraints:

| Dataset Size (`N`) | `std::sort_unstable` (`ipnsort`) | `voracious_radix_sort` | **`arsort`** | Speedup vs `std` | Speedup vs `voracious` | Domain Status |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **10,000** | 157.46 µs | 117.82 µs | **90.71 µs** | **1.74x** | **1.30x** | Dominant (L2 Cache Hit) |
| **20,000** | 357.34 µs | 224.91 µs | **192.12 µs** | **1.86x** | **1.17x** | Crossover Threshold |
| **50,000** | 1,072.10 µs | **600.38 µs** | **856.82 µs** | **1.25x** | **0.70x** | Handover to Single-Pass Memory Engines |

#### Technical Insights & Recommendations

* **Micro-Domain Dominance (N <= 5000):** `arsort` specializes in the L1/L2 cache band (N <= 5000), outperforming standard comparative sorting (`std::sort_unstable`) by 1.4x - 1.6x.
* **Extended Scalability (N <= 20,000):** It maintains a solid performance lead up to N = 20,000 before out-of-place random memory scatter operations cross the L2/L3 cache latency boundary.
* **The Cache Line Boundary (N >= 50,000):** Beyond N = 20,000, random memory access latency gradually neutralizes `arsort`'s single-pass routing efficiency on a single thread.
* **Recommended Algorithm Choice:** For massive datasets (N > 20,000) that far exceed L2/L3 cache capacities, transitioning to single-pass disjoint routing engines like [`zan-sort`](https://crates.io/crates/zan-sort) is recommended. `zan-sort` optimizes memory access alignment for high DRAM bandwidth saturation without multi-pass penalties, scaling cleanly on both single-threaded and multi-core targets.

## Usage

To use the sorting functionality of this crate, you must implement the `SortKey` trait for your target type, providing a key that maps to a `u64`. (This is implemented for `u64` by default).

### Installation

```toml
[dependencies]
arsort = "0.1.1"
```

### Basic Example

```rust
use arsort::prelude::*;

// Define a custom struct
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub id: u32,
    pub value: u64,
}

// Implement the trait to provide the u64 evaluation key
impl SortKey for Item {
    #[inline(always)]
    fn sort_key(&self) -> u64 {
        self.value
    }
}

fn main() {
    let mut data = vec![
        Item { id: 1, value: 500 },
        Item { id: 2, value: 100 },
        Item { id: 3, value: 300 },
    ];

    // The API only requires passing a mutable reference
    // (A thread-local buffer is managed automatically underneath)
    arsort(&mut data);

    assert_eq!(data[0].value, 100);
    assert_eq!(data[1].value, 300);
    assert_eq!(data[2].value, 500);
}
```

## Implementation Notes
* **Type Alignment & Safety:** Arsort enforces a compile-time check ensuring `std::mem::align_of::<T>() <= 8` (e.g., standard integers, pointers, or small structs containing up to 64-bit fields). Passing types requiring larger alignment (such as 128-bit/256-bit SIMD types) will fail cleanly at compile time with zero runtime overhead.
* **Payload Size:** This engine is heavily optimized for elements of **8 to 16 bytes** (e.g., pure `u64` or small `(u64, u32)` key-index pairs). Because it uses an out-of-place scatter routing (requiring a temporary buffer of equal size), larger structs (**32 bytes or more**) will rapidly exhaust the 32KB L1 data cache. For such large payloads, the memory copy overhead will completely erase the benefits of arithmetic routing, and you should use standard in-place sorting algorithms instead.
* **Stability:** This sorting algorithm is **unstable**. The original order of equal elements is not guaranteed to be preserved.
* **Micro-Array Optimization (`N <= 16`):** For extremely small arrays or terminal buckets during scatter routing, Arsort utilizes a custom, in-place insertion sort. By using raw pointer operations (`ptr::read` / `ptr::write`) instead of standard slice swaps, it coerces LLVM into performing register-level element shifting. Because Arsort strictly assumes tiny payloads (8-16 bytes), this avoids memory-to-memory copy overhead entirely, slightly outperforming the standard library's comparative fallback in the micro-domain.
* **Memory Safety & Soundness:** All pointer arithmetic, strict `&mut [MaybeUninit<T>]` workspace handling (including niche-optimized types like `NonZeroU64`), and re-entrant `SortKey` fallbacks are fully verified under `cargo +nightly miri test`, ensuring zero Undefined Behavior (UB), zero alignment violations, and panic-free re-entrancy.

## License

This project is licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))

at your option.
