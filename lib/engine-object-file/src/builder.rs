use crate::ObjectFileEngine;
use wasmer_compiler::{CompilerConfig, Features, Target};

/// The ObjectFile builder
pub struct ObjectFile<'a> {
    compiler_config: Option<&'a dyn CompilerConfig>,
    target: Option<Target>,
    features: Option<Features>,
}

impl<'a> ObjectFile<'a> {
    #[cfg(feature = "compiler")]
    /// Create a new ObjectFile
    pub fn new(compiler_config: &'a mut dyn CompilerConfig) -> Self {
        compiler_config.enable_pic();

        Self {
            compiler_config: Some(compiler_config),
            target: None,
            features: None,
        }
    }

    /// Create a new headless ObjectFile
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

    /// Build the `ObjectFileEngine` for this configuration
    pub fn engine(self) -> ObjectFileEngine {
        if let Some(_compiler_config) = self.compiler_config {
            #[cfg(feature = "compiler")]
            {
                let compiler_config = _compiler_config;
                let target = self.target.unwrap_or_default();
                let features = self
                    .features
                    .unwrap_or_else(|| compiler_config.default_features_for_target(&target));
                let compiler = compiler_config.compiler();
                ObjectFileEngine::new(compiler, target, features)
            }

            #[cfg(not(feature = "compiler"))]
            {
                unreachable!("Cannot call `ObjectFileEngine::new` without the `compiler` feature")
            }
        } else {
            ObjectFileEngine::headless()
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

        fn compiler(&self) -> Box<dyn Compiler + Send> {
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
        let object_file = ObjectFile::new(&mut compiler_config);
        let _engine = object_file.engine();
    }

    #[test]
    fn build_headless_engine() {
        let object_file = ObjectFile::headless();
        let _engine = object_file.engine();
    }
}
