use std::{
    collections::BTreeMap,
    sync::{RwLock, atomic::AtomicU64},
};

use futures::{
    TryFutureExt,
    channel::oneshot::{self, Sender},
};
use thiserror::Error;
use wasmer::RuntimeError;

use crate::utils::thread_local_executor::{ThreadLocalSpawner, ThreadLocalSpawnerError};

#[derive(Debug)]
pub(crate) struct ContextSwitchingContext {
    /// TODO: Document these fields
    unblockers: RwLock<BTreeMap<u64, Sender<Result<(), RuntimeError>>>>,
    current_context_id: AtomicU64,
    next_available_context_id: AtomicU64,
    spawner: ThreadLocalSpawner,
}

#[derive(Debug, Error)]
pub enum ContextSwitchError {
    #[error("Target context to switch to is missing")]
    SwitchTargetMissing,
    #[error("Failed to unblock target context")]
    SwitchUnblockFailed,
    #[error("Own context is already blocked")]
    OwnContextAlreadyBlocked,
}

#[derive(Error, Debug)]
#[error("Context was cancelled")]
pub struct ContextCancelled();

impl ContextSwitchingContext {
    pub(crate) fn new(spawner: ThreadLocalSpawner) -> Self {
        Self {
            unblockers: RwLock::new(BTreeMap::new()),
            current_context_id: AtomicU64::new(0),
            next_available_context_id: AtomicU64::new(1),
            spawner,
        }
    }

    pub(crate) fn active_context_id(&self) -> u64 {
        self.current_context_id
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn set_active_context_id(&self, context_id: u64) {
        self.current_context_id
            .store(context_id, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn allocate_new_context_id(&self) -> u64 {
        self.next_available_context_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn remove_unblocker(
        &self,
        target_context_id: &u64,
    ) -> Option<Sender<Result<(), RuntimeError>>> {
        self.unblockers.write().unwrap().remove(target_context_id)
    }

    /// Insert an unblocker for the given context ID
    ///
    /// Returns the previous unblocker if one existed
    pub(crate) fn insert_unblocker(
        &self,
        target_context_id: u64,
        unblocker: Sender<Result<(), RuntimeError>>,
    ) -> Option<Sender<Result<(), RuntimeError>>> {
        self.unblockers
            .write()
            .unwrap()
            .insert(target_context_id, unblocker)
    }

    pub(crate) fn switch(
        &self,
        target_context_id: u64,
    ) -> Result<
        impl Future<Output = Result<Result<(), RuntimeError>, ContextCancelled>>
        + Send
        + Sync
        + use<>
        + 'static,
        ContextSwitchError,
    > {
        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

        // Lock contexts for this block
        let mut contexts = self.unblockers.write().unwrap();
        let own_context_id = self.active_context_id();

        // Assert preconditions (target is blocked && we are unblocked)
        if contexts.get(&target_context_id).is_none() {
            return Err(ContextSwitchError::SwitchTargetMissing);
        }
        if contexts.get(&own_context_id).is_some() {
            return Err(ContextSwitchError::OwnContextAlreadyBlocked);
        }

        // Unblock the target
        // Dont mark ourself as blocked yet, as we first need to know that unblocking succeeded
        let unblock_target = contexts.remove(&target_context_id).unwrap(); // Unwrap is safe due to precondition check above
        let unblock_result: std::result::Result<(), std::result::Result<(), RuntimeError>> =
            unblock_target.send(Ok(()));
        let Ok(_) = unblock_result else {
            // If there is no target to unblock, we assume it exited, but the unblock function was not removed
            // For now we treat this like a missing context
            // It can't happen again, as we already removed the unblock function
            //
            // TODO: Think about whether this is correct
            tracing::trace!(
                "Context {own_context_id} tried to switch to context {target_context_id} but it could not be unblocked (perhaps it exited?)"
            );
            return Err(ContextSwitchError::SwitchUnblockFailed);
        };

        // After we have unblocked the target, we can insert our own unblock function
        contexts.insert(own_context_id, own_unblocker);
        Ok(async move { wait_for_unblock.map_err(|_| ContextCancelled()).await })
    }

    /// Create a new context and spawn it onto the thread-local executor
    ///
    /// The entrypoint function is called when the context is unblocked for the first time
    ///
    /// If the context is cancelled before it is unblocked, the entrypoint will not be called
    pub(crate) fn new_context<T, F>(&self, entrypoint: T) -> u64
    where
        T: FnOnce(u64) -> F + 'static,
        F: Future<Output = ()> + 'static,
    {
        // Create a new context ID
        let new_context_id = self.allocate_new_context_id();

        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

        // Store the unblocker
        let None = self.insert_unblocker(new_context_id, own_unblocker) else {
            panic!("There already is a context suspended with ID {new_context_id}");
        };

        // Create the future for the new context
        let context_future = async move {
            // First wait for the unblock signal
            let prelaunch_result = wait_for_unblock.map_err(|_| ContextCancelled()).await;

            // Handle if the context was canceled before it even started
            match prelaunch_result {
                Ok(_) => (),
                Err(canceled) => {
                    tracing::trace!(
                        "Context {new_context_id} was canceled before it even started: {canceled}",
                    );
                    // At this point we don't need to do anything else
                    return;
                }
            };

            // Launch the context entrypoint
            entrypoint(new_context_id).await
        };

        // Queue the future onto the thread-local executor
        let spawn_result = self.spawner.spawn_local(context_future);

        match spawn_result {
            Ok(()) => new_context_id,
            Err(ThreadLocalSpawnerError::LocalPoolShutDown) => {
                // TODO: Handle cancellation properly
                panic!(
                    "Failed to spawn context {new_context_id} because the local executor has been shut down",
                );
            }
            Err(ThreadLocalSpawnerError::NotOnTheCorrectThread { expected, found }) => {
                // Not on the correct host thread. If this error happens, it is a bug in WASIX.
                panic!(
                    "Failed to spawn context {new_context_id} because the current thread ({found:?}) is not the expected thread ({expected:?}) for the local executor"
                )
            }
            Err(ThreadLocalSpawnerError::SpawnError) => {
                // This should never happen
                panic!("Failed to spawn_local context {new_context_id} , this should not happen");
            }
        }
    }
}
