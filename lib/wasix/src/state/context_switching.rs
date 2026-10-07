use crate::{
    WasiError, WasiFunctionEnv,
    runtime::task_manager::{LocalTaskSpawnError, LocalTaskSpawner},
};
use futures::{
    TryFutureExt,
    channel::oneshot::{self, Sender},
    future::{AbortHandle, Abortable},
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
    // TODO: We might be able to get rid of this Arc by passing an AsyncFunctionEnvMut to the functions instead
    //       We need to be super-careful about memory leaks by cyclical references if we do that change.
    //       The Arc allows us to have weak references to the ContextSwitchingEnvironment indepenend of the WasiEnv.
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
    /// Spawns contexts on the worker-local executor supplied by the task manager.
    spawner: LocalTaskSpawner,
    /// Every live context, so teardown can cancel them.
    ///
    /// Dropping a context's unblocker is not enough to finish it: an entrypoint
    /// task can be parked on a JSPI promise that will never settle, and such a
    /// future only goes away if something aborts it. Until every one is gone
    /// their store clones are outstanding, and the async store cannot be turned
    /// back into an owned `Store`.
    context_tasks: RwLock<BTreeMap<u64, ContextTask>>,
}

#[derive(Debug)]
struct ContextTask {
    abort: AbortHandle,
    completed: oneshot::Receiver<()>,
}

impl ContextSwitchingEnvironmentInner {
    /// Cancels every context and waits for it to release its store clones.
    async fn shutdown(&self) {
        let tasks = std::mem::take(&mut *self.context_tasks.write().unwrap());
        for task in tasks.values() {
            task.abort.abort();
        }
        for task in tasks.into_values() {
            let _ = task.completed.await;
        }
    }
}

impl Drop for ContextSwitchingEnvironmentInner {
    fn drop(&mut self) {
        for task in self.context_tasks.get_mut().unwrap().values() {
            task.abort.abort();
        }
    }
}

/// Removes a context from the environment and reports its completion, whichever
/// way its task ended.
struct ContextTaskRegistration {
    environment: Weak<ContextSwitchingEnvironmentInner>,
    id: u64,
    completed: Option<Sender<()>>,
}

impl Drop for ContextTaskRegistration {
    fn drop(&mut self) {
        if let Some(environment) = self.environment.upgrade() {
            environment.context_tasks.write().unwrap().remove(&self.id);
        }
        if let Some(completed) = self.completed.take() {
            let _ = completed.send(());
        }
    }
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
    fn new(spawner: LocalTaskSpawner) -> Self {
        Self {
            inner: Arc::new(ContextSwitchingEnvironmentInner {
                unblockers: RwLock::new(BTreeMap::new()),
                current_context_id: AtomicU64::new(MAIN_CONTEXT_ID),
                next_available_context_id: AtomicU64::new(MAIN_CONTEXT_ID + 1),
                spawner,
                context_tasks: RwLock::new(BTreeMap::new()),
            }),
        }
    }

