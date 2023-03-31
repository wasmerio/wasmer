use core::ops::DerefMut;
use std::sync::Mutex;

use std::sync::Arc;

use wasmer_types::CompileError;
use wasmer_types::SerializableModule;
use wasmer_types::Target;

use crate::Artifact;
use crate::ArtifactBuild;
use crate::Engine;

/// Represents the result of the compilation
pub enum CompilationResult {
    /// Waiting for the background process to finish compiling the module
    Waiting,
    /// The module is ready to be turned into an artifact
    Ready {
        /// The result of the compilation in the background
        compilation: Result<SerializableModule, CompileError>,
        /// The target this compilation was done again
        target: Target,
    },
    /// The artifact is ready for cloning
    Artifact(Arc<Artifact>),
}

/// Represents the next tier artifact in a tiered compilation chain
#[derive(Clone)]
pub struct NextArtifact {
    /// Location where the next tier will be placed once it has loaded
    next: Arc<Mutex<CompilationResult>>,
}

impl NextArtifact {
    /// Create a object that will hold the next artifact in a compile chain
    pub fn new() -> Self {
        Self {
            next: Arc::new(Mutex::new(CompilationResult::Waiting)),
        }
    }
    /// Sets the next artifact which will be picked up
    /// by anyone using the module at an appropriate time
    pub fn set(&self, result: CompilationResult) {
        let mut guard = self.next.lock().unwrap();
        *guard = result;
    }

    /// Called by the `Module::try_upgrade` call when
    /// it tries to upgrade itself to the next tier
    pub fn get(&self, engine: &Engine) -> Option<Arc<Artifact>> {
        let mut guard = self.next.lock().unwrap();

        let (module, target) = match guard.deref_mut() {
            CompilationResult::Waiting => {
                return None;
            }
            CompilationResult::Ready {
                compilation: compilation_ref,
                target,
            } => {
                let mut compilation = Err(CompileError::Codegen(
                    "compilation already consumed".to_string(),
                ));
                std::mem::swap(compilation_ref, &mut compilation);
                (compilation, target.clone())
            }
            CompilationResult::Artifact(ret) => return Some(ret.clone()),
        };

        let module = match module {
            Ok(m) => m,
            Err(_err) => {
                //TODO: Logging should be better
                //tracing::warn!("failed to upgrade module - {}", err);
                return None;
            }
        };

        let artifact = ArtifactBuild {
            serializable: module,
            next_tier: None,
        };

        let mut engine_inner = engine.inner_mut();
        let res = Artifact::from_parts(&mut engine_inner, artifact, &target);
        match res {
            Ok(a) => {
                let ret = Arc::new(a);
                *guard = CompilationResult::Artifact(ret.clone());
                return Some(ret);
            }
            Err(_err) => {
                //TODO: Logging should be better
                //tracing::warn!("failed to upgrade module - {}", err);
                None
            }
        }
    }
}
