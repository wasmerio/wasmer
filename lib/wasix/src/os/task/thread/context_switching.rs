use crate::{
    WasiFunctionEnv,
    utils::thread_local_executor::{
        ThreadLocalExecutor, ThreadLocalSpawner, ThreadLocalSpawnerError,
    },
};
use futures::{
    TryFutureExt,
    channel::oneshot::{self, Sender},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use thiserror::Error;
use wasmer::{RuntimeError, Store};

#[derive(Debug)]
pub(crate) struct ContextSwitchingContext {
    /// TODO: Document these fields
    inner: Arc<ContextSwitchingContextInner>,
}

#[derive(Debug)]
struct ContextSwitchingContextInner {
    /// List of the unblockers for all suspended contexts
    unblockers: RwLock<BTreeMap<u64, Sender<Result<(), RuntimeError>>>>,
    /// The ID of the currently active context
    current_context_id: AtomicU64,
    /// The next available context ID
    next_available_context_id: AtomicU64,
    /// This spawner can be used to spawn tasks onto the thread-local executor
    /// associated with this context switching environment
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

const MAIN_CONTEXT_ID: u64 = 0;

#[derive(Error, Debug)]
#[error("Context was canceled")]
pub struct ContextCanceled();

impl ContextSwitchingContext {
    fn new(spawner: ThreadLocalSpawner) -> Self {
        Self {
            inner: Arc::new(ContextSwitchingContextInner {
                unblockers: RwLock::new(BTreeMap::new()),
                current_context_id: AtomicU64::new(MAIN_CONTEXT_ID),
                next_available_context_id: AtomicU64::new(MAIN_CONTEXT_ID + 1),
                spawner,
            }),
        }
    }

    /// Run the main context function in a context switching context
    ///
    /// This call blocks until the entrypoint returns, or it or any of the contexts it spawns traps
    pub(crate) fn run_main_context(
        ctx: &WasiFunctionEnv,
        mut store: Store,
        entrypoint: wasmer::Function,
        params: Vec<wasmer::Value>,
    ) -> (Store, Result<Box<[wasmer::Value]>, RuntimeError>) {
        // Create a new executor
        let mut local_executor = ThreadLocalExecutor::new();

        let this = Self::new(local_executor.spawner());

        // Put the spawner into the WASI env, so that syscalls can use it to queue up new tasks
        let env = ctx.data_mut(&mut store);
        let previous_context = env.context_switching_context.replace(this);
        if previous_context.is_some() {
            panic!(
                "Failed to start a wasix main context as there was already a context switching context present in the WASI env."
            );
        }

        let store_async = store.into_async();
        // Run function with the spawner
        let result = local_executor.run_until(entrypoint.call_async(&store_async, params));
        // Drop the executor to ensure all spawned tasks are dropped, so we have no references to the StoreAsync left
        drop(local_executor);

        // Remove the spawner again
        let mut store = store_async.into_store().ok().unwrap();

        let env = ctx.data_mut(&mut store);
        env.context_switching_context.take().expect(
            "Failed to remove wasix context switching context from WASI env after main context finished, this should never happen",
        );

        (store, result)
    }

    /// Get the ID of the currently active context
    pub(crate) fn active_context_id(&self) -> u64 {
        self.inner
            .current_context_id
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the id of the main context (0)
    pub(crate) fn main_context_id(&self) -> u64 {
        MAIN_CONTEXT_ID
    }

    pub(crate) fn remove_unblocker(
        &self,
        target_context_id: &u64,
    ) -> Option<Sender<Result<(), RuntimeError>>> {
        self.inner
            .unblockers
            .write()
            .unwrap()
            .remove(target_context_id)
    }

    /// Insert an unblocker for the given context ID
    ///
    /// Returns the previous unblocker if one existed
    pub(crate) fn insert_unblocker(
        &self,
        target_context_id: u64,
        unblocker: Sender<Result<(), RuntimeError>>,
    ) -> Option<Sender<Result<(), RuntimeError>>> {
        self.inner
            .unblockers
            .write()
            .unwrap()
            .insert(target_context_id, unblocker)
    }

    /// Unblock the target context and suspend own context
    ///
    /// If this function succeeds, you MUST await the returned future
    #[must_use]
    pub(crate) fn switch(
        &self,
        target_context_id: u64,
    ) -> Result<
        impl Future<Output = Result<(), RuntimeError>> + Send + Sync + use<> + 'static,
        ContextSwitchError,
    > {
        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

        // Lock contexts for this block
        let mut contexts = self.inner.unblockers.write().unwrap();
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
        let weak_inner = Arc::downgrade(&self.inner);
        Ok(async move {
            let unblock_result = wait_for_unblock.map_err(|_| ContextCanceled()).await;

            // Restore our own context ID
            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context switching context has been dropped, so we can't proceed
                // TODO: Handle this properly
                todo!();
            };
            inner
                .current_context_id
                .store(own_context_id, Ordering::Relaxed);
            drop(inner);

            // Handle if we were canceled instead of being unblocked
            match unblock_result {
                Ok(v) => v,
                Err(canceled) => {
                    tracing::trace!(
                        "Context {own_context_id} was canceled while it was suspended: {}",
                        canceled
                    );

                    let err = ContextCanceled().into();
                    return Err(RuntimeError::user(err));
                }
            }
        })
    }

    /// Create a new context and spawn it onto the thread-local executor
    ///
    /// The entrypoint function is called when the context is unblocked for the first time
    ///
    /// If the context is cancelled before it is unblocked, the entrypoint will not be called
    pub(crate) fn new_context<T, F>(&self, entrypoint: T) -> u64
    where
        T: FnOnce(u64) -> F + 'static,
        F: Future<Output = RuntimeError> + 'static,
    {
        // Create a new context ID
        let new_context_id = self
            .inner
            .next_available_context_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

        // Store the unblocker
        let None = self.insert_unblocker(new_context_id, own_unblocker) else {
            panic!("There already is a context suspended with ID {new_context_id}");
        };

        // Create the future for the new context
        let weak_inner = Arc::downgrade(&self.inner);
        let context_future = async move {
            // First wait for the unblock signal
            let prelaunch_result = wait_for_unblock.map_err(|_| ContextCanceled()).await;

            // Set the current context ID
            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context switching context has been dropped, so we can't proceed
                // TODO: Handle this properly
                return;
            };
            inner
                .current_context_id
                .store(new_context_id, Ordering::Relaxed);
            drop(inner);

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
            let launch_result = entrypoint(new_context_id).await;

            // If that function returns something went wrong.
            // If it's a cancellation, we can just let this context run out.
            // If it's another error, we resume the main context with the error
            let error = match launch_result.downcast_ref::<ContextCanceled>() {
                Some(err) => {
                    tracing::trace!("Context {new_context_id} exited with error: {}", err);
                    // Context was cancelled, so we can just let it run out.
                    return;
                }
                None => launch_result, // Propagate the runtime error to main
            };

            // Retrieve the main context
            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context switching context has been dropped, so we can't proceed
                // TODO: Handle this properly
                return;
            };
            let Some(main_context) = inner.unblockers.write().unwrap().remove(&MAIN_CONTEXT_ID)
            else {
                // The main context should always be suspended when another context returns or traps with anything but cancellation
                panic!(
                    "The main context should always be suspended when another context returns or traps (with anything but a cancellation)."
                );
            };
            // Resume the main context with the error
            main_context
                .send(Err(error))
                .expect("Failed to send error to main context, this should not happen");
            drop(inner);
        };

        // Queue the future onto the thread-local executor
        let spawn_result = self.inner.spawner.spawn_local(context_future);

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
