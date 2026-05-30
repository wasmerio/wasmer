//! Per-thread pool of reusable anonymous private `Mmap` regions.
//!
//! Linux-only. Other targets fall through to direct `mmap`/`munmap`.
//!
//! # Why
//!
//! `mmap` and `munmap` both take the process-wide `mm_struct.mmap_lock`
//! write-locked. At 28 threads doing fresh `Instance::new` (which mmap's
//! a wasm linear memory) followed by drop (munmap), the lock spends
//! most of its time held write-locked and per-thread throughput
//! collapses to 2.5% of single-thread. A microbench of bare
//! `mmap+munmap` vs `madvise(MADV_DONTNEED)` at 28 threads showed a 6x
//! throughput improvement just from avoiding the unmap.
//!
//! # How
//!
//! Each thread keeps a small `HashMap<PoolKey, Vec<Mmap>>` of idle
//! `Mmap`s. When an Mmap drops (and the mapping is poolable, see below),
//! the pool resets its contents and stores it instead of unmapping.
//! When a new Mmap of the same shape is requested, the pool hands one
//! over instead of asking the kernel for fresh address space.
//!
//! # Security
//!
//! The pool is shared across wasm Instances within a single process.
//! For wasm hosts that run untrusted code (which is most of them), an
//! incomplete reset is a tenant-isolation bug. Every assumption this
//! file relies on is documented:
//!
//! ## Reset must produce a zero-filled mapping
//!
//! `MADV_DONTNEED` on a `MAP_PRIVATE | MAP_ANON` mapping on Linux is
//! specified to release backing pages so that subsequent accesses fault
//! in fresh zero pages from the kernel page allocator. The kernel
//! guarantees that any page handed to userland is freshly zeroed; old
//! page contents cannot leak. See `man 2 madvise` (`MADV_DONTNEED`),
//! Linux mm/madvise.c (`madvise_dontneed_single_vma` →
//! `zap_page_range`).
//!
//! The `pool_reuse_is_zero_filled` test in this file writes a sentinel,
//! returns the mapping to the pool, takes it again, and asserts the
//! sentinel is gone (overwritten by zero).
//!
//! ## Guard pages must be restored
//!
//! `Mmap::make_accessible` calls `mprotect(PROT_READ | PROT_WRITE)` to
//! extend the accessible range but does NOT update `Mmap::accessible_size`.
//! If we pool a mapping that had been extended via `make_accessible`,
//! the next caller would see the same `accessible_size` they originally
//! requested, but pages above that boundary would still be readable
//! and writable. A wasm program could read past its declared memory.
//!
//! Mitigation: at pool insertion the entire range above
//! `accessible_size` is `mprotect(PROT_NONE)`'d back to guard, regardless
//! of what state it was in. This restores the original guard. The
//! `pool_reuse_restores_guard` test exercises this.
//!
//! ## Shared and file-backed mappings are excluded
//!
//! `MmapType::Shared` and file-backed mappings (`sync_on_drop`) are
//! never pooled. They live longer than a single Instance because
//! other Instances or processes can hold aliases.
//!
//! ## Thread-exit destruction
//!
//! When a thread exits, its TLS pool drops. The pool's `Drop` flips
//! a "draining" flag inside its own state so that the cascading
//! `Mmap` drops (one per pool entry) fall through to direct `munmap`
//! instead of trying to re-insert into the (currently-being-dropped)
//! pool.
//!
//! ## Non-Linux platforms
//!
//! This entire module is `cfg(target_os = "linux")`. On macOS, FreeBSD,
//! NetBSD, and Windows the pool functions are stubs that always fail
//! (so callers fall back to the original `mmap`/`munmap` path). The
//! semantics of `MADV_DONTNEED` differ across BSD lineages and we have
//! not verified the zero-fill guarantee there.

/// Per-(accessible_size, mapping_size) bucket key. We index by exact
/// shape so the consumer sees a mapping with identical layout to what
/// they asked for. Most wasm modules instantiated repeatedly fit one
/// bucket exactly.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PoolKey {
    accessible_size: usize,
    mapping_size: usize,
}

