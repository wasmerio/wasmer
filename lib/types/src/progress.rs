//! Types used to report and handle compilation progress.

use std::{borrow::Cow, fmt, num::NonZeroUsize, string::String, sync::Arc};
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
type ReserveSizeCallbackFn = dyn Fn(usize) -> Result<(), UserAbort> + Send + Sync + 'static;

#[derive(Clone)]
struct ReserveSizeCallback {
    callback: Arc<ReserveSizeCallbackFn>,
    chunk_size: NonZeroUsize,
}

/// Wraps callbacks that can receive compilation progress and output-size notifications.
#[derive(Clone)]
pub struct CompilationProgressCallback {
    callback: Arc<ProgressCallback>,
    reserve_size_callback: Option<ReserveSizeCallback>,
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
            reserve_size_callback: None,
        }
    }

    /// Configures a callback for reporting increases in compilation output size.
    ///
    /// Singlepass uses this callback to account for emitted native-code bytes, reporting after
    /// at least `chunk_size` bytes.
    pub fn with_reserve_size_callback<F>(mut self, callback: F, chunk_size: NonZeroUsize) -> Self
    where
        F: Fn(usize) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        self.reserve_size_callback = Some(ReserveSizeCallback {
            callback: Arc::new(callback),
            chunk_size,
        });
        self
    }

    /// Returns the configured reserve-size reporting chunk size.
    pub fn reserve_size_chunk_size(&self) -> Option<NonZeroUsize> {
        self.reserve_size_callback
            .as_ref()
            .map(|callback| callback.chunk_size)
    }

    /// Reports an increase in compilation output size, in bytes.
    ///
    /// This is a no-op when no reserve-size callback was configured.
    pub fn reserve_size(&self, size_increase: usize) -> Result<(), UserAbort> {
        match &self.reserve_size_callback {
            Some(callback) => (callback.callback)(size_increase),
            None => Ok(()),
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
