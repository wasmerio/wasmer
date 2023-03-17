use std::{
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

impl Default for StoreId {
    // Allocates a unique ID for a new context.
    fn default() -> Self {
        // No overflow checking is needed here: overflowing this would take
        // thousands of years.
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        Self(NonZeroUsize::new(NEXT_ID.fetch_add(1, Ordering::Relaxed)).unwrap())
    }
}
