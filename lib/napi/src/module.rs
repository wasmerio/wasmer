use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use wasmer::{
    sys::{EngineBuilder, Features},
    Module, Store,
};
use wasmer_cache::{Cache, FileSystemCache, Hash as CacheHash};
use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};
use wasmer_types::ModuleHash;

pub struct LoadedWasm {
    pub store: Store,
    pub module: Module,
    pub module_hash: ModuleHash,
}

pub(crate) fn make_store() -> Store {
    let mut features = Features::default();
    features.exceptions(true);
    let mut compiler = LLVM::default();
    compiler.opt_level(LLVMOptLevel::Less);
    let engine = EngineBuilder::new(compiler)
        .set_features(Some(features))
        .engine();
    Store::new(engine)
}

fn wasmer_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("target")
        .join("wasmer-cache")
}

pub(crate) fn load_or_compile_module(store: &Store, wasm_bytes: &[u8]) -> Result<Module> {
    let key = CacheHash::generate(wasm_bytes);
    let mut cache = FileSystemCache::new(wasmer_cache_dir())
        .context("failed to create/access Wasmer cache directory")?;

    if let Ok(module) = unsafe { cache.load(store, key) } {
        return Ok(module);
    }

    let module = Module::new(store, wasm_bytes).context("failed to compile wasm module")?;
    let _ = cache.store(key, &module);
    Ok(module)
}

pub fn load_wasix_module(wasm_path: &Path) -> Result<LoadedWasm> {
    let wasm_bytes = std::fs::read(wasm_path)
        .with_context(|| format!("failed to read wasm file at {}", wasm_path.display()))?;
    let store = make_store();
    let module = load_or_compile_module(&store, &wasm_bytes)?;
    let module_hash = ModuleHash::sha256(&wasm_bytes);

    Ok(LoadedWasm {
        store,
        module,
        module_hash,
    })
}
