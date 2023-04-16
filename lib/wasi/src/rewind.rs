use std::pin::Pin;

use bytes::Bytes;
use futures::Future;
use wasmer::{AsStoreMut, AsStoreRef, MemorySize};
use wasmer_wasix_types::wasi::{Errno, ExitCode};

use crate::{
    syscalls::{get_memory_stack, set_memory_stack},
    WasiEnv, WasiFunctionEnv,
};

/// Future that will be polled by asyncify methods
#[doc(hidden)]
pub type AsyncifyFuture = dyn Future<Output = Result<(), Errno>> + Send + Sync + 'static;

/// Trait that will be invoked after the rewind has finished
/// It is possible that the process will be terminated rather
/// than restored at this point
#[doc(hidden)]
pub trait RewindPostProcess {
    fn finish(
        &mut self,
        env: &WasiEnv,
        store: &dyn AsStoreRef,
        res: Result<(), Errno>,
    ) -> Result<(), ExitCode>;
}

/// The rewind state after a deep sleep
#[doc(hidden)]
pub struct RewindState {
    /// Memory stack used to restore the stack trace back to where it was
    pub memory_stack: Bytes,
    /// Call stack used to restore the stack trace back to where it was
    pub rewind_stack: Bytes,
    /// All the global data stored in the store
    pub store_data: Bytes,
    /// Flag that indicates if this rewind is 64-bit or 32-bit memory based
    pub is_64bit: bool,
    /// This is the function that's invoked after the work is finished
    /// and the rewind has been applied.
    pub finish: Box<dyn RewindPostProcess + Send + Sync + 'static>,
}

impl RewindState {
    #[doc(hidden)]
    pub fn rewinding_finish<M: MemorySize>(
        &mut self,
        ctx: &WasiFunctionEnv,
        store: &mut impl AsStoreMut,
        res: Result<(), Errno>,
    ) -> Result<(), ExitCode> {
        let mut ctx = ctx.env.clone().into_mut(store);
        let (env, mut store) = ctx.data_and_store_mut();
        set_memory_stack::<M>(env, &mut store, self.memory_stack.clone()).map_err(|err| {
            tracing::error!("failed on rewinding_finish - {}", err);
            ExitCode::Errno(Errno::Memviolation)
        })?;
        let ret = self.finish.finish(env, &store, res);
        if ret.is_ok() {
            self.memory_stack = get_memory_stack::<M>(env, &mut store)
                .map_err(|err| {
                    tracing::error!("failed on rewinding_finish - {}", err);
                    ExitCode::Errno(Errno::Memviolation)
                })?
                .freeze();
        }
        ret
    }
}

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
