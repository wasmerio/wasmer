use virtual_fs::Pipe;

use crate::{
    capabilities::Capabilities, http::HttpClientCapabilityV1,
    runners::wcgi::callbacks::CreateEnvResult, WasiEnvBuilder,
};

use super::{callbacks::CreateEnvConfig, RecycleEnvConfig};

pub(crate) async fn default_recycle_env<M>(_conf: RecycleEnvConfig<M>)
where
    M: Send + Sync + 'static,
{
    tracing::debug!("Destroying the WebAssembly instance");
}

pub(crate) async fn default_create_env<M>(
    conf: CreateEnvConfig<M>,
) -> anyhow::Result<CreateEnvResult>
where
    M: Send + Sync + 'static,
{
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

    let mut store = conf.runtime.new_store();
    let (_, env) = builder.instantiate_ext(conf.module, conf.module_hash, &mut store)?;
    Ok(CreateEnvResult {
        env,
        store,
        body_sender: req_body_sender,
        body_receiver: res_body_receiver,
        stderr_receiver,
    })
}
