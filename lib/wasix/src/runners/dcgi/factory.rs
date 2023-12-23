use std::sync::{Arc, Mutex};

use derivative::Derivative;
use virtual_fs::Pipe;
use wasmer_wasix_types::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};

use crate::{
    runners::wcgi::{CreateEnvConfig, CreateEnvResult, RecycleEnvConfig},
    state::conv_env_vars,
    WasiStateCreationError,
};

use super::*;

#[derive(Debug, Default)]
struct State {
    /// Once the instance is running it will
    instance: Option<DcgiInstance>,
}

/// This factory will store and reuse instances between invocations thus
/// allowing for the instances to be stateful.
#[derive(Derivative, Clone, Default)]
#[derivative(Debug)]
pub struct DcgiInstanceFactory {
    state: Arc<Mutex<State>>,
}

impl DcgiInstanceFactory {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn release(&self, conf: RecycleEnvConfig) {
        let mut state = self.state.lock().unwrap();
        state.instance.replace(DcgiInstance {
            env: conf.env,
            //memory: conf.memory,
            //store: conf.store,
        });
    }

    pub async fn acquire(&self, conf: &mut CreateEnvConfig) -> Option<CreateEnvResult> {
        let mut state = self.state.lock().unwrap();
        if let Some(inst) = state.instance.take() {
            tracing::debug!("attempting to reinitialize DCGI instance");
            match convert_instance(inst, conf) {
                Ok(converted) => return Some(converted),
                Err(err) => {
                    tracing::warn!("failed to reinitialize DCGI instance - {}", err);
                }
            }
        }

        None
    }
}

fn convert_instance(
    inst: DcgiInstance,
    conf: &mut CreateEnvConfig,
) -> anyhow::Result<CreateEnvResult> {
    let mut env = inst.env;

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
        //memory: Some((inst.memory, inst.store)),
        memory: None,
        body_sender: req_body_sender,
        body_receiver: res_body_receiver,
        stderr_receiver,
    })
}
