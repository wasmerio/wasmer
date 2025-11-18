use dashmap::DashMap;
use futures::task::LocalSpawnExt;
use std::{sync::atomic::AtomicU64, thread::ThreadId};
use thiserror::Error;
use wasmer::{RuntimeError, Store, Value};

use crate::WasiFunctionEnv;

thread_local! {
    static NEXT_SPAWNER_ID: AtomicU64 = AtomicU64::new(0);
    static LOCAL_SPAWNERS: DashMap<u64, futures::executor::LocalSpawner> = DashMap::new();
}

// Send, just a handle
#[derive(Clone, Debug)]
pub struct ThreadLocalSpawner {
    id: u64,
    /// The thread this spawner is associated with
    ///
    /// Used to generate better error messages when trying to spawn on the wrong thread
    thread: ThreadId,
}

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

// Not send
pub struct ThreadLocalExecutor {
    id: u64,
    pool: futures::executor::LocalPool,
}

impl ThreadLocalSpawner {
    /// Spawn a future onto the same thread as the local spawner
    ///
    /// Needs to be called from the same thread as the spawner was created on
    pub fn spawn_local<F: Future<Output = ()> + 'static>(
        &self,
        future: F,
    ) -> Result<(), ThreadLocalSpawnerError> {
        if std::thread::current().id() != self.thread {
            return Err(ThreadLocalSpawnerError::NotOnTheCorrectThread {
                expected: self.thread,
                found: std::thread::current().id(),
            });
        }
        LOCAL_SPAWNERS.with(|runtimes| {
            let spawner = runtimes
                .get(&self.id)
                .ok_or(ThreadLocalSpawnerError::LocalPoolShutDown)?;
            spawner
                .spawn_local(future)
                .map_err(|_| ThreadLocalSpawnerError::SpawnError)?;
        });
        Ok(())
    }
}

impl ThreadLocalExecutor {
    fn new() -> (ThreadLocalSpawner, Self) {
        let localpool = futures::executor::LocalPool::new();
        let local_spawner = localpool.spawner();
        let runtime_id =
            NEXT_SPAWNER_ID.with(|id| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        LOCAL_SPAWNERS.with(|runtimes| {
            runtimes.insert(runtime_id, local_spawner);
        });
        (
            ThreadLocalSpawner {
                id: runtime_id,
                thread: std::thread::current().id(),
            },
            Self {
                id: runtime_id,
                pool: localpool,
            },
        )
    }

    fn run_until<F: Future>(&mut self, future: F) -> F::Output {
        self.pool.run_until(future)
    }
}

impl Drop for ThreadLocalExecutor {
    fn drop(&mut self) {
        LOCAL_SPAWNERS.with(|runtimes| {
            runtimes.remove(&self.id);
        });
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
    let (spawner, mut local_executor) = ThreadLocalExecutor::new();
    let previous_spawner = env.current_spawner.replace(spawner);

    // Run function with the spawner
    let result = local_executor.run_until(entrypoint.call_async(&mut *store, &cloned_params));

    // Reset to previous spawner
    let env = ctx.data_mut(&mut store);
    env.current_spawner = previous_spawner;

    result
}
