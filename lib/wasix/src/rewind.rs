use std::pin::Pin;

use bytes::Bytes;
use futures::Future;
use wasmer_wasix_types::{
    wasi::Errno,
    wasix::{ThreadStartType, WasiMemoryLayout},
};

use crate::os::task::thread::RewindResultType;

/// Future that will be polled by asyncify methods
#[doc(hidden)]
pub type AsyncifyFuture = dyn Future<Output = Bytes> + Send + Sync + 'static;

/// Trait that will be invoked after the rewind has finished
/// It is possible that the process will be terminated rather
/// than restored at this point
pub trait RewindPostProcess {
    /// Returns the serialized object that is returned on the rewind
    fn finish(&mut self, res: Result<(), Errno>) -> Bytes;
}

/// The rewind state after a deep sleep
pub struct RewindState {
    /// Memory stack used to restore the stack trace back to where it was
    pub memory_stack: Bytes,
    /// Call stack used to restore the stack trace back to where it was
    pub rewind_stack: Bytes,
    /// All the global data stored in the store
    pub store_data: Bytes,
    /// Describes the type of thread start
    pub start: ThreadStartType,
    /// Layout of the memory,
    pub layout: WasiMemoryLayout,
    /// Flag that indicates if this rewind is 64-bit or 32-bit memory based
    pub is_64bit: bool,
}

pub type RewindStateOption = Option<(RewindState, RewindResultType)>;

/// Represents the work that will be done when a thread goes to deep sleep and
/// includes the things needed to restore it again
pub struct DeepSleepWork {
    /// This is the work that will be performed before the thread is rewoken
    pub trigger: Pin<Box<AsyncifyFuture>>,
    /// State that the thread will be rewound to
    pub rewind: RewindState,
}
impl std::fmt::Debug for DeepSleepWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "deep-sleep-work(memory_stack_len={}, rewind_stack_len={}, store_size={})",
            self.rewind.memory_stack.len(),
            self.rewind.rewind_stack.len(),
            self.rewind.store_data.len()
        )
    }
}
