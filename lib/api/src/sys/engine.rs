pub use wasmer_compiler::BaseTunables;
pub use wasmer_compiler::{Artifact, Engine, EngineBuilder};

/// Returns the default engine for the Sys engine
pub(crate) fn default_engine() -> Engine {
    // We store them on a function that returns to make
    // sure this function doesn't emit a compile error even if
    // more than one compiler is enabled.
    #[allow(unreachable_code)]
    #[cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]
    fn get_config() -> impl wasmer_compiler::CompilerConfig + 'static {
        cfg_if::cfg_if! {
            if #[cfg(feature = "cranelift")] {
                wasmer_compiler_cranelift::Cranelift::default()
            } else if #[cfg(feature = "llvm")] {
                wasmer_compiler_llvm::LLVM::default()
            } else if #[cfg(feature = "singlepass")] {
                wasmer_compiler_singlepass::Singlepass::default()
            } else {
                compile_error!("No default compiler chosen")
            }
        }
    }

    #[allow(unreachable_code, unused_mut)]
    fn get_engine() -> Engine {
        cfg_if::cfg_if! {
            if #[cfg(feature = "compiler")] {
                cfg_if::cfg_if! {
                    if #[cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]
                    {
                        let config = get_config();
                        EngineBuilder::new(Box::new(config) as Box<dyn wasmer_compiler::CompilerConfig>)
                            .engine()
                    } else {
                        EngineBuilder::headless()
                            .engine()
                    }
                }
            } else {
                compile_error!("No default engine chosen")
            }
        }
    }

    let mut engine = get_engine();
    let tunables = BaseTunables::for_target(engine.target());
    engine.set_tunables(tunables);
    engine
}
