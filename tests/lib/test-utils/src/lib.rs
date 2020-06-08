#![cfg(feature = "compiler")]

#[macro_use]
pub mod macros;
pub use macros::*;

use std::sync::Arc;
use wasmer::{Store, Tunables};
use wasmer_compiler::{CompilerConfig, Features, Target};
use wasmer_engine_jit::JITEngine;

pub fn get_compiler_config_from_str(
    compiler_name: &str,
    try_nan_canonicalization: bool,
    features: Features,
) -> Box<dyn CompilerConfig> {
    // We use the current host target for testing locally
    let target = Target::default();

    match compiler_name {
        #[cfg(feature = "singlepass")]
        "singlepass" => {
            let mut singlepass_config =
                wasmer_compiler_singlepass::SinglepassConfig::new(features, target);
            singlepass_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(singlepass_config)
        }
        #[cfg(feature = "cranelift")]
        "cranelift" => {
            let mut cranelift_config =
                wasmer_compiler_cranelift::CraneliftConfig::new(features, target);
            cranelift_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(cranelift_config)
        }
        #[cfg(feature = "llvm")]
        "llvm" => {
            let mut llvm_config = wasmer_compiler_llvm::LLVMConfig::new(features, target);
            llvm_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(llvm_config)
        }
        _ => panic!("Compiler {} not supported", compiler_name),
    }
}

/// for when you need a store but you don't care about the details
pub fn get_default_store() -> Store {
    let compiler_config = get_compiler_config_from_str("cranelift", false, Features::default());
    let tunables = Tunables::for_target(compiler_config.target().triple());
    Store::new(Arc::new(JITEngine::new(compiler_config, tunables)))
}
