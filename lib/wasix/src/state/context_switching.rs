use crate::{
    WasiError, WasiFunctionEnv,
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
    mem::forget,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use thiserror::Error;
use tracing::trace;
use wasmer::{RuntimeError, Store};
use wasmer_wasix_types::wasi::ExitCode;

/// The context-switching environment represents all state for WASIX context-switching
/// on a single host thread.
#[derive(Debug)]
pub(crate) struct ContextSwitchingEnvironment {
    inner: Arc<ContextSwitchingEnvironmentInner>,
}

#[derive(Debug)]
struct ContextSwitchingEnvironmentInner {
    /// List of the unblockers for all suspended contexts
    unblockers: RwLock<BTreeMap<u64, Sender<Result<(), RuntimeError>>>>,
    /// The ID of the currently active context
    current_context_id: AtomicU64,
    /// The next available context ID
    next_available_context_id: AtomicU64,
    /// This spawner can be used to spawn tasks onto the thread-local executor
    /// associated with this context-switching environment
    spawner: ThreadLocalSpawner,
}

/// Errors that can occur during a context switch
#[derive(Debug, Error)]
pub enum ContextSwitchError {
    #[error("Target context to switch to is missing")]
    SwitchTargetMissing,
}

const MAIN_CONTEXT_ID: u64 = 0;

/// Contexts will trap with this error as a RuntimeError::user when they are canceled
///
/// If encountered in a host function it MUST be propagated to the context's entrypoint.
/// To make it harder to run into that behaviour by ignoring this error, dropping it
/// will cause a panic with a message that it was not propagated properly. If you think
/// you know what you are doing, you can call `defuse` (or just forget it) to avoid
/// the panic.
///
/// When it bubbles up to the start of the entrypoint function of a context, it will be
/// handled by just letting the context exit silently.
#[derive(Error, Debug)]
#[error("Context was canceled")]
pub struct ContextCanceled(());
impl ContextCanceled {
    /// Defuse the ContextCanceled so it does not panic when dropped
    pub fn defuse(self) {
        // Consume self without panicking
        forget(self);
    }
}
impl Drop for ContextCanceled {
    fn drop(&mut self) {
        panic!(
            "A ContextCanceled error was dropped without being propagated to the context's entrypoint. This is likely a bug in a host function, please make sure to propagate ContextCanceled errors properly."
        );
    }
}

/// Contexts will trap with this error as a RuntimeError::user when they entrypoint returns
///
/// It is not allowed for context entrypoints to return normally, they must always
/// either get destroyed while suspended or trap with an error (like ContextCanceled)
///
/// This error will be picked up by the main context and cause it to trap as well.
#[derive(Error, Debug)]
#[error("The entrypoint of context {0} returned which is not allowed")]
pub struct ContextEntrypointReturned(u64);

impl ContextSwitchingEnvironment {
    fn new(spawner: ThreadLocalSpawner) -> Self {
        Self {
            inner: Arc::new(ContextSwitchingEnvironmentInner {
                unblockers: RwLock::new(BTreeMap::new()),
                current_context_id: AtomicU64::new(MAIN_CONTEXT_ID),
                next_available_context_id: AtomicU64::new(MAIN_CONTEXT_ID + 1),
                spawner,
            }),
        }
    }

    /// Run the main context function in a context-switching environment
    ///
    /// This call blocks until the entrypoint returns or traps
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
        let previous_environment = env.context_switching_environment.replace(this);
        if previous_environment.is_some() {
            panic!(
                "Failed to start a wasix main context as there was already a context-switching environment present."
            );
        }

        let store_async = store.into_async();
        // Run function with the spawner
        let result = local_executor.run_until(entrypoint.call_async(&store_async, params));

        // Process if this was terminated by a context entrypoint returning
        let result = match &result {
            Err(e) => match e.downcast_ref::<ContextEntrypointReturned>() {
                Some(ContextEntrypointReturned(id)) => {
                    // Context entrypoint returned, which is not allowed
                    // Exit with code 129
                    tracing::error!("The entrypoint of context {id} returned which is not allowed");
                    Err(RuntimeError::user(
                        WasiError::Exit(ExitCode::from(129)).into(),
                    ))
                }
                _ => result,
            },
            _ => result,
        };
        // Drop the executor to ensure all spawned tasks are dropped, so we have no references to the StoreAsync left
        drop(local_executor);

        // Remove the spawner again
        let mut store = store_async.into_store().ok().unwrap();

        let env = ctx.data_mut(&mut store);
        env.context_switching_environment.take().expect(
            "Failed to remove wasix context-switching environment from WASIX env after main context finished, this should never happen",
        );

        (store, result)
    }

    /// Get the ID of the currently active context
    pub(crate) fn active_context_id(&self) -> u64 {
        self.inner.current_context_id.load(Ordering::Relaxed)
    }

    /// Get the id of the main context (0)
    pub(crate) fn main_context_id(&self) -> u64 {
        MAIN_CONTEXT_ID
    }

    pub(crate) fn remove_unblocker(&self, target_context_id: &u64) -> bool {
        self.inner
            .unblockers
            .write()
            .unwrap()
            .remove(target_context_id)
            .is_some()
    }

    /// Unblock the target context and suspend own context
    ///
    /// If this function succeeds, you MUST await the returned future
    pub(crate) fn switch(
        &self,
        target_context_id: u64,
    ) -> Result<
        impl Future<Output = Result<(), RuntimeError>> + Send + Sync + use<> + 'static,
        ContextSwitchError,
    > {
        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();
        let wait_for_unblock = wait_for_unblock.map_err(|_| ContextCanceled(()));

        // Lock contexts for this block
        let mut unblockers = self.inner.unblockers.write().unwrap();
        let own_context_id = self.active_context_id();

        // Assert preconditions (target is blocked && we are unblocked)
        if unblockers.get(&target_context_id).is_none() {
            return Err(ContextSwitchError::SwitchTargetMissing);
        }
        if unblockers.get(&own_context_id).is_some() {
            // This should never happen, because if we are blocked, we should not be running code at all
            //
            // This is a bug in WASIX and should never happen, so we panic here.
            panic!("There is already a unblock present for the current context {own_context_id}");
        }

        // Unblock the target
        // Dont mark ourself as blocked yet, as we first need to know that unblocking succeeded
        let unblock_target = unblockers.remove(&target_context_id).unwrap(); // Unwrap is safe due to precondition check above
        let unblock_result: std::result::Result<(), std::result::Result<(), RuntimeError>> =
            unblock_target.send(Ok(()));
        let Ok(_) = unblock_result else {
            // If there is a unblock function in unblockers, the target context must be awaiting the related future.
            // One way we can get into this path is, when the target context was already resumed and we somehow managed to keep the unblocker around.
            // This can't happen as calling the unblocker consumes it.
            // Another way this could happen is if the future waiting for the unblocker was canceled before we called it.
            // This should not happen. This would be a bug in WASIX.
            // Another way this could happen is if the target context never awaited the unblocker future in the first place.
            // This also would be a bug in WASIX.
            //
            // So if we reach this path it is a bug in WASIX and should never happen, so we panic here.
            panic!(
                "Context {own_context_id} tried to unblock context {target_context_id} but the unblock target does not seem to exist."
            );
        };

        // After we have unblocked the target, we can insert our own unblock function
        unblockers.insert(own_context_id, own_unblocker);
        let weak_inner = Arc::downgrade(&self.inner);
        Ok(async move {
            let unblock_result = wait_for_unblock.await;

            // Handle if we were canceled instead of being unblocked
            let result = match unblock_result {
                Ok(v) => v,
                Err(canceled) => {
                    tracing::trace!("Context {own_context_id} was canceled while it was suspended");

                    // When our context was canceled return the `ContextCanceled` error.
                    // It will be handled by the entrypoint wrapper and the context will exit silently.
                    //
                    // If we reach this point, we must try to restore our context ID as it will not be read again
                    return Err(RuntimeError::user(canceled.into()));
                }
            };

            // Restore our own context ID
            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context-switching environment has been dropped, so we can't proceed
                //
                // This should only happen during shutdown when the ContextSwitchingEnvironment and thus the list of unblockers
                // is dropped and the futures continue being polled (because dropping that list would cause all wait_for_unblock
                // futures to resolve to canceled).
                // However looking at the implementation in `run_main_context` this should not happen, as we drop the executor
                // before dropping the environment,
                //
                // In a future implementation that allows the executor to outlive the environment, we should handle this case,
                // most likely by returning a `ContextCanceled` error here as well.
                // For now this should never happen, so it's a WASIX bug, so we panic here.
                panic!(
                    "The switch future for context {own_context_id} was polled after the context-switching environment was dropped, this should not happen"
                );
            };
            inner
                .current_context_id
                .store(own_context_id, Ordering::Relaxed);
            drop(inner);

            result
        })
    }

    /// Create a new context and spawn it onto the thread-local executor
    ///
    /// The entrypoint function is called when the context is unblocked for the first time
    ///
    /// If entrypoint returns, it must be a RuntimeError, as it is not allowed to return normally.
    /// If the RuntimeError is a [`ContextCanceled`], the context will just exit silently.
    /// Otherwise, the error will be propagated to the main context.
    ///
    /// If the context is cancelled before it is unblocked, the entrypoint will not be called
    pub(crate) fn create_context<F>(&self, entrypoint: F) -> u64
    where
        F: Future<Output = Result<(), RuntimeError>> + 'static,
    {
        // Create a new context ID
        let new_context_id = self
            .inner
            .next_available_context_id
            .fetch_add(1, Ordering::Relaxed);

        let (own_unblocker, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();
        let wait_for_unblock = wait_for_unblock.map_err(|_| ContextCanceled(()));

        // Store the unblocker

        let None = self
            .inner
            .unblockers
            .write()
            .unwrap()
            .insert(new_context_id, own_unblocker)
        else {
            panic!("There already is a context suspended with ID {new_context_id}");
        };

        // Create the future for the new context
        let weak_inner = Arc::downgrade(&self.inner);
        let context_future = async move {
            // First wait for the unblock signal
            let prelaunch_result = wait_for_unblock.await;

            // Handle if the context was canceled before it even started
            match prelaunch_result {
                Ok(_) => (),
                Err(canceled) => {
                    trace!("Context {new_context_id} was successfully destroyed before it started");
                    // We know what we are doing, so we can prevent the panic on drop
                    canceled.defuse();
                    // Context was cancelled before it was started, so we can just let it return.
                    // This will resolve the original future passed to `spawn_local` with
                    // `Ok(())` which should make the executor drop it properly
                    return;
                }
            };

            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context-switching environment has been dropped, so we can't proceed.
                // See the comments on the first Weak::upgrade call in this file for background on when this can happen.
                //
                // Note that in case the context was canceled properly, we accept that and allowed it to exit
                // silently (in the match block above). That could happen if the main context canceled the
                // this context before exiting itself and the executor outlives the environment.
                //
                // However it should not be possible to switch to this context after the main context has exited,
                // as there can only be one active context at a time and that one (the main context) just exited.
                // So there can't be another context in that context-switching environment that could switch to this one.
                panic!(
                    "Context {new_context_id} was switched to after the context-switching environment was dropped. This indicates a bug where multiple contexts are active at the same time which should never happen"
                );
            };
            // Set the current context ID
            inner
                .current_context_id
                .store(new_context_id, Ordering::Relaxed);
            // Drop inner again so we don't hold a strong ref while running the entrypoint, so it cleans itself up properly
            drop(inner);

            // Launch the context entrypoint
            let entrypoint_result = entrypoint.await;

            // If that function returns, we need to resume the main context with an error
            // Take the underlying error, or create a new error if the context returned a value
            let entrypoint_result = entrypoint_result.map_or_else(
                |e| e,
                |_| RuntimeError::user(ContextEntrypointReturned(new_context_id).into()),
            );

            // If that function returns something went wrong.
            // If it's a cancellation, we can just let this context run out.
            // If it's another error, we resume the main context with the error
            let error = match entrypoint_result.downcast::<ContextCanceled>() {
                Ok(canceled) => {
                    tracing::trace!("Context {new_context_id} was successfully destroyed");
                    // We know what we are doing, so we can prevent the panic on drop
                    canceled.defuse();
                    // Context was cancelled, so we can just let it return.
                    // This will resolve the original future passed to `spawn_local` with
                    // `Ok(())` which should make the executor drop it properly
                    return;
                }
                Err(error) => error, // Propagate the runtime error to main
            };

            // Retrieve the main context
            let Some(inner) = Weak::upgrade(&weak_inner) else {
                // The context-switching environment has been dropped, so we can't proceed.
                // See the comments on the first Weak::upgrade call in this file for background on when this can happen.
                //
                // Note that in case the context was canceled properly, we accept that and allowed it to exit
                // silently (in the match block above). That could happen if the main context canceled the
                // this context before exiting itself and the executor outlives the environment.
                //
                // However it should not be possible to switch to this context after the main context has exited,
                // as there can only be one active context at a time and that one (the main context) just exited.
                // So there can't be another context in that context-switching environment that could switch to this one.
                //
                // So in conclusion if we reach this point it is a bug in WASIX and should never happen, so we panic here.
                panic!(
                    "Context {new_context_id} was switched to after the context-switching environment was dropped. This indicates a bug where multiple contexts are active at the same time which should never happen"
                );
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
                // This case could happen if the executor is being shut down while it is still polling a future (this one).
                // Which shouldn't be able with a single-threaded executor, as the shutdown would have to
                // be initiated from within a future running on that executor.
                // I the current WASIX context switching implemenation should not be able to produce this case,
                // but maybe it will be possible in future implementations. If someone manages to produce this case,
                // they should open an issue so we can discuss how to handle this case properly.
                // If this case is reachable we could return the same error as when no context-switching environment is present,
                panic!(
                    "Failed to spawn context {new_context_id} because the local executor has been shut down. Please open an issue and let me know how you produced this error.",
                );
            }
            Err(ThreadLocalSpawnerError::NotOnTheCorrectThread { expected, found }) => {
                // This should never happen and is a bug in WASIX, so we panic here
                panic!(
                    "Failed to create context because the thread local spawner lives on {expected:?} but you are on {found:?}"
                )
            }
            Err(ThreadLocalSpawnerError::SpawnError) => {
                panic!("Failed to spawn_local context {new_context_id}, this should not happen");
            }
        }
    }
}
