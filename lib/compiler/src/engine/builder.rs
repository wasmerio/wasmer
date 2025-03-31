use super::Engine;
#[cfg(feature = "compiler")]
use crate::CompilerConfig;
use wasmer_types::{target::Target, Features, HashAlgorithm};

/// The Builder contents of `Engine`
pub struct EngineBuilder {
    /// The compiler
    #[cfg(feature = "compiler")]
    compiler_config: Option<Box<dyn CompilerConfig>>,
    /// The machine target
    target: Option<Target>,
    /// The features to compile the Wasm module with
    features: Option<Features>,
    /// The hashing algorithm
    hash_algorithm: Option<HashAlgorithm>,
}

impl EngineBuilder {
    /// Create a new builder with pre-made components
    #[cfg(feature = "compiler")]
    pub fn new<T>(compiler_config: T) -> Self
    where
        T: Into<Box<dyn CompilerConfig>>,
    {
        Self {
            compiler_config: Some(compiler_config.into()),
            target: None,
            features: None,
            hash_algorithm: None,
        }
    }

    /// Create a new headless Backend
    pub fn headless() -> Self {
        Self {
            #[cfg(feature = "compiler")]
            compiler_config: None,
            target: None,
            features: None,
            hash_algorithm: None,
        }
    }

    /// Set the target
    pub fn set_target(mut self, target: Option<Target>) -> Self {
        self.target = target;
        self
    }

    /// Set the features
    pub fn set_features(mut self, features: Option<Features>) -> Self {
        self.features = features;
        self
    }

    /// Set the hashing algorithm
    pub fn set_hash_algorithm(mut self, hash_algorithm: Option<HashAlgorithm>) -> Self {
        self.hash_algorithm = hash_algorithm;
        self
    }

    /// Build the `Engine` for this configuration
    #[cfg(feature = "compiler")]
    pub fn engine(self) -> Engine {
        let target = self.target.unwrap_or_default();
        if let Some(compiler_config) = self.compiler_config {
            let features = self
                .features
                .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
            let mut engine = Engine::new(compiler_config, target, features);

            engine.set_hash_algorithm(self.hash_algorithm);

            engine
        } else {
            Engine::headless()
        }
    }

    /// Build the `Engine` for this configuration
    #[cfg(not(feature = "compiler"))]
    pub fn engine(self) -> Engine {
        Engine::headless()
    }

    /// The Wasm features
    pub fn features(&self) -> Option<&Features> {
        self.features.as_ref()
    }

    /// The target
    pub fn target(&self) -> Option<&Target> {
        self.target.as_ref()
    }
}
