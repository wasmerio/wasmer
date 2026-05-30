use core::fmt::Display;
use std::{
    cell::Cell,
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Unique ID to identify a context.
///
/// Every handle to an object managed by a context also contains the ID of the
/// context. This is used to check that a handle is always used with the
/// correct context.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct StoreId(NonZeroUsize);

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for StoreId {
    fn size_of_val(&self, _visited: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of_val(self)
    }
}

impl StoreId {
    /// Returns the raw [`NonZeroUsize`] value of this [`StoreId`].
    pub fn as_raw(&self) -> NonZeroUsize {
        self.0
    }
}

/// Number of IDs each thread reserves from the global counter at a time.
///
/// `Default::default` used to hit a single global `AtomicUsize` on every
/// call, so multiple `Store::new` callers on different cores would
/// ping-pong the same cache line. With chunked allocation, each thread
/// reserves `CHUNK_SIZE` consecutive IDs in one atomic step and then
/// hands them out from a thread-local cursor with zero cross-thread
/// traffic until the chunk is exhausted.
///
/// 256 keeps the global atomic out of the picture for any normal
/// workload while leaving the total ID space unchanged: the global
/// counter still grows by the same total amount, just in batches.
const CHUNK_SIZE: usize = 256;

/// Global pointer to the first ID of the next available chunk.
///
/// Starts at 1 so the first ID handed out is non-zero (the wrapper is
/// `NonZeroUsize`).
static NEXT_CHUNK_START: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    /// Per-thread cursor inside the currently-held chunk.
    ///
    /// Tuple is `(next_id_to_hand_out, end_of_chunk_exclusive)`. The
    /// initial `(0, 0)` triggers a chunk reservation on the first
    /// `Default::default` call for this thread.
    static LOCAL_CURSOR: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

impl Default for StoreId {
    // Allocates a unique ID for a new context.
    fn default() -> Self {
        // No overflow checking is needed here: the global counter is
        // `AtomicUsize`. On 64-bit hosts, exhausting it at one
        // `Store::new` per nanosecond on every core would still take
        // centuries.
        let raw = LOCAL_CURSOR.with(|cell| {
            let (mut next, mut end) = cell.get();
            if next == end {
                next = NEXT_CHUNK_START.fetch_add(CHUNK_SIZE, Ordering::Relaxed);
                end = next + CHUNK_SIZE;
            }
            cell.set((next + 1, end));
            next
        });
        Self(NonZeroUsize::new(raw).expect("chunked allocator never returns 0"))
    }
}

impl Display for StoreId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val: usize = self.0.into();
        if val == usize::MAX {
            write!(f, "unknown")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::thread;

    /// Many sequential `default()` calls from one thread produce unique
    /// IDs across at least three chunk boundaries (catches off-by-one
    /// errors at the chunk seam).
    #[test]
    fn ids_unique_within_a_thread_across_chunk_boundaries() {
        let n = CHUNK_SIZE * 3 + 17;
        let mut seen = HashSet::with_capacity(n);
        for _ in 0..n {
            let id = StoreId::default();
            assert!(
                seen.insert(id.as_raw()),
                "duplicate ID handed out within a single thread",
            );
        }
    }

    /// Two threads each pulling many chunks must never see overlap. This
    /// is the property the global `fetch_add` is preserving: the chunks
    /// are disjoint windows of the integer space.
    #[test]
    fn ids_unique_across_threads_under_load() {
        const THREADS: usize = 16;
        const PER_THREAD: usize = CHUNK_SIZE * 8;
        let collected: Mutex<HashSet<NonZeroUsize>> =
            Mutex::new(HashSet::with_capacity(THREADS * PER_THREAD));
        thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    let mut local = Vec::with_capacity(PER_THREAD);
                    for _ in 0..PER_THREAD {
                        local.push(StoreId::default().as_raw());
                    }
                    let mut guard = collected.lock().unwrap();
                    for id in local {
                        assert!(
                            guard.insert(id),
                            "duplicate ID handed out across threads: {id}",
                        );
                    }
                });
            }
        });
        let total = collected.into_inner().unwrap().len();
        assert_eq!(
            total,
            THREADS * PER_THREAD,
            "expected every produced ID to be unique",
        );
    }

    /// `StoreId` is `NonZeroUsize`-backed, so the allocator must never
    /// hand out zero. The chunk start is initialised to 1 and chunk
    /// reservations only ever increase it, so a zero would indicate a
    /// regression in chunk bookkeeping.
    #[test]
    fn allocator_never_returns_zero() {
        for _ in 0..10_000 {
            let id = StoreId::default();
            assert_ne!(id.as_raw().get(), 0);
        }
    }
}
