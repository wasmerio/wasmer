/// Abstracts the Webassembly compiler.
// NOTE: currently only a stub, will be expanded with actual compilation capability in the future.
pub trait Compiler: std::fmt::Debug {
    fn new_store(&self) -> wasmer::Store;
}

pub type DynCompiler = std::sync::Arc<dyn Compiler + Send + Sync + 'static>;

#[derive(Clone, Debug)]
pub struct StubCompiler;

impl Compiler for StubCompiler {
    fn new_store(&self) -> wasmer::Store {
        wasmer::Store::default()
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
        fn new_store(&self) -> wasmer::Store {
            wasmer::Store::new(self.engine.clone())
        }
    }
}
