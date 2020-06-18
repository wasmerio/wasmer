// Allow unused imports while developing
#![allow(unused_imports, dead_code)]

use crate::compiler::SinglepassCompiler;
use std::sync::Arc;
use wasmer_compiler::{
    Compiler, CompilerConfig, CpuFeature, FunctionMiddlewareGenerator, Target,
};

#[derive(Clone)]
pub struct SinglepassConfig {
    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    pub enable_nan_canonicalization: bool,

    /// Enable stack check.
    ///
    /// When enabled, an explicit stack depth check will be performed on entry
    /// to each function to prevent stack overflow.
    ///
    /// Note that this doesn't guarantee deterministic execution across
    /// different platforms.
    pub enable_stack_check: bool,

    target: Target,

    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn FunctionMiddlewareGenerator>>,
}

impl SinglepassConfig {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new(target: Target) -> Self {
        Self {
            enable_nan_canonicalization: true,
            enable_stack_check: false,
            target,
            middlewares: vec![],
        }
    }
}

impl CompilerConfig for SinglepassConfig {
    fn enable_pic(&mut self) {
        // Do nothing, since singlepass already emits
        // PIC code.
    }

    /// Gets the target that we will use for compiling
    /// the WebAssembly module
    fn target(&self) -> &Target {
        &self.target
    }

    /// Transform it into the compiler
    fn compiler(&self) -> Box<dyn Compiler + Send> {
        Box::new(SinglepassCompiler::new(&self))
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn FunctionMiddlewareGenerator>) {
        self.middlewares.push(middleware);
    }
}

impl Default for SinglepassConfig {
    fn default() -> SinglepassConfig {
        Self::new(Default::default(), Default::default())
    }
}