    /// Run the main context function in a context-switching environment
    ///
    /// This call yields until the entrypoint returns or traps.
    ///
    /// Every guest entry goes through here: a process's start function, and
    /// the entry functions of the threads it spawns and the forks it makes. So
    /// this is where [`Capabilities::enable_async_entrypoint`] and
    /// [`Capabilities::enable_context_switching`] take effect, for all three:
    /// the guest is entered synchronously, asynchronously, or asynchronously
    /// with a context-switching environment to join.
    ///
    /// [`Capabilities::enable_async_entrypoint`]: crate::capabilities::Capabilities::enable_async_entrypoint
    /// [`Capabilities::enable_context_switching`]: crate::capabilities::Capabilities::enable_context_switching
    pub(crate) async fn run_main_context(
        ctx: &WasiFunctionEnv,
        mut store: Store,
        entrypoint: wasmer::Function,
        params: Vec<wasmer::Value>,
        local_tasks: LocalTaskSpawner,
    ) -> (Store, Result<Box<[wasmer::Value]>, RuntimeError>) {
        // If we are already in a context-switching environment, something went wrong
        if ctx
            .data_mut(&mut store)
            .context_switching_environment
            .is_some()
        {
            panic!(
                "Failed to start a WASIX main context as there was already a context-switching environment present."
            );
        }

        // JSPI and Asyncify are alternative suspension mechanisms *on the JS
        // backend*, where a guest instrumented to unwind its own stack uses
        // Asyncify and must not be entered through JSPI as well. Native async
        // calls run Asyncify guests perfectly well, fork and vfork included, so
        // this exclusion is deliberately not applied there.
        //
        // The predicate is `will_use_asyncify`, which answers for *statically*
        // linked modules only. Asking the main module of a dynamically linked
        // tree instead would claim Asyncify for guests that cannot run it — every
        // WASIX build imports `stack_checkpoint` whether or not it is
        // instrumented — and so would cost them the JSPI entry they do need.
        let supports_async = store.engine().supports_async();
        #[cfg(feature = "js")]
        let supports_async =
            supports_async && !(store.engine().is_js() && ctx.data(&store).will_use_asyncify());
        let capabilities = &ctx.data(&store).capabilities;
        let enter_async = supports_async && capabilities.enable_async_entrypoint;
        let context_switching = capabilities.context_switching_enabled();

        // Entered synchronously: nothing in the guest can suspend, and the
        // context-switching API is unavailable whatever its own setting says.
        if !enter_async {
            let result = entrypoint.call(&mut store, &params);
            return (store, result);
        }

        // Entered asynchronously, but without an environment to join: imports
        // that suspend at the top of the guest's stack work, as N-API's event
        // loop checkpoint does on JS, while the context-switching syscalls stay
        // unavailable.
        if !context_switching {
            let store_async = store.into_async();
            let result = entrypoint.call_async(&store_async, params).await;
            let store = reclaim_store(store_async).await;
            return (store, result);
        }

        let this = Self::new(local_tasks);
        let context_tasks = Arc::downgrade(&this.inner);

        // Add the context-switching environment to the WasiEnv
        let previous = ctx
            .data_mut(&mut store)
            .context_switching_environment
            .replace(this);
        assert!(previous.is_none()); // Should never be hit because of the check at the top

        // Turn the store into an async store and run the entrypoint
        let store_async = store.into_async();
        let result = entrypoint.call_async(&store_async, params).await;

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
        tracing::trace!("Main context finished execution and returned {result:?}");

        // Cancel every remaining context and wait for it to let go of the store.
        // An entrypoint task can be parked on a JSPI promise that will never
        // settle, so dropping its unblocker does not finish it; only aborting
        // does, and until all of them are gone their store clones keep the async
        // store shared.
        if let Some(environment) = context_tasks.upgrade() {
            environment.shutdown().await;
        }
        let mut store = reclaim_store(store_async).await;

        // Remove the context-switching environment from the WasiEnv
        let env = ctx.data_mut(&mut store);

        env.context_switching_environment
            .take()
            .or_else(|| {
                env.vfork
                    .as_mut()
                    .and_then(|vfork| vfork.env.context_switching_environment.take())
                    .inspect(|_| {
                        // Grace for vforks, so they don't bring everything down with them.
                        // This is still an error.
                        // The message below is oversimplified there is more nuance to this.
                        tracing::error!("Exiting a vforked process in any other way than calling `_exit()` is undefined behavior but the current program just did that.");
                    })
            })
            .expect("Failed to remove wasix context-switching environment from WASIX env after main context finished. This means we lost it somehow which should never happen.");

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

    pub(crate) fn destroy_context(&self, target_context_id: &u64) -> bool {
        // For now this only queues the context up for destruction by removing its unblocker.
        // This will only cause destruction when the context_switch is called.
        // That could cause memory issues if many contexts are created and destroyed without switching
        // which could happen in applications that use contexts during setup, but not during main execution.
        // We don't do immediate destruction because that would make this more complex, as it is essentially
        // identical to switching to the target context
        // TODO: Implement immediate destruction if the above becomes an issue
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
    pub(crate) fn switch_context(
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

        // Assert that we are unblocked
        if unblockers.get(&own_context_id).is_some() {
            // This should never happen, because if we are blocked, we should not be running code at all
            //
            // This is a bug in WASIX and should never happen, so we panic here.
            panic!("There is already a unblock present for the current context {own_context_id}");
        }

        // Assert that the target is blocked
        let Some(unblock_target) = unblockers.remove(&target_context_id) else {
            return Err(ContextSwitchError::SwitchTargetMissing);
        };

        // Unblock the target
        // Dont mark ourself as blocked yet, as we first need to know that unblocking succeeded
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
                    tracing::trace!("Canceled context {own_context_id} while it was suspended");

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
                    "Resumed context {new_context_id} after the context-switching environment was dropped. This indicates a bug where multiple contexts are active at the same time which should never happen"
                );
            };
            // Set the current context ID
            inner
                .current_context_id
                .store(new_context_id, Ordering::Relaxed);
            // Drop inner again so we don't hold a strong ref while running the entrypoint, so it cleans itself up properly
            drop(inner);

            tracing::trace!("Resumed context {new_context_id} for the first time");

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
                    tracing::trace!(
                        "Destroyed context {new_context_id} successfully after it was canceled"
                    );
                    // We know what we are doing, so we can prevent the panic on drop
                    canceled.defuse();
                    // Context was cancelled, so we can just let it return.
                    // This will resolve the original future passed to `spawn_local` with
                    // `Ok(())` which should make the executor drop it properly
                    return;
                }
                Err(error) => error, // Propagate the runtime error to main
            };

            tracing::trace!("Context {new_context_id} entrypoint returned with {error:?}");

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
                    "Context {new_context_id} entrypoint returned after the context-switching environment was dropped. This indicates a bug where multiple contexts are active at the same time which should never happen"
                );
            };

            tracing::trace!(
                "Resuming main context {MAIN_CONTEXT_ID} with error from context {new_context_id}"
            );
            let Some(main_context) = inner.unblockers.write().unwrap().remove(&MAIN_CONTEXT_ID)
            else {
                // The main context should always be suspended when another context returns or traps with anything but cancellation
                panic!(
                    "The main context should always be suspended when another context returns or traps (with anything but a cancellation)."
                );
            };
            drop(inner);

            // Resume the main context with the error
            main_context
                .send(Err(error))
                .expect("Failed to send error to main context, this should not happen");
        };

        // Make the task abortable and register it, so teardown can cancel a
        // context parked on a promise that will never settle and then wait for it
        // to release its store clones.
        let (abort, abort_registration) = AbortHandle::new_pair();
        let (completed, wait_for_completion) = oneshot::channel();
        let registration = ContextTaskRegistration {
            environment: Arc::downgrade(&self.inner),
            id: new_context_id,
            completed: Some(completed),
        };
        let context_future = async move {
            let _registration = registration;
            let _ = Abortable::new(context_future, abort_registration).await;
        };
        self.inner.context_tasks.write().unwrap().insert(
            new_context_id,
            ContextTask {
                abort,
                completed: wait_for_completion,
            },
        );

        // Queue the future onto the worker-local executor
        tracing::trace!("Spawning context {new_context_id} onto the worker-local executor");
        let spawn_result = self.inner.spawner.spawn(context_future);

        match spawn_result {
            Ok(()) => new_context_id,
            Err(LocalTaskSpawnError::ShutDown) => {
                // This case could happen if the executor is being shut down while it is still polling a future (this one).
                // Which shouldn't be able with a single-threaded executor, as the shutdown would have to
                // be initiated from within a future running on that executor.
                // I the current WASIX context switching implementation should not be able to produce this case,
                // but maybe it will be possible in future implementations. If someone manages to produce this case,
                // they should open an issue so we can discuss how to handle this case properly.
                // If this case is reachable we could return the same error as when no context-switching environment is present,
                panic!(
                    "Failed to spawn context {new_context_id} because the local executor has been shut down. Please open an issue and let me know how you produced this error.",
                );
            }
            Err(LocalTaskSpawnError::WrongThread { expected, found }) => {
                // This should never happen and is a bug in WASIX, so we panic here
                panic!(
                    "Failed to create context because the worker-local spawner lives on {expected:?} but you are on {found:?}"
                )
            }
            Err(LocalTaskSpawnError::Spawn) => {
                panic!("Failed to spawn context {new_context_id}, this should not happen");
            }
        }
    }
}

/// Turns the guest's async store back into a [`Store`] once nothing else holds
/// it.
///
/// On the JS backend a call that is cancelled just as one of its imports
/// completes leaves its guest to resume once more, in a JavaScript job that
/// cannot be cancelled; it keeps the store until that guest next suspends or
/// finishes, which it does as soon as the job runs. Yielding lets that job run
/// first. Anything still holding the store after that is a leak, and as before
/// it is fatal.
async fn reclaim_store(mut store_async: wasmer::StoreAsync) -> Store {
    const ATTEMPTS: usize = 64;
    for _ in 0..ATTEMPTS {
        match store_async.into_store() {
            Ok(store) => return store,
            Err(still_shared) => store_async = still_shared,
        }
        yield_once().await;
    }
    panic!(
        "the guest's store is still in use after its entrypoint returned and every \
         context was shut down"
    );
}

/// Yields to the executor once, waking itself so it is polled again.
fn yield_once() -> impl Future<Output = ()> {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            return std::task::Poll::Ready(());
        }
        yielded = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    })
}
