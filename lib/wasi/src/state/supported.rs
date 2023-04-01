#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer::Module;

/// This structure returns a list of capabilities that the module
/// is capable of - this is reverse engineered from the imports
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct WasiSupportedStuff {
    /// The module is compiled against WASIX
    pub wasix: bool,
    /// The module supports an externalized current directory
    pub pwd: bool,
    /// The module supports asynchronous threading
    pub asyncify: bool,
    /// The module supports signaling
    pub signal: bool,
    /// The module supports threading
    pub threading: bool,
    /// The module uses the `sock_listen()` syscall
    pub socket_listen: bool,
    /// The module uses the `sock_accept()` syscall
    pub socket_accept: bool,
}

impl WasiSupportedStuff {
    pub fn new(module: &Module) -> Self {
        let is_wasix = crate::utils::is_wasix_module(module);
        Self {
            wasix: is_wasix,
            pwd: is_wasix,
            asyncify: module
                .exports()
                .any(|e| e.name() == "asyncify_start_unwind"),
            signal: module.exports().any(|e| e.name() == "__wasm_signal"),
            threading: module.exports().any(|e| e.name() == "wasi_thread_start"),
            socket_listen: module.imports().any(|i| i.name() == "sock_listen"),
            socket_accept: module.imports().any(|i| i.name() == "sock_accept"),
        }
    }
}
