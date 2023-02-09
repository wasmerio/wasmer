use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

/// Unique ID to identify a context.
///
/// Every handle to an object managed by a context also contains the ID of the
/// context. This is used to check that a handle is always used with the
/// correct context.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct StoreId(NonZeroU64);

impl Default for StoreId {
    // Allocates a unique ID for a new context.
    fn default() -> Self {
        // No overflow checking is needed here: overflowing this would take
        // thousands of years.
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NonZeroU64::new(NEXT_ID.fetch_add(1, Ordering::Relaxed)).unwrap())
    }
}
