use std::sync::Arc;
use test_utils::get_compiler_config_from_str;
use wasmer::{Features, Store, Tunables};
use wasmer_engine_jit::JITEngine;

fn get_compiler_str() -> &'static str {
    cfg_if::cfg_if! {
        if #[cfg(feature = "test-cranelift")] {
            "cranelift"
        } else if #[cfg(feature = "test-llvm")] {
            "llvm"
        } else if #[cfg(feature = "test-singlepass")] {
            "singlepass"
        }
        else {
            compile_error!("No compiler chosen for the tests")
        }
    }
}

pub fn get_store() -> Store {
    let features = Features::default();
    let try_nan_canonicalization = false;
    let compiler_config =
        get_compiler_config_from_str(get_compiler_str(), try_nan_canonicalization, features);
    let tunables = Tunables::for_target(compiler_config.target().triple());
    let store = Store::new(Arc::new(JITEngine::new(compiler_config, tunables)));
    store
}
