use std::sync::{Arc, Mutex};

use wasmer_wasix_types::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};

use crate::{
    WasiStateCreationError,
    fs::Stdio,
    runners::wcgi::{CreateEnvConfig, CreateEnvResult, RecycleEnvConfig},
    state::conv_env_vars,
};

use super::*;

#[derive(Debug, Default)]
struct State {
    /// Once the instance is running it will
    instance: Option<DcgiInstance>,
}

/// This factory will store and reuse instances between invocations thus
/// allowing for the instances to be stateful.
#[derive(Debug, Clone, Default)]
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

    let (req_body_sender, req_body_receiver) = tokio::io::duplex(64 * 1024);
    let (res_body_sender, res_body_receiver) = tokio::io::duplex(64 * 1024);
    let (stderr_sender, stderr_receiver) = tokio::io::duplex(64 * 1024);

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
    env.state.fs.close_fd(__WASI_STDIN_FILENO).ok();
    env.state
        .fs
        .with_fd(
            wasmer_wasix_types::wasi::Rights::FD_READ
                | wasmer_wasix_types::wasi::Rights::POLL_FD_READWRITE,
            wasmer_wasix_types::wasi::Rights::empty(),
            wasmer_wasix_types::wasi::Fdflags::empty(),
            wasmer_wasix_types::wasi::Fdflagsext::empty(),
            crate::fs::Kind::Stdin {
                handle: Arc::new(Stdio::from_reader(Box::new(req_body_receiver))),
            },
            __WASI_STDIN_FILENO,
        )
        .map_err(|err| WasiStateCreationError::WasiFsSetupError(format!("{err:?}")))?;

    env.state.fs.close_fd(__WASI_STDOUT_FILENO).ok();
    env.state
        .fs
        .with_fd(
            wasmer_wasix_types::wasi::Rights::FD_WRITE
                | wasmer_wasix_types::wasi::Rights::POLL_FD_READWRITE,
            wasmer_wasix_types::wasi::Rights::empty(),
            wasmer_wasix_types::wasi::Fdflags::APPEND,
            wasmer_wasix_types::wasi::Fdflagsext::empty(),
            crate::fs::Kind::Stdout {
                handle: Arc::new(Stdio::from_writer(Box::new(res_body_sender))),
            },
            __WASI_STDOUT_FILENO,
        )
        .map_err(|err| WasiStateCreationError::WasiFsSetupError(format!("{err:?}")))?;

    env.state.fs.close_fd(__WASI_STDERR_FILENO).ok();
    env.state
        .fs
        .with_fd(
            wasmer_wasix_types::wasi::Rights::FD_WRITE
                | wasmer_wasix_types::wasi::Rights::POLL_FD_READWRITE,
            wasmer_wasix_types::wasi::Rights::empty(),
            wasmer_wasix_types::wasi::Fdflags::APPEND,
            wasmer_wasix_types::wasi::Fdflagsext::empty(),
            crate::fs::Kind::Stderr {
                handle: Arc::new(Stdio::from_writer(Box::new(stderr_sender))),
            },
            __WASI_STDERR_FILENO,
        )
        .map_err(|err| WasiStateCreationError::WasiFsSetupError(format!("{err:?}")))?;

    Ok(CreateEnvResult {
        env,
        //memory: Some((inst.memory, inst.store)),
        memory: None,
        body_sender: req_body_sender,
        body_receiver: res_body_receiver,
        stderr_receiver,
    })
}
