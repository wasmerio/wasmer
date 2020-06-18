use crate::JITEngine;
use std::sync::Arc;
use wasmer_compiler::{Compiler, CompilerConfig, Features, Target};

/// The JIT builder
pub struct JIT<'a> {
    compiler_config: Option<&'a dyn CompilerConfig>,
    target: Option<Target>,
    features: Option<Features>,
}

impl<'a> JIT<'a> {
    /// Create a new JIT
    pub fn new(compiler_config: &'a dyn CompilerConfig) -> Self {
        Self {
            compiler_config: Some(compiler_config),
            target: None,
            features: None,
        }
    }

    /// Create a new headless JIT
    pub fn headless() -> Self {
        Self {
            compiler_config: None,
            target: None,
            features: None,
        }
    }

    /// Set the target
    pub fn target(mut self, target: Target) -> Self {
        self.target = Some(target);
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
        if let Some(compiler_config) = self.compiler_config {
            let features = self
                .features
                .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
            let compiler = compiler_config.compiler();
            JITEngine::new(compiler, target, features)
        } else {
            JITEngine::headless()
        }
    }
}
