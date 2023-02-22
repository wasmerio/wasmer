use bytes::Bytes;

use wcgi_host::CgiDialect;

use crate::{
    module_loader::{LoadedModule, ModuleLoader, ModuleLoaderContext},
    Error,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WasmLoader {
    program: String,
    wasm: Bytes,
    dialect: CgiDialect,
}

impl WasmLoader {
    pub fn new(program: String, wasm: Bytes) -> Self {
        WasmLoader::new_with_dialect(program, wasm, CgiDialect::Wcgi)
    }

    pub fn new_with_dialect(program: String, wasm: Bytes, dialect: CgiDialect) -> Self {
        WasmLoader {
            program,
            wasm,
            dialect,
        }
    }
}

#[async_trait::async_trait]
impl ModuleLoader for WasmLoader {
    async fn load(&self, ctx: ModuleLoaderContext<'_>) -> Result<LoadedModule, Error> {
        Ok(LoadedModule {
            module: ctx.compile_wasm(self.wasm.clone()).await?,
            dialect: self.dialect,
            program: self.program.clone(),
        })
    }
}
