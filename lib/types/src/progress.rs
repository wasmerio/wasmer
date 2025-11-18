//! Types used to report and handle compilation progress.

use crate::lib::std::{borrow::Cow, fmt, string::String, sync::Arc};

/// Indicates the current compilation progress.
///
/// All fields are kept private for forwards compatibility and future extension.
/// Use the provided methods to access progress data.
#[derive(Clone, Debug, Default)]
pub struct CompilationProgress {
    phase_name: Option<Cow<'static, str>>,
    phase_step_count: Option<u64>,
    phase_step: Option<u64>,
}

impl CompilationProgress {
    /// Creates a new [`CompilationProgress`].
    pub fn new(
        phase_name: Option<Cow<'static, str>>,
        phase_step_count: Option<u64>,
        phase_step: Option<u64>,
    ) -> Self {
        Self {
            phase_name,
            phase_step_count,
            phase_step,
        }
    }

    /// Returns the name of the phase currently being executed.
    pub fn phase_name(&self) -> Option<&str> {
        self.phase_name.as_deref()
    }

    /// Returns the total number of steps in the current phase, if known.
    pub fn phase_step_count(&self) -> Option<u64> {
        self.phase_step_count
    }

    /// Returns the index of the current step within the phase, if known.
    pub fn phase_step(&self) -> Option<u64> {
        self.phase_step
    }
}

/// Error returned when the user requests to abort an expensive computation.
#[derive(Clone, Debug)]
pub struct UserAbort {
    reason: String,
}

impl UserAbort {
    /// Creates a new [`UserAbort`].
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// Returns the configured reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for UserAbort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(f)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for UserAbort {}

/// Wraps a boxed callback that can receive compilation progress notifications.
#[derive(Clone)]
pub struct CompilationProgressCallback {
    callback: Arc<dyn Fn(CompilationProgress) -> Result<(), UserAbort> + Send + Sync + 'static>,
}

impl CompilationProgressCallback {
    /// Create a new callback wrapper.
    ///
    /// The provided callback will be invoked with progress updates during the compilation process,
    /// and has to return a `Result<(), UserAbort>`.
    ///
    /// If the callback returns an error, the compilation will be aborted with a `CompileError::Aborted`.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(CompilationProgress) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Notify the callback about new progress information.
    pub fn notify(&self, progress: CompilationProgress) -> Result<(), UserAbort> {
        (self.callback)(progress)
    }
}

impl fmt::Debug for CompilationProgressCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompilationProgressCallback").finish()
    }
}
