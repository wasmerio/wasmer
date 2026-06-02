//! Micro-benchmarks for the per-thread `Mmap` pool.
//!
//! The pool's value comes from avoiding `mm_struct.mmap_lock` traffic
//! under concurrent `Mmap::accessible_reserved` / drop pairs. The
//! relevant signals are:
//!
//!   1. single-thread steady state - the pool always hits after the
//!      first iteration. Per-iteration cost is dominated by the
//!      `MADV_DONTNEED` reset rather than `mmap`/`munmap`.
//!   2. multi-thread contention - N threads independently allocating
//!      + dropping. The previous design serialised on the process-
//!      wide mmap_lock; the per-thread pool should give a near-flat
//!      scaling curve up to the point where the underlying `madvise`
//!      syscall itself becomes a bottleneck.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p wasmer-vm --bench mmap_pool_alloc
//! ```
//!
//! Linux-only payoff. The pool is gated on `target_os = "linux"`; on
//! other targets the benches still build but exercise the fallback
//! direct-mmap path.

use std::thread;

use criterion::{Criterion, criterion_group, criterion_main};
use wasmer_vm::{Mmap, MmapType};

/// One wasm linear memory's worth of mapping. Picks a size in the
/// same order of magnitude as a typical AssemblyScript tenant's
/// initial allocation.
const MAPPING_SIZE: usize = 64 * 1024;

fn one_alloc_free_cycle() {
    let m = Mmap::accessible_reserved(MAPPING_SIZE, MAPPING_SIZE, None, MmapType::Private)
        .expect("mmap");
    drop(m);
}

fn single_thread_steady_state(c: &mut Criterion) {
    c.bench_function("mmap_pool::alloc_drop/single_thread", |b| {
        b.iter(one_alloc_free_cycle);
    });
}

fn multi_thread_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("mmap_pool::alloc_drop/multi_thread");
    for &threads in &[1usize, 2, 4, 8] {
        group.bench_function(format!("{threads}_threads"), |b| {
            b.iter(|| {
                thread::scope(|s| {
                    for _ in 0..threads {
                        s.spawn(|| {
                            for _ in 0..64 {
                                one_alloc_free_cycle();
                            }
                        });
                    }
                });
            });
        });
    }
    group.finish();
}

criterion_group!(benches, single_thread_steady_state, multi_thread_contention);
criterion_main!(benches);
