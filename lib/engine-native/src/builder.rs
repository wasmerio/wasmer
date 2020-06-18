use crate::NativeEngine;
use std::sync::Arc;
use wasmer_compiler::{Compiler, CompilerConfig, Features, Target};
use wasmer_engine::Tunables;

/// The Native builder
pub struct Native {
    compiler: Option<Box<dyn Compiler + Send>>,
    tunables_fn: Option<Box<Fn(&Target) -> Box<dyn Tunables + Send + Sync>>>,
    target: Option<Target>,
    features: Option<Features>,
}

impl Native {
    /// Create a new Native
    pub fn new(compiler_config: &mut dyn CompilerConfig) -> Self {
        compiler_config.enable_pic();
        Self {
            compiler: Some(compiler_config.compiler()),
            target: None,
            tunables_fn: None,
            features: None,
        }
    }

    /// Create a new headless Native
    pub fn headless() -> Self {
        Self {
            compiler: None,
            target: None,
            tunables_fn: None,
            features: None,
        }
    }

    /// Set the target
    pub fn target(mut self, target: Target) -> Self {
        self.target = Some(target);
        self
    }

    /// Set the tunables constructor function.
    ///
    /// It should receive a [`Target`] and return a
    pub fn tunables<F>(mut self, tunables_fn: F) -> Self
    where
        F: Fn(&Target) -> Box<dyn Tunables + Send + Sync> + 'static,
    {
        self.tunables_fn = Some(Box::new(tunables_fn));
        self
    }

    /// Set the features
    pub fn features(mut self, features: Features) -> Self {
        self.features = Some(features);
        self
    }

    /// Build the `NativeEngine` for this configuration
    pub fn engine(self) -> NativeEngine {
        let target = self.target.unwrap_or_default();
        let tunables_fn = self
            .tunables_fn
            .expect("You need to specify tunables for the JIT");
        let tunables: Arc<dyn Tunables + Send + Sync> = tunables_fn(&target).into();
        if let Some(compiler) = self.compiler {
            let features = self.features.unwrap_or_default();
            NativeEngine::new(compiler, target, tunables, features)
        } else {
            NativeEngine::headless(tunables)
        }
    }
}
