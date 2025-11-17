use dashmap::DashMap;
use futures::task::LocalSpawnExt;
use std::sync::atomic::AtomicU64;
use wasmer::{RuntimeError, Store, Value};

use crate::{WasiFunctionEnv, state::env::context::Context};

thread_local! {
    static NEXT_SPAWNER_ID: AtomicU64 = AtomicU64::new(0);
    static LOCAL_SPAWNERS: DashMap<u64, futures::executor::LocalSpawner> = DashMap::new();
}

// Send, just a handle
#[derive(Clone, Debug)]
pub struct ThreadLocalSpawner {
    id: u64,
}

// Not send
pub struct ThreadLocalExecutor {
    id: u64,
    pool: futures::executor::LocalPool,
}

impl ThreadLocalSpawner {
    // Spawn a future onto the same thread as the local spawner
    pub fn spawn_local<F: Future<Output = ()> + 'static>(&self, future: F) {
        LOCAL_SPAWNERS.with(|runtimes| {
            let spawner = runtimes
                .get(&self.id)
                .expect("Failed to find local spawner. Maybe you are on the wrong thread?");
            spawner
                .spawn_local(future)
                .expect("Failed to spawn local future");
        });
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
            ThreadLocalSpawner { id: runtime_id },
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

pub fn call_in_async_runtime<'a>(
    ctx: &WasiFunctionEnv,
    mut store: &mut Store,
    entrypoint: wasmer::Function,
    params: &'a [wasmer::Value],
) -> Result<Box<[Value]>, RuntimeError> {
    let cloned_params = params.to_vec();
    let main_context = Context::new();
    let env = ctx.data_mut(&mut store);
    env.contexts.write().unwrap().insert(0, main_context);

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
