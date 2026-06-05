//! Micro-benchmarks for `StoreId::default()`.
//!
//! The chunked thread-local allocator amortizes the global atomic
//! cross every `CHUNK_SIZE` allocations, so the relevant signals are:
//!
//!   1. **single-thread hot path** — every call returns from the
//!      TLS cursor except 1-in-`CHUNK_SIZE` that hits the global
//!      atomic.
//!   2. **multi-thread contention** — N threads racing on
//!      `Default::default` concurrently. The previous design hit the
//!      global `AtomicUsize` on every call and cross-core ping-pong
//!      dominated the cost; the chunked design should show near-flat
//!      scaling.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p wasmer-types --bench store_id_alloc
//! ```

use std::thread;

use criterion::{Criterion, criterion_group, criterion_main};
use wasmer_types::StoreId;

fn single_thread_default(c: &mut Criterion) {
    c.bench_function("store_id::default/single_thread", |b| {
        b.iter(StoreId::default);
    });
}

fn multi_thread_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("store_id::default/multi_thread");
    // Sample at a few worker counts so the scaling curve is visible
    // in the report.
    for &threads in &[1usize, 2, 4, 8] {
        group.bench_function(format!("{threads}_threads"), |b| {
            b.iter(|| {
                thread::scope(|s| {
                    for _ in 0..threads {
                        s.spawn(|| {
                            // Burn a CHUNK_SIZE-class run per worker
                            // so the per-iter timing is dominated by
                            // the steady-state TLS path plus a
                            // handful of global-atomic refills.
                            for _ in 0..1024 {
                                let _ = std::hint::black_box(StoreId::default());
                            }
                        });
                    }
                });
            });
        });
    }
    group.finish();
}

criterion_group!(benches, single_thread_default, multi_thread_default);
criterion_main!(benches);
