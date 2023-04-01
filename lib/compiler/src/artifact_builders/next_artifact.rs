use core::ops::DerefMut;
use std::sync::Mutex;

use std::sync::Arc;

use wasmer_types::CompileError;
use wasmer_types::SerializableModule;
use wasmer_types::Target;

use crate::Artifact;
use crate::ArtifactBuild;
use crate::EngineInner;

/// Represents the result of the compilation
pub enum CompilationResult {
    /// Nothing is being built and has ever been built
    Nothing,
    /// Initialized but not yet running
    Initialized {
        /// This function will spawn the compilation step
        spawn: Box<dyn FnOnce() + Send + Sync + 'static>,
    },
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

/// Trait used to kick of the process the will build
/// the next artifact (or it will immediately return
/// the already serialized module)
pub trait NextArtifactBuilder
where
    Self: Sized,
{
    fn start(self) -> Option<NextArtifact>;
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
            next: Arc::new(Mutex::new(CompilationResult::Nothing)),
        }
    }

    /// Creates the next artifact from an existing compilation
    pub fn new_existing(compilation: SerializableModule, target: Target) -> Self {
        Self {
            next: Arc::new(Mutex::new(CompilationResult::Ready {
                compilation: Ok(compilation),
                target,
            })),
        }
    }

    /// Sets the next artifact which will be picked up
    /// by anyone using the module at an appropriate time
    pub fn set(&self, result: CompilationResult) {
        let mut guard = self.next.lock().unwrap();
        *guard = result;
    }

    /// Called before compiling early stages in a compilation
    /// chain for a fast path (early exit of the chain)
    pub fn peek(&self) -> Option<Result<SerializableModule, CompileError>> {
        let mut guard = self.next.lock().unwrap();
        match guard.deref_mut() {
            CompilationResult::Ready {
                compilation: compilation_ref,
                ..
            } if compilation_ref.is_ok() => {
                let mut compilation = Err(CompileError::Codegen(
                    "compilation already consumed via peek".to_string(),
                ));
                std::mem::swap(compilation_ref, &mut compilation);
                return Some(compilation);
            }
            _ => return None,
        }
    }

    /// Called by the `Module::try_upgrade` call when
    /// it tries to upgrade itself to the next tier
    pub fn get(&self, engine_inner: &mut EngineInner) -> Option<Arc<Artifact>> {
        let mut guard = self.next.lock().unwrap();

        let (module, target) = match guard.deref_mut() {
            CompilationResult::Nothing => {
                return None;
            }
            CompilationResult::Initialized { .. } => {
                let mut init = CompilationResult::Waiting;
                std::mem::swap(guard.deref_mut(), &mut init);
                if let CompilationResult::Initialized { spawn } = init {
                    spawn();
                }
                return None;
            }
            CompilationResult::Waiting => {
                return None;
            }
            CompilationResult::Ready {
                compilation: compilation_ref,
                target,
            } => {
                if let Err(err) = compilation_ref {
                    (Err(err.clone()), target.clone())
                } else {
                    let mut compilation = Err(CompileError::Codegen(
                        "compilation already consumed via get".to_string(),
                    ));
                    std::mem::swap(compilation_ref, &mut compilation);
                    (compilation, target.clone())
                }
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

        let res = Artifact::from_parts(engine_inner, artifact, &target);
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
