use std::sync::Arc;
use wasmer::sys::Features;
use wasmer::{CompilerConfig, ModuleMiddleware, Store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compiler {
    LLVM,
    Cranelift,
    Singlepass,
}

#[derive(Clone)]
pub struct Config {
    pub compiler: Compiler,
    pub features: Option<Features>,
    pub middlewares: Vec<Arc<dyn ModuleMiddleware>>,
    pub canonicalize_nans: bool,
}

impl Config {
    pub fn new(compiler: Compiler) -> Self {
        Self {
            compiler,
            features: None,
            canonicalize_nans: false,
            middlewares: vec![],
        }
    }

    pub fn set_middlewares(&mut self, middlewares: Vec<Arc<dyn ModuleMiddleware>>) {
        self.middlewares = middlewares;
    }

    pub fn set_features(&mut self, features: Features) {
        self.features = Some(features);
    }

    pub fn set_nan_canonicalization(&mut self, canonicalize_nans: bool) {
        self.canonicalize_nans = canonicalize_nans;
    }

    pub fn store(&self) -> Store {
        let compiler_config = self.compiler_config(self.canonicalize_nans);
        let engine = self.engine(compiler_config);
        Store::new(engine)
    }

    pub fn headless_store(&self) -> Store {
        let engine = self.engine_headless();
        Store::new(engine)
    }

    pub fn engine(&self, compiler_config: Box<dyn CompilerConfig>) -> wasmer::Engine {
        let mut engine = wasmer::sys::EngineBuilder::new(compiler_config);
        if let Some(ref features) = self.features {
            engine = engine.set_features(Some(features.clone()));
        }
        engine.engine().into()
    }

    pub fn engine_headless(&self) -> wasmer::Engine {
        wasmer::sys::EngineBuilder::headless().engine().into()
    }

    pub fn compiler_config(
        &self,
        #[allow(unused_variables)] canonicalize_nans: bool,
    ) -> Box<dyn CompilerConfig> {
        match &self.compiler {
            #[cfg(feature = "cranelift")]
            Compiler::Cranelift => {
                let mut compiler = wasmer_compiler_cranelift::Cranelift::new();
                compiler.canonicalize_nans(canonicalize_nans);
                compiler.enable_verifier();
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[cfg(feature = "llvm")]
            Compiler::LLVM => {
                let mut compiler = wasmer_compiler_llvm::LLVM::new();
                compiler.canonicalize_nans(canonicalize_nans);
                compiler.enable_verifier();
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[cfg(feature = "singlepass")]
            Compiler::Singlepass => {
                let mut compiler = wasmer_compiler_singlepass::Singlepass::new();
                compiler.canonicalize_nans(canonicalize_nans);
                compiler.enable_verifier();
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[allow(unreachable_patterns)]
            compiler => {
                panic!(
                    "The {:?} Compiler is not enabled. Enable it via the features",
                    compiler
                )
            }
        }
    }

    #[allow(dead_code)]
    fn add_middlewares(&self, config: &mut dyn CompilerConfig) {
        for middleware in self.middlewares.iter() {
            config.push_middleware(middleware.clone());
        }
    }
}
