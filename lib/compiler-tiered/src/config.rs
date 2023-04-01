// Allow unused imports while developing
#![allow(unused_imports, dead_code)]

use wasmer_compiler::{
    Compiler, CompilerConfig, Engine, EngineBuilder, Features, ModuleMiddleware,
};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_compiler_singlepass::Singlepass;
use wasmer_types::Target;

use crate::{compiler::TieredCompiler, DefaultTieredCaching, TieredCaching};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Tiered {
    pub(crate) singlepass: Box<Singlepass>,
    pub(crate) cranelift: Box<Cranelift>,
    pub(crate) middlewares: Vec<Arc<dyn ModuleMiddleware>>,
    pub(crate) caching: Arc<dyn TieredCaching>,
}

impl Tiered {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            singlepass: Box::new(Singlepass::new()),
            cranelift: Box::new(Cranelift::new()),
            middlewares: Default::default(),
            caching: Arc::new(DefaultTieredCaching::default()),
        }
    }

    /// Attaches a caching implementation to the tiered compiler
    pub fn with_caching<T>(&mut self, caching: T) -> &mut Self
    where
        T: TieredCaching + 'static,
    {
        self.caching = Arc::new(caching);
        self
    }
}

impl CompilerConfig for Tiered {
    fn enable_pic(&mut self) {
        self.singlepass.enable_pic();
        self.cranelift.enable_pic();
    }

    fn compiler(self: Box<Self>) -> Box<dyn Compiler> {
        Box::new(TieredCompiler::new(*self))
    }

    fn default_features_for_target(&self, target: &Target) -> Features {
        let ret = self.singlepass.default_features_for_target(target);
        ret.and(self.cranelift.default_features_for_target(target))
    }

    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>) {
        self.singlepass.push_middleware(middleware.clone());
        self.cranelift.push_middleware(middleware.clone());
        self.middlewares.push(middleware);
    }
}

impl Default for Tiered {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Tiered> for Engine {
    fn from(config: Tiered) -> Self {
        EngineBuilder::new(config).engine()
    }
}
