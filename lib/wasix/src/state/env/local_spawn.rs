use crate::WasiFunctionEnv;
use futures::{
    executor::{LocalPool, LocalSpawner},
    task::LocalSpawnExt,
};
use std::{
    sync::{Arc, Mutex, Weak},
    thread::ThreadId,
};
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
pub struct ThreadLocalSpawner {
    /// A reference to the local executor's spawner
    pool: Weak<Mutex<Option<LocalSpawner>>>,
    /// The thread this spawner is associated with
    ///
    /// Used to generate better error messages when trying to spawn on the wrong thread
    thread: ThreadId,
}

// SAFETY: The ThreadLocalSpawner enforces using the spawner on the thread it was created on
// through runtime checks. See the safety comment in spawn_local.
unsafe impl Send for ThreadLocalSpawner {}

#[derive(Debug, Error)]
pub enum ThreadLocalSpawnerError {
    #[error(
        "Trying to spawn on a different thread than the one the ThreadLocalSpawner was created on. Expected: {expected:?}, found: {found:?}"
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
    pub fn spawn_local<F: Future<Output = ()> + 'static>(
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

        let spawner = self
            .pool
            .upgrade()
            .ok_or(ThreadLocalSpawnerError::LocalPoolShutDown)?;
        // Unwrap on the mutex, as it should never be poisoned
        let spawner = spawner.lock().unwrap();
        let spawner = spawner
            .as_ref()
            .ok_or(ThreadLocalSpawnerError::LocalPoolShutDown)?;

        spawner
            .spawn_local(future)
            .map_err(|_| ThreadLocalSpawnerError::SpawnError);
        Ok(())
    }
}

/// A thread-local executor that can run tasks on the current thread
pub struct ThreadLocalExecutor {
    pool: LocalPool,
    spawner: Arc<Mutex<Option<LocalSpawner>>>,
}

impl ThreadLocalExecutor {
    fn new() -> Self {
        let local_pool = futures::executor::LocalPool::new();
        let local_spawner = Arc::new(Mutex::new(Some(local_pool.spawner())));
        Self {
            pool: local_pool,
            spawner: local_spawner,
        }
    }

    fn get_spawner(&self) -> ThreadLocalSpawner {
        ThreadLocalSpawner {
            pool: Arc::downgrade(&self.spawner),
            thread: std::thread::current().id(),
        }
    }

    fn run_until<F: Future>(&mut self, future: F) -> F::Output {
        self.pool.run_until(future)
    }
}

impl Drop for ThreadLocalExecutor {
    fn drop(&mut self) {
        // Remove the spawner so no new tasks can be spawned
        //
        // This is technically not necessary, as the Weak upgrading in
        // LocalSpawner does a similar thing, but if the Weak was upgraded
        // before dropping the Executor this could lead to a SpawnError
        // instead of a LocalPoolShutDown. We can differentiate "real"
        // SpawnErrors from "dropped executor" errors this way
        let mut spawner = self.spawner.lock().unwrap();
        *spawner = None;
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
    let spawner = local_executor.get_spawner();
    let previous_spawner = env.current_spawner.replace(spawner);

    // Run function with the spawner
    let result = local_executor.run_until(entrypoint.call_async(&mut *store, &cloned_params));

    // Reset to previous spawner
    let env = ctx.data_mut(&mut store);
    env.current_spawner = previous_spawner;

    result
}
