use std::sync::Arc;

use derivative::Derivative;

use super::*;
use crate::runners::wcgi::{self, CreateEnvConfig, CreateEnvResult, RecycleEnvConfig};

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

    async fn recycle_env(&self, conf: RecycleEnvConfig<DcgiMetadata>) {
        self.factory.release(conf).await;
    }

    async fn create_env(
        &self,
        mut conf: CreateEnvConfig<DcgiMetadata>,
    ) -> anyhow::Result<CreateEnvResult> {
        if let Some(res) = self.factory.acquire(&mut conf).await {
            return Ok(res);
        }
        self.inner.create_env(conf).await
    }
}
