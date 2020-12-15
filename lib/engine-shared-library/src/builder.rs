use crate::SharedLibraryEngine;
use wasmer_compiler::{CompilerConfig, Features, Target};

/// The Shared Library builder
pub struct SharedLibrary {
    compiler_config: Option<Box<dyn CompilerConfig>>,
    target: Option<Target>,
    features: Option<Features>,
}

impl SharedLibrary {
    #[cfg(feature = "compiler")]
    /// Create a new SharedLibrary
    pub fn new<T>(compiler_config: T) -> Self
    where
        T: Into<Box<dyn CompilerConfig>>,
    {
        let mut compiler_config = compiler_config.into();
        compiler_config.enable_pic();

        Self {
            compiler_config: Some(compiler_config),
            target: None,
            features: None,
        }
    }

    /// Create a new headless SharedLibrary
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

    /// Build the `SharedLibraryEngine` for this configuration
    pub fn engine(self) -> SharedLibraryEngine {
        if let Some(_compiler_config) = self.compiler_config {
            #[cfg(feature = "compiler")]
            {
                let compiler_config = _compiler_config;
                let target = self.target.unwrap_or_default();
                let features = self
                    .features
                    .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
                let compiler = compiler_config.compiler();
                SharedLibraryEngine::new(compiler, target, features)
            }

            #[cfg(not(feature = "compiler"))]
            {
                unreachable!(
                    "Cannot call `SharedLibraryEngine::new` without the `compiler` feature"
                )
            }
        } else {
            SharedLibraryEngine::headless()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "compiler")]
    use std::sync::Arc;
    #[cfg(feature = "compiler")]
    use wasmer_compiler::{Compiler, FunctionMiddlewareGenerator};

    #[cfg(feature = "compiler")]
    #[derive(Default)]
    pub struct TestCompilerConfig {
        pub enabled_pic: bool,
        pub middlewares: Vec<Arc<dyn FunctionMiddlewareGenerator>>,
    }

    #[cfg(feature = "compiler")]
    impl CompilerConfig for TestCompilerConfig {
        fn enable_pic(&mut self) {
            self.enabled_pic = true;
        }

        fn compiler(&self) -> Box<dyn Compiler> {
            unimplemented!("compiler not implemented");
        }

        fn push_middleware(&mut self, middleware: Arc<dyn FunctionMiddlewareGenerator>) {
            self.middlewares.push(middleware);
        }
    }

    #[cfg(feature = "compiler")]
    #[test]
    #[should_panic(expected = "compiler not implemented")]
    fn build_engine() {
        let mut compiler_config = TestCompilerConfig::default();
        let shared_library = SharedLibrary::new(compiler_config);
        let _engine = shared_library.engine();
    }

    #[test]
    fn build_headless_engine() {
        let shared_library = SharedLibrary::headless();
        let _engine = shared_library.engine();
    }
}
