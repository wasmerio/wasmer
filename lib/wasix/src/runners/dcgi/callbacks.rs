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
    inner: Arc<dyn wcgi::Callbacks>,
    factory: DcgiInstanceFactory,
}

impl DcgiCallbacks {
    pub fn new<C>(factory: DcgiInstanceFactory, inner: C) -> Self
    where
        C: wcgi::Callbacks,
    {
        Self {
            inner: Arc::new(inner),
            factory,
        }
    }
}

#[async_trait::async_trait]
impl wcgi::Callbacks for DcgiCallbacks {
    fn started(&self, abort: AbortHandle) {
        self.inner.started(abort)
    }

    fn on_stderr(&self, stderr: &[u8]) {
        self.inner.on_stderr(stderr)
    }

    fn on_stderr_error(&self, error: std::io::Error) {
        self.inner.on_stderr_error(error)
    }

    async fn recycle_env(&self, conf: RecycleEnvConfig) {
        tracing::debug!("recycling DCGI instance");

        // The stdio have to be reattached on each call as they are
        // read to completion (EOF) during nominal flows
        conf.env
            .state
            .fs
            .swap_file(__WASI_STDIN_FILENO, Box::<NullFile>::default())
            .ok();
        conf.env
            .state
            .fs
            .swap_file(__WASI_STDOUT_FILENO, Box::<NullFile>::default())
            .ok();
        conf.env
            .state
            .fs
            .swap_file(__WASI_STDERR_FILENO, Box::<NullFile>::default())
            .ok();

        // Now we make the instance available for reuse
        self.factory.release(conf).await;
    }

    async fn create_env(&self, mut conf: CreateEnvConfig) -> anyhow::Result<CreateEnvResult> {
        tracing::debug!("attempting to acquire existing DCGI instance");

        if let Some(res) = self.factory.acquire(&mut conf).await {
            tracing::debug!("found existing DCGI instance");
            return Ok(res);
        }

        let mut ret = self.inner.create_env(conf).await;

        if let Ok(ret) = ret.as_mut() {
            // We disable the cleanup to prevent the instance so that the
            // resources can be reused
            ret.env.disable_fs_cleanup = true;
        }

        ret
    }
}
