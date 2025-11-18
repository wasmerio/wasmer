use crate::WasiFunctionEnv;
use futures::{
    executor::{LocalPool, LocalSpawner},
    task::LocalSpawnExt,
};
use std::thread::ThreadId;
use thiserror::Error;
use wasmer::{RuntimeError, Store, Value};

/// A `Send`able spawner that spawns onto a thread-local executor
///
/// Despite being `Send`, the spawner enforces at runtime that
/// it is only used to spawn on the thread it was created on.
//
// If that limitation is a problem, we can consider implementing a version that
// accepts `Send` futures and sends them to the correct thread via channels.
#[derive(Clone, Debug)]
pub(crate) struct ThreadLocalSpawner {
    /// A reference to the local executor's spawner
    spawner: LocalSpawner,
    /// The thread this spawner is associated with
    ///
    /// Used to generate better error messages when trying to spawn on the wrong thread
    thread: ThreadId,
}
// SAFETY: The ThreadLocalSpawner enforces the spawner is only used on the correct thread.
// See the safety comment in ThreadLocalSpawner::spawn_local and ThreadLocalSpawner::spawner
unsafe impl Send for ThreadLocalSpawner {}
// SAFETY: The ThreadLocalSpawner enforces the spawner is only used on the correct thread.
// See the safety comment in ThreadLocalSpawner::spawn_local and ThreadLocalSpawner::spawner
unsafe impl Sync for ThreadLocalSpawner {}

/// Errors that can occur during `spawn_local` calls
#[derive(Debug, Error)]
pub enum ThreadLocalSpawnerError {
    #[error(
        "The ThreadLocalSpawner can only spawn tasks on the thread it was created on. Expected to be on {expected:?} but was actually on {found:?}"
    )]
    NotOnTheCorrectThread { expected: ThreadId, found: ThreadId },
    #[error(
        "The local executor associated with this spawner has been shut down and cannot accept new tasks"
    )]
    LocalPoolShutDown,
    #[error("An error occurred while spawning the task")]
    SpawnError,
}

impl ThreadLocalSpawner {
    /// Spawn a future onto the same thread as the local spawner
    ///
    /// Needs to be called from the same thread on which the associated executor was created
    pub(crate) fn spawn_local<F: Future<Output = ()> + 'static>(
        &self,
        future: F,
    ) -> Result<(), ThreadLocalSpawnerError> {
        // SAFETY: This is what makes implementing Send on ThreadLocalSpawner safe. We ensure that we only spawn
        // on the same thread as the one the spawner was created on.
        if std::thread::current().id() != self.thread {
            return Err(ThreadLocalSpawnerError::NotOnTheCorrectThread {
                expected: self.thread,
                found: std::thread::current().id(),
            });
        }

        // As we now know that we are on the correct thread, we can use the spawner safely
        self.spawner
            .spawn_local(future)
            .map_err(|e| match e.is_shutdown() {
                true => ThreadLocalSpawnerError::LocalPoolShutDown,
                false => ThreadLocalSpawnerError::SpawnError,
            })
    }
}

/// A thread-local executor that can run tasks on the current thread
pub(crate) struct ThreadLocalExecutor {
    /// The local pool
    pool: LocalPool,
}

impl ThreadLocalExecutor {
    pub(crate) fn new() -> Self {
        let local_pool = futures::executor::LocalPool::new();
        Self { pool: local_pool }
    }

    pub(crate) fn spawner(&self) -> ThreadLocalSpawner {
        ThreadLocalSpawner {
            spawner: self.pool.spawner(),
            // SAFETY: This will always be the thread where the spawner was created on, as the ThreadLocalExecutor is not Send
            thread: std::thread::current().id(),
        }
    }

    pub(crate) fn run_until<F: Future>(&mut self, future: F) -> F::Output {
        self.pool.run_until(future)
    }
}

// TODO: This does not belong here
pub fn call_in_async_runtime<'a>(
    ctx: &WasiFunctionEnv,
    mut store: &mut Store,
    entrypoint: wasmer::Function,
    params: &'a [wasmer::Value],
) -> Result<Box<[Value]>, RuntimeError> {
    let cloned_params = params.to_vec();
    let env = ctx.data_mut(&mut store);
    // TODO: Ensure there is only one executor at a time?

    // Set spawner in env
    let mut local_executor = ThreadLocalExecutor::new();
    let spawner = local_executor.spawner();
    let previous_spawner = env.current_spawner.replace(spawner);

    // Run function with the spawner
    let result = local_executor.run_until(entrypoint.call_async(&mut *store, &cloned_params));

    // Reset to previous spawner
    let env = ctx.data_mut(&mut store);
    env.current_spawner = previous_spawner;

    result
}
