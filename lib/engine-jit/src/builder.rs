use crate::JITEngine;
use std::sync::Arc;
use wasmer_compiler::{Compiler, CompilerConfig, Features, Target};
use wasmer_engine::Tunables;

/// The JIT builder
pub struct JIT<'a> {
    compiler_config: Option<&'a dyn CompilerConfig>,
    tunables_fn: Option<Box<Fn(&Target) -> Box<dyn Tunables + Send + Sync>>>,
    target: Option<Target>,
    features: Option<Features>,
}

impl<'a> JIT<'a> {
    /// Create a new JIT
    pub fn new(compiler_config: &'a dyn CompilerConfig) -> Self {
        Self {
            compiler_config: Some(compiler_config),
            target: None,
            tunables_fn: None,
            features: None,
        }
    }

    /// Create a new headless JIT
    pub fn headless() -> Self {
        Self {
            compiler_config: None,
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
    pub fn tunables<F, T>(mut self, tunables_fn: F) -> Self
    where
        F: Fn(&Target) -> T + 'static,
        T: Tunables + Send + Sync + 'static
    {
        self.tunables_fn = Some(Box::new(move |target: &Target| {
            Box::new(tunables_fn(target))
        }));
        self
    }

    /// Set the features
    pub fn features(mut self, features: Features) -> Self {
        self.features = Some(features);
        self
    }

    /// Build the `JITEngine` for this configuration
    pub fn engine(self) -> JITEngine {
        let target = self.target.unwrap_or_default();
        let tunables_fn = self
            .tunables_fn
            .expect("You need to specify tunables for the JIT");
        let tunables: Arc<dyn Tunables + Send + Sync> = tunables_fn(&target).into();
        if let Some(compiler_config) = self.compiler_config {
            let features = self
                .features
                .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
            let compiler = compiler_config.compiler();
            JITEngine::new(compiler, target, tunables, features)
        } else {
            JITEngine::headless(tunables)
        }
    }
}
