//! Types used to report and handle compilation progress.

use std::{borrow::Cow, fmt, string::String, sync::Arc};
use thiserror::Error;

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
#[derive(Clone, Debug, Error)]
#[error("{reason}")]
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

type ProgressCallback =
    dyn Fn(CompilationProgress) -> Result<(), UserAbort> + Send + Sync + 'static;
type ReserveCallback = dyn Fn(usize) -> Result<(), UserAbort> + Send + Sync + 'static;

/// Wraps callbacks that can receive compilation progress and reservation notifications.
#[derive(Clone)]
pub struct CompilationProgressCallback {
    callback: Arc<ProgressCallback>,
    reserve_callback: Option<Arc<ReserveCallback>>,
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
            reserve_callback: None,
        }
    }

    /// Configures a callback for reserving resources during compilation.
    ///
    /// Singlepass uses this callback to account for emitted native-code bytes.
    /// Calls may be made concurrently when compilation uses multiple threads.
    pub fn with_reserve_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        self.reserve_callback = Some(Arc::new(callback));
        self
    }

    /// Reserves the requested amount through the configured reservation callback.
    ///
    /// This is a no-op when no reservation callback was configured.
    pub fn reserve(&self, amount: usize) -> Result<(), UserAbort> {
        match &self.reserve_callback {
            Some(callback) => callback(amount),
            None => Ok(()),
        }
    }

    /// Returns whether a reservation callback is configured.
    pub fn has_reserve_callback(&self) -> bool {
        self.reserve_callback.is_some()
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
