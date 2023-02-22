mod cached;
mod file_loader;
mod wasm;
mod webc;

use bytes::Bytes;
use tokio::runtime::Handle;
use wasmer::{Engine, Module};
use wcgi_host::CgiDialect;

use crate::Error;

pub(crate) use self::{
    cached::Cached,
    file_loader::FileLoader,
    wasm::WasmLoader,
    webc::{WebcCommand, WebcLoader, WebcOptions},
};

#[async_trait::async_trait]
pub(crate) trait ModuleLoader: Send + Sync {
    async fn load(&self, ctx: ModuleLoaderContext<'_>) -> Result<LoadedModule, Error>;

    /// Wrap this [`ModuleLoader`] with a cache that will only reload the
    /// module when `invalidated` returns `true`.
    fn cached(self, invalidated: impl Fn() -> bool + Send + Sync + 'static) -> Cached<Self>
    where
        Self: Sized,
    {
        Cached::new(self, invalidated)
    }

    fn load_once(self) -> Cached<Self>
    where
        Self: Sized,
    {
        Cached::new(self, || false)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedModule {
    pub(crate) program: String,
    pub(crate) module: Module,
    pub(crate) dialect: CgiDialect,
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleLoaderContext<'a> {
    engine: &'a Engine,
    handle: &'a Handle,
}

impl<'a> ModuleLoaderContext<'a> {
    pub(crate) fn new(engine: &'a Engine, handle: &'a Handle) -> Self {
        ModuleLoaderContext { engine, handle }
    }

    pub(crate) async fn compile_wasm(&self, wasm: Bytes) -> Result<Module, Error> {
        let engine = self.engine.clone();
        let module = self
            .handle
            .spawn_blocking(move || Module::new(&engine, &wasm))
            .await??;
        Ok(module)
    }
}
