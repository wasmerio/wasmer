use crate::tunables::Tunables;
#[cfg(all(feature = "compiler", feature = "engine"))]
use wasmer_compiler::CompilerConfig;
use wasmer_engine::Tunables as BaseTunables;

use std::sync::Arc;
use wasmer_engine::Engine;

/// The store is what holds the directives to the `wasmer` crate, and
/// siblings, to act as expected. It holds the engine (that is
/// —amongst many things— used to compile the Wasm bytes into a valid
/// module artifact), in addition to the tunables (that is used to
/// create the memories, tables and globals).
#[derive(Clone)]
pub struct Store {
    engine: Arc<dyn Engine + Send + Sync>,
    tunables: Arc<dyn BaseTunables + Send + Sync>,
}

impl Store {
    /// Creates a new `Store` with a specific `Engine`.
    pub fn new<E>(engine: &E) -> Store
    where
        E: Engine + ?Sized,
    {
        Store {
            engine: engine.cloned(),
            tunables: Arc::new(Tunables::for_target(engine.target())),
        }
    }

    /// Creates a new `Store` with a specific `Engine` and a specific
    /// `Tunables`.
    pub fn new_with_tunables<E>(
        engine: &E,
        tunables: impl BaseTunables + Send + Sync + 'static,
    ) -> Store
    where
        E: Engine + ?Sized,
    {
        Store {
            engine: engine.cloned(),
            tunables: Arc::new(tunables),
        }
    }

    /// Returns the tunables.
    pub fn tunables(&self) -> &dyn BaseTunables {
        self.tunables.as_ref()
    }

    /// Returns the engine.
    pub fn engine(&self) -> &Arc<dyn Engine + Send + Sync> {
        &self.engine
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Store, b: &Store) -> bool {
        a.engine.id() == b.engine.id()
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Store::same(self, other)
    }
}

// We only implement default if we have assigned a default compiler and engine
#[cfg(all(feature = "default-compiler", feature = "default-engine"))]
impl Default for Store {
    fn default() -> Store {
        // We store them on a function that returns to make
        // sure this function doesn't emit a compile error even if
        // more than one compiler is enabled.
        #[allow(unreachable_code)]
        fn get_config() -> impl CompilerConfig + Send + Sync {
            cfg_if::cfg_if! {
                if #[cfg(feature = "default-cranelift")] {
                    wasmer_compiler_cranelift::Cranelift::default()
                } else if #[cfg(feature = "default-llvm")] {
                    wasmer_compiler_llvm::LLVM::default()
                } else if #[cfg(feature = "default-singlepass")] {
                    wasmer_compiler_singlepass::Singlepass::default()
                } else {
                    compile_error!("No default compiler chosen")
                }
            }
        }

        #[allow(unreachable_code)]
        fn get_engine(config: impl CompilerConfig + Send + Sync) -> impl Engine + Send + Sync {
            cfg_if::cfg_if! {
                if #[cfg(feature = "default-jit")] {
                    wasmer_engine_jit::JIT::new(&config)
                        .engine()
                } else if #[cfg(feature = "default-native")] {
                    wasmer_engine_native::Native::new(&config)
                        .engine()
                } else {
                    compile_error!("No default engine chosen")
                }
            }
        }

        let config = get_config();
        let engine = get_engine(config);
        let tunables = Tunables::for_target(engine.target());
        Store {
            engine: Arc::new(engine),
            tunables: Arc::new(tunables),
        }
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
