use std::sync::Arc;

use anyhow::Error;
use virtual_net::VirtualNetworking;

use crate::{
    http::{web::WebHttpClient, HttpClient},
    runtime::{
        module_cache::{ModuleCache, WebWorkerModuleCache},
        package_loader::{BuiltinPackageLoader, PackageLoader, UnsupportedPackageLoader},
        resolver::{PackageSpecifier, PackageSummary, QueryError, Source, WapmSource},
        task_manager::web::{WebTaskManager, WebThreadPool},
    },
    Runtime, VirtualTaskManager,
};

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct WebRuntime {
    pool: WebThreadPool,
    task_manager: Arc<dyn VirtualTaskManager>,
    networking: Arc<dyn VirtualNetworking>,
    source: Arc<dyn Source + Send + Sync>,
    http_client: Arc<dyn HttpClient + Send + Sync>,
    package_loader: Arc<dyn PackageLoader + Send + Sync>,
    module_cache: Arc<WebWorkerModuleCache>,
    #[derivative(Debug = "ignore")]
    tty: Option<Arc<dyn crate::os::TtyBridge + Send + Sync>>,
}

impl WebRuntime {
    pub fn with_pool_size(pool_size: usize) -> Self {
        let pool = WebThreadPool::new(pool_size);
        WebRuntime::new(pool)
    }

    pub fn with_max_threads() -> Result<Self, anyhow::Error> {
        let pool = WebThreadPool::new_with_max_threads()?;
        Ok(WebRuntime::new(pool))
    }

    pub fn new(pool: WebThreadPool) -> Self {
        let task_manager = WebTaskManager::new(pool.clone());
        let http_client = Arc::new(WebHttpClient::default());
        let package_loader = BuiltinPackageLoader::new_only_client(http_client.clone());
        let module_cache = WebWorkerModuleCache::default();

        WebRuntime {
            pool,
            task_manager: Arc::new(task_manager),
            networking: Arc::new(virtual_net::UnsupportedVirtualNetworking::default()),
            source: Arc::new(UnsupportedSource),
            http_client: Arc::new(http_client),
            package_loader: Arc::new(package_loader),
            module_cache: Arc::new(module_cache),
            tty: None,
        }
    }

    /// Set the registry that packages will be fetched from.
    pub fn with_registry(&mut self, url: &str) -> Result<&mut Self, url::ParseError> {
        let url = url.parse()?;
        self.source = Arc::new(WapmSource::new(url, self.http_client.clone()));
        Ok(self)
    }

    /// Enable networking (i.e. TCP and UDP) via a gateway server.
    pub fn with_network_gateway(&mut self, gateway_url: impl Into<String>) -> &mut Self {
        let networking = crate::runtime::web::net::connect_networking(gateway_url.into());
        self.networking = Arc::new(networking);
        self
    }

    pub fn with_tty(
        &mut self,
        tty: impl crate::os::TtyBridge + Send + Sync + 'static,
    ) -> &mut Self {
        self.tty = Some(Arc::new(tty));
        self
    }
}

impl Runtime for WebRuntime {
    fn networking(&self) -> &Arc<dyn VirtualNetworking> {
        &self.networking
    }

    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager> {
        &self.task_manager
    }

    fn source(&self) -> Arc<dyn crate::runtime::resolver::Source + Send + Sync> {
        self.source.clone()
    }

    fn http_client(&self) -> Option<&crate::http::DynHttpClient> {
        Some(&self.http_client)
    }

    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync> {
        self.package_loader.clone()
    }

    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync> {
        self.module_cache.clone()
    }

    fn tty(&self) -> Option<&(dyn crate::os::TtyBridge + Send + Sync)> {
        self.tty.as_deref()
    }
}

/// A  that will always error out with [`QueryError::Unsupported`].
#[derive(Debug, Clone)]
struct UnsupportedSource;

#[async_trait::async_trait]
impl Source for UnsupportedSource {
    async fn query(&self, _package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError> {
        Err(QueryError::Unsupported)
    }
}
