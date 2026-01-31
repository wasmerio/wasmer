use std::sync::Arc;

use crate::{
    WasiEnvBuilder, capabilities::Capabilities, fs::Stdio, http::HttpClientCapabilityV1,
    runners::wcgi::callbacks::CreateEnvResult,
};

use super::{RecycleEnvConfig, callbacks::CreateEnvConfig};

pub(crate) async fn default_recycle_env(mut conf: RecycleEnvConfig) {
    tracing::debug!("Destroying the WebAssembly instance");

    conf.env.disable_fs_cleanup = false;
    conf.env.on_exit(None).await;
}

pub(crate) async fn default_create_env(conf: CreateEnvConfig) -> anyhow::Result<CreateEnvResult> {
    tracing::debug!("Creating the WebAssembly instance");

    let (req_body_sender, req_body_receiver) = tokio::io::duplex(64 * 1024);
    let (res_body_sender, res_body_receiver) = tokio::io::duplex(64 * 1024);
    let (stderr_sender, stderr_receiver) = tokio::io::duplex(64 * 1024);

    let mut builder = WasiEnvBuilder::new(&conf.program_name);

    (conf.setup_builder)(&mut builder)?;

    builder.add_envs(conf.env);

    let builder = builder
        .stdin(Arc::new(Stdio::from_reader(Box::new(req_body_receiver))))
        .stdout(Arc::new(Stdio::from_writer(Box::new(res_body_sender))))
        .stderr(Arc::new(Stdio::from_writer(Box::new(stderr_sender))))
        .capabilities(Capabilities {
            insecure_allow_all: true,
            http_client: HttpClientCapabilityV1::new_allow_all(),
            threading: Default::default(),
        });
    let env = builder.build()?;

    Ok(CreateEnvResult {
        env,
        memory: None,
        body_sender: req_body_sender,
        body_receiver: res_body_receiver,
        stderr_receiver,
    })
}
