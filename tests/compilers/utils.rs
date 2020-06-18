use std::sync::Arc;
use test_utils::get_compiler_config_from_str;
use wasmer::{Features, FunctionMiddlewareGenerator, Store, Triple, Tunables};
use wasmer_engine_jit::JIT;

fn get_compiler_str() -> &'static str {
    cfg_if::cfg_if! {
        if #[cfg(any(
            all(feature = "test-llvm", any(feature = "test-cranelift", feature = "test-singlepass")),
            all(feature = "test-cranelift", feature = "test-singlepass")
        ))] {
            compile_error!("Only one compiler can be selected")
        } else if #[cfg(feature = "test-cranelift")] {
            "cranelift"
        } else if #[cfg(feature = "test-llvm")] {
            "llvm"
        } else if #[cfg(feature = "test-singlepass")] {
            "singlepass"
        } else {
            compile_error!("No compiler chosen for the tests")
        }
    }
}

pub fn get_store() -> Store {
    let mut features = Features::default();
    #[cfg(feature = "test-singlepass")]
    features.multi_value(false);
    let try_nan_canonicalization = false;
    let compiler_config =
        get_compiler_config_from_str(get_compiler_str(), try_nan_canonicalization);
    let store = Store::new(
        &JIT::new(&*compiler_config)
            .tunables(Tunables::for_target)
            .features(features)
            .engine(),
    );
    store
}

pub fn get_store_with_middlewares<I: Iterator<Item = Arc<dyn FunctionMiddlewareGenerator>>>(
    middlewares: I,
) -> Store {
    let mut features = Features::default();
    #[cfg(feature = "test-singlepass")]
    features.multi_value(false);
    let try_nan_canonicalization = false;
    let mut compiler_config =
        get_compiler_config_from_str(get_compiler_str(), try_nan_canonicalization);
    for x in middlewares {
        compiler_config.push_middleware(x);
    }
    let store = Store::new(
        &JIT::new(&*compiler_config)
            .tunables(Tunables::for_target)
            .features(features)
            .engine(),
    );
    store
}

pub fn get_headless_store() -> Store {
    let store = Store::new(&JIT::headless().tunables(Tunables::for_target).engine());
    store
}
