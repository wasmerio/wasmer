use virtual_fs::Pipe;

use crate::{
    capabilities::Capabilities, http::HttpClientCapabilityV1,
    runners::wcgi::callbacks::CreateEnvResult, WasiEnvBuilder,
};

use super::{callbacks::CreateEnvConfig, RecycleEnvConfig};

pub(crate) async fn default_recycle_env(mut conf: RecycleEnvConfig) {
    tracing::debug!("Destroying the WebAssembly instance");

    conf.env.disable_fs_cleanup = false;
    conf.env.on_exit(None).await;
}

pub(crate) async fn default_create_env(conf: CreateEnvConfig) -> anyhow::Result<CreateEnvResult> {
    tracing::debug!("Creating the WebAssembly instance");

    let (req_body_sender, req_body_receiver) = Pipe::channel();
    let (res_body_sender, res_body_receiver) = Pipe::channel();
    let (stderr_sender, stderr_receiver) = Pipe::channel();

    let mut builder = WasiEnvBuilder::new(&conf.program_name);

    (conf.setup_builder)(&mut builder)?;

    builder.add_envs(conf.env);

    let builder = builder
        .stdin(Box::new(req_body_receiver))
        .stdout(Box::new(res_body_sender))
        .stderr(Box::new(stderr_sender))
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
