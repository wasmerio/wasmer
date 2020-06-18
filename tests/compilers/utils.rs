use std::sync::Arc;
use wasmer::{CompilerConfig, Features, FunctionMiddlewareGenerator, Store, Triple, Tunables};
use wasmer_engine::Engine;
use wasmer_engine_jit::JIT;

pub fn get_compiler(canonicalize_nans: bool) -> impl CompilerConfig {
    cfg_if::cfg_if! {
        if #[cfg(any(
            all(feature = "test-llvm", any(feature = "test-cranelift", feature = "test-singlepass")),
            all(feature = "test-cranelift", feature = "test-singlepass")
        ))] {
            compile_error!("Only one compiler can be selected")
        } else if #[cfg(feature = "test-cranelift")] {
            let mut compiler = wasmer_compiler_cranelift::Cranelift::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler
        } else if #[cfg(feature = "test-llvm")] {
            let mut compiler = wasmer_compiler_llvm::LLVM::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler
        } else if #[cfg(feature = "test-singlepass")] {
            let mut compiler = wasmer_compiler_singlepass::Singlepass::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler
        } else {
            compile_error!("No compiler chosen for the tests")
        }
    }
}

pub fn get_engine() -> impl Engine {
    let mut features = Features::default();
    #[cfg(feature = "test-singlepass")]
    features.multi_value(false);

    let compiler_config = get_compiler(false);
    JIT::new(&compiler_config)
        .tunables(Tunables::for_target)
        .features(features)
        .engine()
}

pub fn get_store() -> Store {
    Store::new(&get_engine())
}

pub fn get_store_with_middlewares<I: Iterator<Item = Arc<dyn FunctionMiddlewareGenerator>>>(
    middlewares: I,
) -> Store {
    let mut features = Features::default();
    #[cfg(feature = "test-singlepass")]
    features.multi_value(false);

    let mut compiler_config = get_compiler(false);
    for x in middlewares {
        compiler_config.push_middleware(x);
    }
    let engine = JIT::new(&compiler_config)
        .tunables(Tunables::for_target)
        .features(features)
        .engine();
    Store::new(&engine)
}

pub fn get_headless_store() -> Store {
    Store::new(&JIT::headless().tunables(Tunables::for_target).engine())
}
