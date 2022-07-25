use super::Engine;
use crate::{CompilerConfig, Features};
use wasmer_types::Target;

/// The Backend builder
pub struct Backend {
    #[allow(dead_code)]
    compiler_config: Option<Box<dyn CompilerConfig>>,
    target: Option<Target>,
    features: Option<Features>,
}

impl Backend {
    /// Create a new Backend
    pub fn new<T>(compiler_config: T) -> Self
    where
        T: Into<Box<dyn CompilerConfig>>,
    {
        Self {
            compiler_config: Some(compiler_config.into()),
            target: None,
            features: None,
        }
    }

    /// Create a new headless Backend
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

    /// Build the `Engine` for this configuration
    #[cfg(feature = "engine_compilation")]
    pub fn engine(self) -> Engine {
        let target = self.target.unwrap_or_default();
        if let Some(compiler_config) = self.compiler_config {
            let features = self
                .features
                .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
            let compiler = compiler_config.compiler();
            Engine::new(compiler, target, features)
        } else {
            Engine::headless()
        }
    }

    /// Build the `Engine` for this configuration
    #[cfg(not(feature = "engine_compilation"))]
    pub fn engine(self) -> Engine {
        Engine::headless()
    }
}
