//! Re-export of the standalone `wasmer-module-cache` crate to preserve the
//! historic module path within `wasmer-wasix`.

pub use wasmer_module_cache::*;

#[cfg(feature = "sys-thread")]
use crate::runtime::task_manager::tokio::TokioTaskManager;
#[cfg(feature = "sys-thread")]
use wasmer_module_cache::TokioHandleProvider;

#[cfg(feature = "sys-thread")]
impl TokioHandleProvider for TokioTaskManager {
    fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime_handle()
    }
}
