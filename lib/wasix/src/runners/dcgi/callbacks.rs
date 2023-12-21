use std::sync::Arc;

use derivative::Derivative;

use super::*;
use crate::runners::wcgi::{self, CreateEnvConfig, CreateEnvResult, RecycleEnvConfig};
use virtual_fs::NullFile;
use wasmer_wasix_types::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct DcgiCallbacks {
    #[derivative(Debug = "ignore")]
    inner: Arc<dyn wcgi::Callbacks<DcgiMetadata>>,
    factory: DcgiInstanceFactory,
}

impl DcgiCallbacks {
    pub fn new<C>(factory: DcgiInstanceFactory, inner: C) -> Self
    where
        C: wcgi::Callbacks<DcgiMetadata>,
    {
        Self {
            inner: Arc::new(inner),
            factory,
        }
    }
}

#[async_trait::async_trait]
impl wcgi::Callbacks<DcgiMetadata> for DcgiCallbacks {
    fn started(&self, abort: AbortHandle) {
        self.inner.started(abort)
    }

    fn on_stderr(&self, stderr: &[u8]) {
        self.inner.on_stderr(stderr)
    }

    fn on_stderr_error(&self, error: std::io::Error) {
        self.inner.on_stderr_error(error)
    }

    async fn recycle_env(&self, mut conf: RecycleEnvConfig<DcgiMetadata>) {
        tracing::debug!(shard = conf.meta.shard, "recycling DCGI instance");

        // We cycle out the stdio so that the pipes close properly
        {
            let env = conf.env.data_mut(&mut conf.store);

            // The stdio have to be reattached on each call as they are
            // read to completion (EOF) during nominal flows
            env.state
                .fs
                .swap_file(__WASI_STDIN_FILENO, Box::new(NullFile::default()))
                .ok();
            env.state
                .fs
                .swap_file(__WASI_STDOUT_FILENO, Box::new(NullFile::default()))
                .ok();
            env.state
                .fs
                .swap_file(__WASI_STDERR_FILENO, Box::new(NullFile::default()))
                .ok();
        }

        // Now we make the instance available for reuse
        self.factory.release(conf).await;
    }

    async fn create_env(
        &self,
        mut conf: CreateEnvConfig<DcgiMetadata>,
    ) -> anyhow::Result<CreateEnvResult> {
        tracing::debug!(
            shard = conf.meta.shard,
            "attempting to acquire existing DCGI instance"
        );

        if let Some(res) = self.factory.acquire(&mut conf).await {
            tracing::debug!(shard = conf.meta.shard, "found existing DCGI instance");
            return Ok(res);
        }

        let mut ret = self.inner.create_env(conf).await;

        if let Ok(ret) = ret.as_mut() {
            // We disable the cleanup to prevent the instance so that the
            // resources can be reused
            ret.env.clone().data_mut(&mut ret.store).disable_cleanup = true;
        }

        ret
    }
}