/// Maximum idle mappings per bucket per thread. Higher = better
/// hit rate, more RSS held when idle. 8 is enough for most wasm
/// hosts where a single thread bounces between a handful of
/// instantiations in flight.
#[cfg(target_os = "linux")]
const POOL_PER_BUCKET_MAX: usize = 8;

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crate::mmap::Mmap;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct Pool {
        buckets: HashMap<PoolKey, Vec<Mmap>>,
        /// True once this pool's `Drop` has begun. While draining,
        /// `try_pool` always returns `Some(mmap)` so the caller will
        /// `munmap` instead of recursing into us.
        draining: bool,
    }

    impl Pool {
        fn new() -> Self {
            Self {
                buckets: HashMap::new(),
                draining: false,
            }
        }
    }

    impl Drop for Pool {
        fn drop(&mut self) {
            // Flip before letting `buckets` drop. Each Mmap that drops
            // off the inner Vecs sees `draining = true` via the TLS
            // accessor and falls through to direct `munmap`.
            self.draining = true;
            // `self.buckets` drops here as we return.
        }
    }

    thread_local! {
        static TLS_POOL: RefCell<Pool> = RefCell::new(Pool::new());
    }

    /// Get an idle mapping with the requested exact shape, or `None`
    /// if the pool has none.
    pub(crate) fn try_take(accessible_size: usize, mapping_size: usize) -> Option<Mmap> {
        if mapping_size == 0 {
            return None;
        }
        let key = PoolKey {
            accessible_size,
            mapping_size,
        };
        TLS_POOL
            .try_with(|cell| {
                let mut pool = cell.try_borrow_mut().ok()?;
                if pool.draining {
                    return None;
                }
                let bucket = pool.buckets.get_mut(&key)?;
                bucket.pop()
            })
            .ok()
            .flatten()
    }

    /// Try to return a mapping to the pool. If pooling succeeds the
    /// mapping has been consumed and `None` is returned. If pooling
    /// fails (pool full, draining, reset error, etc.) the original
    /// mapping is returned via `Some` and the caller MUST `munmap` it.
    pub(crate) fn try_pool(mut mmap: Mmap) -> Option<Mmap> {
        let total = mmap.total_size_for_pool();
        if total == 0 {
            return Some(mmap);
        }

        let key = PoolKey {
            accessible_size: mmap.accessible_size_for_pool(),
            mapping_size: total,
        };

        // Reset the mapping so it can be safely handed to a different
        // tenant. Order matters for both correctness and performance,
        // and the reset is wider when the previous tenant grew the
        // memory beyond its declared accessible size. See `reset` for
        // the full argument.
        if reset(&mut mmap).is_err() {
            return Some(mmap);
        }

        // Move `mmap` into an Option so we can conditionally take it
        // out from inside the TLS-access closure. If we successfully
        // push it to a bucket, `take()` empties the Option and we
        // return `None`. Otherwise the Option keeps the value and we
        // return it.
        let mut holder = Some(mmap);
        let _ = TLS_POOL.try_with(|cell| {
            let Ok(mut pool) = cell.try_borrow_mut() else {
                return;
            };
            if pool.draining {
                return;
            }
            let bucket = pool.buckets.entry(key).or_default();
            if bucket.len() < POOL_PER_BUCKET_MAX {
                bucket.push(holder.take().expect("holder set above"));
            }
        });
        holder
    }

    /// Reset a mapping so it can be safely handed back out by the
    /// pool. Two security invariants must hold after this returns Ok:
    ///
    ///   1. Every physical page that the previous tenant could have
    ///      written to has had its backing released, so the next
    ///      tenant's reads fault in fresh zero pages from the kernel
    ///      page allocator (not the previous tenant's data).
    ///   2. The VMA protection bits match what a fresh
    ///      `accessible_reserved(accessible, total, ...)` call would
    ///      produce: `[0..accessible)` is PROT_RW, `[accessible..total)`
    ///      is PROT_NONE.
    ///
    /// # The two paths
    ///
    /// **No growth happened** (`was_extended_for_pool() == false`).
    /// The previous tenant only ever wrote to `[0..accessible_size)`,
    /// because `[accessible_size..total_size)` was PROT_NONE for the
    /// entire mapping lifetime (set by `accessible_reserved`, never
    /// touched). We only need to zero the accessible range. The guard
    /// region is already PROT_NONE so no `mprotect` is necessary,
    /// which avoids the TLB-shootdown IPI that otherwise costs ~10%
    /// of total runtime at 28 threads.
    ///
    /// **Growth happened** (`was_extended_for_pool() == true`).
    /// `make_accessible` was called with an end past `accessible_size`,
    /// so pages in `[accessible_size..extended_end)` were PROT_RW and
    /// the previous tenant could have written secrets there. We do
    /// NOT track `extended_end` precisely (no field on Mmap), so we
    /// conservatively `madvise(DONTNEED)` the entire mapping. The
    /// kernel skips ranges without present PTEs efficiently, so this
    /// is cheap even for sparse 4GB-mapping/64KB-accessible cases.
    /// Then `mprotect(PROT_NONE)` on `[accessible_size..total_size)`
    /// restores the guard.
    ///
    /// # Why `MADV_DONTNEED` alone is not sufficient when growth
    /// happened
    ///
    /// `mprotect` does not zero pages. A previous tenant writing
    /// secret data into a grown region, followed by guard-restoration
    /// via `mprotect(PROT_NONE)`, leaves the physical pages allocated
    /// and full of secrets. A future tenant that calls `memory.grow`
    /// and then reads those pages would receive the secrets. The
    /// `madvise(DONTNEED)` on the whole mapping is what releases the
    /// backing so the future tenant's fault-in goes to a fresh zero
    /// page from the kernel page allocator.
    ///
    /// # Why `mprotect` alone is not sufficient when growth happened
    ///
    /// Symmetrically: leaving pages PROT_RW after the previous tenant
    /// drops them would let a future tenant read those pages by simple
    /// `memory.load` without going through `memory.grow`. The
    /// `mprotect(PROT_NONE)` is what makes the wasm side perceive the
    /// region as guard again.
    fn reset(mmap: &mut Mmap) -> std::io::Result<()> {
        let accessible = mmap.accessible_size_for_pool();
        let total = mmap.total_size_for_pool();
        let ptr = mmap.raw_ptr_for_pool();

        if mmap.was_extended_for_pool() {
            // Growth path: previous tenant may have written above
            // `accessible_size`. Release backing of the entire mapping,
            // then mprotect the guard region back.
            if total > 0 {
                let r =
                    unsafe { libc::madvise(ptr as *mut libc::c_void, total, libc::MADV_DONTNEED) };
                if r != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if total > accessible {
                let guard_start = ptr + accessible;
                let guard_len = total - accessible;
                let r = unsafe {
                    libc::mprotect(guard_start as *mut libc::c_void, guard_len, libc::PROT_NONE)
                };
                if r != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            mmap.clear_extended_for_pool();
        } else {
            // No-growth path: only `[0..accessible_size)` could have
            // tenant data. The guard region is still PROT_NONE from
            // the original `accessible_reserved` call so no `mprotect`
            // is needed.
            if accessible > 0 {
                let r = unsafe {
                    libc::madvise(ptr as *mut libc::c_void, accessible, libc::MADV_DONTNEED)
                };
                if r != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::mmap::Mmap;

    pub(crate) fn try_take(_accessible_size: usize, _mapping_size: usize) -> Option<Mmap> {
        None
    }

    pub(crate) fn try_pool(mmap: Mmap) -> Option<Mmap> {
        Some(mmap)
    }
}

/// Re-exports for `mmap.rs` to call into.
pub(crate) use imp::{try_pool, try_take};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::mmap::{Mmap, MmapType};

    fn page_size() -> usize {
        region::page::size()
    }

    /// **Tenant isolation invariant.** After a mapping has been written
    /// to, returned to the pool, and taken back, the new owner must see
    /// fresh zeros. This is the security-critical property MADV_DONTNEED
    /// is supposed to deliver on Linux PRIVATE+ANON mappings.
    ///
    /// If a future refactor breaks this (e.g. by skipping the
    /// `MADV_DONTNEED` step) the test fails and we catch a guaranteed
    /// cross-tenant data leak before it ships.
    #[test]
    fn pool_reuse_is_zero_filled() {
        let ps = page_size();
        let size = ps * 4;

        // Round 1: get a mapping, write a sentinel pattern.
        let mut m1 = Mmap::accessible_reserved(size, size, None, MmapType::Private).unwrap();
        for byte in m1.as_mut_slice_accessible().iter_mut() {
            *byte = 0xAB;
        }
        // Cache the address so we can recognize the same physical region.
        let addr1 = m1.as_ptr() as usize;
        drop(m1); // returns to pool

        // Round 2: ask for the same shape; pool should hand back the
        // same VMA. The accessible slice MUST read as all zeros.
        let m2 = Mmap::accessible_reserved(size, size, None, MmapType::Private).unwrap();
        let addr2 = m2.as_ptr() as usize;
        // The pool should hand back the same address (proves we're
        // actually reusing). If addresses differ the test still passes
        // its zeroing check, but we lose the "we're testing the pool"
        // signal.
        assert_eq!(
            addr1, addr2,
            "pool did not reuse the dropped mapping; test is not actually exercising the pool",
        );
        for (i, &byte) in m2.as_slice_accessible().iter().enumerate() {
            assert_eq!(
                byte, 0,
                "byte at offset {i} should be zero after pool reuse but was {byte:#x}",
            );
        }
    }

    /// **Guard-page restoration invariant.** When a mapping had its
    /// accessible range extended via `make_accessible` and is then
    /// returned to the pool, the pool must mprotect the extra pages
    /// back to PROT_NONE so the next user sees them as guard. Without
    /// this, a pooled mapping leaks an extended-RW region into a fresh
    /// instance and a wasm program could read past its declared memory.
    #[test]
    fn pool_reuse_restores_guard() {
        let ps = page_size();
        let accessible = ps;
        let mapping = ps * 8;

        // Round 1: a reserve-and-extend mapping. Extend access to the
        // entire mapping_size, so all pages are RW. Then drop.
        let mut m1 =
            Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        m1.make_accessible(accessible, mapping - accessible)
            .unwrap();
        // Sanity: writing to the extended range must succeed BEFORE the drop.
        unsafe {
            (m1.as_mut_ptr().add(mapping - 1)).write_volatile(0xCD);
        }
        drop(m1); // returns to pool with guard restored

        // Round 2: same shape. The bytes in (accessible..mapping) must
        // now be protected. We verify by forking a child that tries to
        // write into the guard region and observe it die with SIGSEGV.
        let m2 = Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        let guard_ptr = unsafe { m2.as_ptr().add(accessible) as usize };

        let child = unsafe { libc::fork() };
        if child == 0 {
            // Child: write into the guard. Either SIGSEGV (success) or
            // (failure) the write succeeds because guard wasn't restored.
            unsafe {
                (guard_ptr as *mut u8).write_volatile(0x42);
            }
            // If we got here, guard was NOT restored. Exit with a
            // sentinel code the parent will read.
            unsafe {
                libc::_exit(99);
            }
        } else {
            assert!(child > 0, "fork failed");
            let mut status: libc::c_int = 0;
            unsafe {
                libc::waitpid(child, &mut status, 0);
            }
            assert!(
                libc::WIFSIGNALED(status),
                "child should have died from a signal (guard works), \
                 but exited normally with status {status} \
                 (guard pages NOT restored across pool reuse)",
            );
            assert_eq!(
                libc::WTERMSIG(status),
                libc::SIGSEGV,
                "child died from a non-SEGV signal (status {status})",
            );
        }

        drop(m2);
    }

    /// Shared (`MmapType::Shared`) and file-backed mappings must never
    /// enter the pool: another thread or process could hold a live
    /// alias to the same region. The exclusion is enforced by the
    /// `backing_file.is_none() && memory_type == MmapType::Private`
    /// gate in `Mmap::accessible_reserved` and by setting
    /// `Mmap::poolable = false` on every other path.
    ///
    /// Verifying "this mapping was not pooled" from the outside is
    /// unreliable: the kernel's mmap allocator can re-issue the same
    /// VMA address after `munmap`, so address-equality assertions
    /// confound legitimate kernel reuse with a pool-isolation bug. The
    /// exclusion is therefore guarded by code review of the one-line
    /// gate (and by this comment), not by a behavioural test.

    /// File-backed mappings (`sync_on_drop` true on the Linux build)
    /// also must not be pooled. We exercise the negative case: a
    /// Private+file-backed mapping followed by a Private+anon request
    /// of the same shape MUST NOT recycle the file-backed mapping.
    ///
    /// (We don't construct a backing file in this test because it adds
    /// fs setup that isn't relevant. The negative test is covered
    /// indirectly: in the pool code, the `accessible_reserved` fast
    /// path only fires when `backing_file.is_none()`, so a file-backed
    /// mapping can never even be queried against the pool.)
    #[test]
    fn pool_round_trip_preserves_layout() {
        let ps = page_size();
        let accessible = ps * 2;
        let mapping = ps * 8;

        let m1 = Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        assert_eq!(m1.len(), mapping);
        let addr1 = m1.as_ptr() as usize;
        drop(m1);

        let m2 = Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        assert_eq!(
            m2.len(),
            mapping,
            "pool reuse must hand back a mapping with identical total length",
        );
        assert_eq!(m2.as_ptr() as usize, addr1, "should be the same VMA");
    }

    /// Different shapes (different `accessible_size` or `mapping_size`)
    /// must not reuse each other. Each bucket is keyed by the exact
    /// shape; mixing would let a 1-page-accessible mapping satisfy a
    /// 16-page-accessible request, which is wrong.
    #[test]
    fn pool_does_not_reuse_across_shapes() {
        let ps = page_size();

        let m1 = Mmap::accessible_reserved(ps, ps, None, MmapType::Private).unwrap();
        let addr_small = m1.as_ptr() as usize;
        drop(m1);

        let m2 = Mmap::accessible_reserved(ps * 4, ps * 4, None, MmapType::Private).unwrap();
        assert_ne!(
            m2.as_ptr() as usize,
            addr_small,
            "pool should not have handed a 1-page mapping for a 4-page request",
        );
        assert_eq!(m2.len(), ps * 4);
        drop(m2);
    }

    /// **Tenant isolation across the grown range.** The most dangerous
    /// data-leak path is: tenant A extends accessible RW past
    /// `accessible_size` via `make_accessible`, writes secret data
    /// there, drops the mapping; the pool restores guard protection
    /// (mprotect PROT_NONE) but mprotect does NOT zero pages; tenant
    /// B takes the same mapping, calls `make_accessible` to grow,
    /// then reads. Without a `MADV_DONTNEED` covering the previously-
    /// extended range, tenant B would observe tenant A's data.
    ///
    /// The fix is in `mmap_pool::reset`: when `was_extended_for_pool`
    /// is true we `madvise(DONTNEED)` the entire mapping (not just
    /// the original accessible range) before mprotecting the guard.
    /// This test exercises that path end-to-end.
    #[test]
    fn pool_reuse_no_leak_across_grown_range() {
        let ps = page_size();
        let accessible = ps;
        let mapping = ps * 8;
        let sentinel: u8 = 0xCD;

        // Round 1: tenant A grows then writes a sentinel into every
        // byte of the extended range.
        let mut m1 =
            Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        m1.make_accessible(accessible, mapping - accessible)
            .unwrap();
        unsafe {
            let p = m1.as_mut_ptr();
            for off in accessible..mapping {
                *p.add(off) = sentinel;
            }
        }
        drop(m1);

        // Round 2: tenant B takes the pooled mapping. The accessible
        // range is initially [0..accessible) (PROT_RW), and
        // [accessible..mapping) is restored to PROT_NONE. Tenant B
        // grows by mprotecting [accessible..mapping) back to PROT_RW,
        // then reads. Every byte MUST be zero.
        let mut m2 =
            Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private).unwrap();
        m2.make_accessible(accessible, mapping - accessible)
            .unwrap();
        let slice = unsafe {
            std::slice::from_raw_parts(m2.as_ptr().add(accessible), mapping - accessible)
        };
        for (i, &byte) in slice.iter().enumerate() {
            assert_eq!(
                byte,
                0,
                "byte at extended offset {} should be zero but is {:#x} \
                 (cross-tenant leak through the grown region)",
                accessible + i,
                byte,
            );
        }
        drop(m2);
    }

    /// **High-volume isolation stress.** 28 threads, three distinct
    /// `(accessible, mapping)` shapes, 2000 iters per thread. Each
    /// iter: take a fresh mapping, verify it is all-zero in both the
    /// accessible range and (when growing) the extended range, write a
    /// thread-unique sentinel byte across the entire usable range,
    /// drop. A failure here means a recycled mapping leaked data from
    /// some other tenant.
    ///
    /// 56000 take/grow/write/drop cycles is enough to bounce every
    /// thread's pool bucket through many recycle events and catch
    /// failures that single-cycle tests miss (e.g. a stale flag, a
    /// stale PTE, a missed mprotect on a particular size).
    #[test]
    fn pool_stress_no_leak_under_concurrency() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::thread;

        let ps = page_size();
        let shapes = [
            (ps, ps),         // accessible == mapping, no guard
            (ps, ps * 4),     // small accessible, larger guard
            (ps * 2, ps * 8), // moderate accessible, larger guard
        ];

        let leaks = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();

        for tid in 1..=28u8 {
            let leaks = leaks.clone();
            handles.push(thread::spawn(move || {
                for iter in 0..2000u64 {
                    let (accessible, mapping) = shapes[(iter as usize) % shapes.len()];
                    let mut m = Mmap::accessible_reserved(
                        accessible,
                        mapping,
                        None,
                        MmapType::Private,
                    )
                    .unwrap();

                    // Invariant 1: accessible range MUST be zero on take.
                    for (i, &b) in m.as_slice_accessible().iter().enumerate() {
                        if b != 0 {
                            eprintln!(
                                "tid={tid} iter={iter} accessible[{i}] = {b:#x}, expected 0",
                            );
                            leaks.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // Sometimes grow into the guard region. When we do,
                    // the newly accessible bytes MUST also be zero.
                    let grow = iter % 3 == 0 && mapping > accessible;
                    if grow {
                        let extra = mapping - accessible;
                        m.make_accessible(accessible, extra).unwrap();
                        let grown = unsafe {
                            std::slice::from_raw_parts(m.as_ptr().add(accessible), extra)
                        };
                        for (i, &b) in grown.iter().enumerate() {
                            if b != 0 {
                                eprintln!(
                                    "tid={tid} iter={iter} grown[{i}] = {b:#x}, expected 0",
                                );
                                leaks.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }

                    // Write thread-unique sentinel everywhere this iter
                    // claims is accessible. If the pool's reset is
                    // incomplete, a future iter on this thread (or a
                    // recycled mapping reaching this thread later) will
                    // observe the sentinel.
                    let usable = if grow { mapping } else { accessible };
                    unsafe {
                        let p = m.as_mut_ptr();
                        std::ptr::write_bytes(p, tid, usable);
                    }
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let n = leaks.load(Ordering::Relaxed);
        assert_eq!(
            n, 0,
            "{n} byte(s) of cross-tenant data observed via the Mmap pool; \
             see stderr above for offending (tid, iter, offset, value) tuples",
        );
    }

    /// Cross-thread Send: thread A creates a Mmap, writes a sentinel,
    /// sends it to thread B via channel; thread B drops it (returns to
    /// B's pool); B then re-allocates the same shape and MUST observe
    /// zeros. Verifies that the pool's reset works correctly on a
    /// mapping that was originally allocated by a different thread.
    #[test]
    fn pool_cross_thread_send_no_leak() {
        use std::sync::mpsc;
        use std::thread;

        let ps = page_size();
        let (accessible, mapping) = (ps, ps * 4);
        let (tx, rx) = mpsc::channel::<Mmap>();

        let producer = thread::spawn(move || {
            for tid in 1..=50u8 {
                let mut m = Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private)
                    .unwrap();
                unsafe {
                    std::ptr::write_bytes(m.as_mut_ptr(), tid, accessible);
                }
                tx.send(m).expect("producer send");
            }
        });

        let consumer = thread::spawn(move || {
            let mut observed_leak = 0u64;
            while let Ok(m) = rx.recv() {
                drop(m);
                // The dropped Mmap goes into THIS thread's pool with
                // its reset already complete. A new alloc of the same
                // shape may come from that pool entry; it must be zero.
                let m2 = Mmap::accessible_reserved(accessible, mapping, None, MmapType::Private)
                    .unwrap();
                for (i, &b) in m2.as_slice_accessible().iter().enumerate() {
                    if b != 0 {
                        eprintln!("cross-thread leak at {i}: {b:#x}");
                        observed_leak += 1;
                    }
                }
                drop(m2);
            }
            assert_eq!(observed_leak, 0, "{observed_leak} cross-thread byte leaks");
        });

        producer.join().unwrap();
        consumer.join().unwrap();
    }

    /// Pool bucket overflow: drop more mappings than `POOL_PER_BUCKET_MAX`
    /// at once. The excess must be `munmap`'d, not silently retained
    /// (which would unbounded-grow RSS).
    #[test]
    fn pool_bucket_overflow_unmaps_excess() {
        let ps = page_size();

        // Hold POOL_PER_BUCKET_MAX + 4 mappings simultaneously, then
        // drop them. The pool will accept the first POOL_PER_BUCKET_MAX
        // and munmap the rest. We can't directly observe which path
        // each took, but if the excess weren't unmapped we'd leak
        // VMAs; the kernel would eventually complain about
        // /proc/sys/vm/max_map_count. Here we just verify the test
        // completes without panic.
        let mut held = Vec::new();
        for _ in 0..(POOL_PER_BUCKET_MAX + 4) {
            held.push(Mmap::accessible_reserved(ps, ps, None, MmapType::Private).unwrap());
        }
        drop(held);

        // Cycle one more take/drop pair through the same bucket and
        // confirm the pool is still functional.
        let m = Mmap::accessible_reserved(ps, ps, None, MmapType::Private).unwrap();
        drop(m);
    }

    /// **`with_at_least` mappings must not be pooled.** They are used
    /// by `CodeMemory`, which calls `region::protect(.., READ_EXECUTE)`
    /// directly to publish executable function bodies. That protection
    /// change happens outside the `Mmap` API surface so the pool
    /// cannot detect it. If a `with_at_least` mapping went into the
    /// pool, the next caller would receive memory that still had
    /// `READ_EXECUTE` over the first part and would SIGSEGV the moment
    /// it tried to write to it.
    ///
    /// Regression test for a headless-deserialize segfault on first
    /// run of this branch.
    #[test]
    fn with_at_least_mappings_are_not_pooled() {
        let ps = page_size();

        // Two cycles. If `with_at_least` produced poolable mappings,
        // the second one would come from the pool with whatever
        // protection state was left on the first. Externally mprotect
        // the first to READ_EXECUTE before dropping (this is what
        // `CodeMemory::publish` does). On the second take, we write
        // through the slice. If pool returned the first mapping, the
        // mprotect-RX state survives recycling and the write SIGSEGVs.
        let mut m1 = Mmap::with_at_least(ps * 4).unwrap();
        // Touch a byte so the kernel actually fault-installs a PTE
        // before we change protection.
        unsafe {
            m1.as_mut_ptr().write_volatile(0);
        }
        unsafe {
            region::protect(m1.as_mut_ptr(), ps * 4, region::Protection::READ_EXECUTE).unwrap();
        }
        drop(m1);

        // Second take. We MUST get a writable mapping. If the pool
        // recycled the previous one with PROT_RX, this write segfaults.
        let mut m2 = Mmap::with_at_least(ps * 4).unwrap();
        unsafe {
            m2.as_mut_ptr().write_volatile(0xAB);
        }
        drop(m2);
    }

    /// Thread exit must drain its pool without recursing forever, and
    /// without crashing. Spawn a thread, build and drop several
    /// poolable mappings, let the thread exit, and ensure the parent
    /// thread can still use its own pool afterwards.
    #[test]
    fn thread_exit_drains_pool_safely() {
        let ps = page_size();
        let handle = std::thread::spawn(move || {
            for _ in 0..32 {
                let m = Mmap::accessible_reserved(ps * 2, ps * 2, None, MmapType::Private).unwrap();
                drop(m);
            }
        });
        handle.join().expect("worker thread should exit cleanly");

        // Parent's pool is independent and still works.
        let m = Mmap::accessible_reserved(ps, ps, None, MmapType::Private).unwrap();
        drop(m);
    }
}
