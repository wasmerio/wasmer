//! Wasmer Runtime Core Library
//!
//! # Important Note; Please Read
//!
//! Wasmer has entirely changed its API (called the “new API”
//! here). This new version of Wasmer improves the performance and the
//! memory consumption, in addition to a ton of new features and much
//! more flexibility! In order to help users to enjoy the performance
//! boost and memory improvements without updating your program that
//! much, we have created a new version of the `wasmer-runtime-core`
//! crate, which is now *a port* of the new API but with the old API,
//! as much as possible. Indeed, it was not always possible to provide
//! the exact same API, but changes are subtle.
//!
//! We have carefully documented most of the differences. It is
//! important to understand the public of this port (see the
//! `CHANGES.md`) document. We do not recommend to advanced users of
//! Wasmer to use this port. Advanced API, like `ModuleInfo` or the
//! `vm` module (incl. `vm::Ctx`) have not been fully ported because
//! it was very internals to Wasmer. For advanced users, we highly
//! recommend to migrate to the new version of Wasmer, which is
//! awesome by the way (completely neutral opinion). The public for
//! this port is beginners or regular users that do not necesarily
//! have time to update their code immediately but that want to enjoy
//! a performance boost and memory improvements.
//!
//! # Introduction
//!
//! This crate provides common data structures which are shared by
//! compiler backends to implement a WebAssembly runtime.
//!
//! This crate also provides an API for users who use wasmer as an
//! embedded wasm runtime which allows operations like compiling,
//! instantiating, providing imports, access exports, memories, and
//! tables for example.
//!
//! Most wasmer users should prefer the API which is re-exported by
//! the `wasmer-runtime` library by default. This crate provides
//! additional APIs which may be useful to users that wish to
//! customize the wasmer runtime.

pub(crate) mod new {
    pub use wasmer;
    pub use wasmer_cache;
    pub use wasmer_compiler;
    #[cfg(feature = "cranelift")]
    pub use wasmer_compiler_cranelift;
    #[cfg(feature = "llvm")]
    pub use wasmer_compiler_llvm;
    #[cfg(feature = "singlepass")]
    pub use wasmer_compiler_singlepass;
    pub use wasmer_engine;
    pub use wasmer_engine_universal;
    pub use wasmer_types;
    pub use wasmer_vm;
}

pub mod backend;
pub mod cache;
pub mod error;
pub mod export;
mod functional_api;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod structures;
pub mod table;
pub mod typed_func;
pub mod types;
pub mod units;
pub mod vm;

pub use crate::cache::{Artifact, WasmHash};
#[allow(deprecated)]
pub use crate::import::IsExport;
pub use crate::instance::{DynFunc, Exports, Instance};
pub use crate::module::Module;
pub use crate::new::wasmer_compiler::wasmparser;
pub use crate::typed_func::{DynamicFunc, Func};
pub use crate::units::{Bytes, Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
pub use functional_api::{
    compile, compile_with, compile_with_config, load_cache_with, validate, wat2wasm,
};

pub mod prelude {
    pub use crate::import::{namespace, ImportObject, Namespace};
    pub use crate::typed_func::{DynamicFunc, Func};
    pub use crate::types::{FuncIndex, GlobalIndex, MemoryIndex, TableIndex, Type, Value};
}

/// The current version of this crate
pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

use backend::Backend;
use std::sync::{Arc, Mutex};

struct GlobalStore {
    store: Mutex<Arc<new::wasmer::Store>>,
    backend: Backend,
}

impl GlobalStore {
    fn new() -> Self {
        Self {
            store: Mutex::new(Arc::new(Default::default())),
            backend: Backend::Auto,
        }
    }

    fn renew_with(&self, compiler: backend::Backend) {
        if compiler == self.backend {
            return;
        }

        #[allow(unused_variables)]
        let update = |engine: new::wasmer_engine_universal::Universal,
                      global_store: &GlobalStore| {
            let engine = engine.engine();
            *self.store.lock().unwrap() = Arc::new(new::wasmer::Store::new(&engine));
        };

        match compiler {
            #[cfg(feature = "singlepass")]
            Backend::Singlepass => update(
                new::wasmer_engine_universal::Universal::new(
                    new::wasmer_compiler_singlepass::Singlepass::default(),
                ),
                &self,
            ),

            #[cfg(feature = "cranelift")]
            Backend::Cranelift => update(
                new::wasmer_engine_universal::Universal::new(
                    new::wasmer_compiler_cranelift::Cranelift::default(),
                ),
                &self,
            ),

            #[cfg(feature = "llvm")]
            Backend::LLVM => update(
                new::wasmer_engine_universal::Universal::new(
                    new::wasmer_compiler_llvm::LLVM::default(),
                ),
                &self,
            ),

            Backend::Auto => *self.store.lock().unwrap() = Arc::new(Default::default()),
        };
    }

    fn inner_store(&self) -> Arc<new::wasmer::Store> {
        (*self.store.lock().unwrap()).clone()
    }
}

lazy_static::lazy_static! {
    static ref GLOBAL_STORE: GlobalStore = GlobalStore::new();
}

/// Useful if one needs to update the global store.
pub(crate) fn renew_global_store_with(backend: Backend) {
    GLOBAL_STORE.renew_with(backend);
}

/// Useful if one needs to migrate to the new Wasmer's API gently.
pub fn get_global_store() -> Arc<new::wasmer::Store> {
    GLOBAL_STORE.inner_store()
}
