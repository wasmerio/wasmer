use wasmer::Tunables;

/// Abstracts the Webassembly compiler.
// NOTE: currently only a stub, will be expanded with actual compilation capability in the future.
pub trait Compiler: std::fmt::Debug {
    fn engine(&self) -> Option<wasmer::Engine>;
    fn new_store(&self, tunables: Option<ArcTunables>) -> wasmer::Store;
}

pub type DynCompiler = std::sync::Arc<dyn Compiler + Send + Sync + 'static>;

#[derive(Clone, Debug)]
pub struct StubCompiler;

impl Compiler for StubCompiler {
    fn engine(&self) -> Option<wasmer::Engine> {
        None
    }

    fn new_store(&self, tunables: Option<ArcTunables>) -> wasmer::Store {
        if let Some(tunables) = tunables {
            let engine = wasmer::Store::default().engine().clone();
            wasmer::Store::new_with_tunables(engine, tunables)
        } else {
            wasmer::Store::default()
        }
    }
}

#[cfg(feature = "compiler")]
pub mod engine {
    #[derive(Clone, Debug)]
    pub struct EngineCompiler {
        engine: wasmer::Engine,
    }

    impl EngineCompiler {
        pub fn new(engine: wasmer::Engine) -> Self {
            Self { engine }
        }
    }

    impl super::Compiler for EngineCompiler {
        fn engine(&self) -> Option<wasmer::Engine> {
            Some(self.engine.clone())
        }

        fn new_store(&self, tunables: Option<super::ArcTunables>) -> wasmer::Store {
            if let Some(tunables) = tunables {
                wasmer::Store::new_with_tunables(self.engine.clone(), tunables)
            } else {
                wasmer::Store::new(self.engine.clone())
            }
        }
    }
}
