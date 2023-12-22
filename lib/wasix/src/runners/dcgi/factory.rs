use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};

use virtual_fs::Pipe;
use wasmer_wasix_types::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};

use crate::{
    runners::wcgi::{CreateEnvConfig, CreateEnvResult, RecycleEnvConfig},
    state::conv_env_vars,
    WasiStateCreationError,
};

use super::*;

#[derive(Debug)]
struct StateShard {
    instances: VecDeque<DcgiInstance>,
    last_acquire: Instant,
    master_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Default for StateShard {
    fn default() -> Self {
        Self {
            instances: Default::default(),
            last_acquire: Instant::now(),
            master_lock: Arc::new(Default::default()),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    // List of the shards and a queue of DCGI instances that
    // are running for these shards
    shards: HashMap<String, StateShard>,
}

/// This factory will store and reuse instances between invocations thus
/// allowing for the instances to be stateful.
#[derive(Debug, Clone, Default)]
pub struct DcgiInstanceFactory {
    state: Arc<Mutex<State>>,
}

impl DcgiInstanceFactory {
    pub fn new() -> Self {
        Self {
            state: Default::default(),
        }
    }

    pub async fn release(&self, mut conf: RecycleEnvConfig<DcgiMetadata>) {
        let shard = conf.meta.shard;

        let mut state = self.state.lock().unwrap();
        state
            .shards
            .entry(shard)
            .or_default()
            .instances
            .push_front(DcgiInstance {
                env: conf.env,
                memory: conf.memory,
                store: conf.store,
            });

        drop(state);
        conf.meta.master_lock.take();
    }

    pub async fn acquire(
        &self,
        conf: &mut CreateEnvConfig<DcgiMetadata>,
    ) -> Option<CreateEnvResult> {
        let shard = conf.meta.shard.clone();

        // We take a short lock that looks for existing instances
        // that have been recycled, otherwise we will use the
        // master lock to prevent concurrent creations
        let master_lock = {
            let mut state = self.state.lock().unwrap();
            let shard = state.shards.entry(shard.clone()).or_default();
            shard.last_acquire = Instant::now();
            shard.master_lock.clone()
        };

        // We acquire a master lock whenever creating a new instance and hold it
        // until the instance dies or the instance is returned to the factory. This
        // is done using the `DcgiMetadata`
        let master_lock = master_lock.clone().lock_owned().await;
        conf.meta.master_lock.replace(Arc::new(master_lock));

        // We check the shard again under a short lock as maybe one was returned
        {
            let mut state = self.state.lock().unwrap();
            let shard = state.shards.entry(shard).or_default();
            shard.last_acquire = Instant::now();

            if let Some(inst) = shard.instances.pop_front() {
                tracing::debug!(
                    shard = conf.meta.shard,
                    "attempting to reinitialize DCGI instance"
                );
                match convert_instance(inst, conf) {
                    Ok(converted) => return Some(converted),
                    Err(err) => {
                        tracing::warn!("failed to reinitialize DCGI instance - {}", err);
                    }
                }
            }
        }

        None
    }
}

fn convert_instance(
    inst: DcgiInstance,
    conf: &mut CreateEnvConfig<DcgiMetadata>,
) -> anyhow::Result<CreateEnvResult> {
    let mut env = inst.env;
    let mut store = inst.store;
    let memory = inst.memory;

    let (req_body_sender, req_body_receiver) = Pipe::channel();
    let (res_body_sender, res_body_receiver) = Pipe::channel();
    let (stderr_sender, stderr_receiver) = Pipe::channel();

    env.reinit()?;

    // Replace the environment variables as these will change
    // depending on the WCGI call
    *env.state.envs.lock().unwrap() = conv_env_vars(
        conf.env
            .iter()
            .map(|(k, v)| (k.clone(), v.as_bytes().to_vec()))
            .collect(),
    );

    // The stdio have to be reattached on each call as they are
    // read to completion (EOF) during nominal flows
    env.state
        .fs
        .swap_file(__WASI_STDIN_FILENO, Box::new(req_body_receiver))
        .map_err(WasiStateCreationError::FileSystemError)?;

    env.state
        .fs
        .swap_file(__WASI_STDOUT_FILENO, Box::new(res_body_sender))
        .map_err(WasiStateCreationError::FileSystemError)?;

    env.state
        .fs
        .swap_file(__WASI_STDERR_FILENO, Box::new(stderr_sender))
        .map_err(WasiStateCreationError::FileSystemError)?;

    Ok(CreateEnvResult {
        env,
        store,
        memory: Some(memory),
        body_sender: req_body_sender,
        body_receiver: res_body_receiver,
        stderr_receiver,
    })
}
