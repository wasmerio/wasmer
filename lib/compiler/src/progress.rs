//! Shared helpers for reporting compilation progress across the different backends.

use crate::lib::std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use wasmer_types::{CompilationProgress, CompilationProgressCallback, CompileError};

/// Tracks progress within a compilation phase and forwards updates to a callback.
///
/// Convenience wrapper around a [`CompilationProgressCallback`] for the compilers.
#[derive(Clone)]
pub struct ProgressContext {
    callback: CompilationProgressCallback,
    counter: Arc<AtomicU64>,
    total: u64,
    phase_name: &'static str,
}

impl ProgressContext {
    /// Creates a new [`ProgressContext`] for the given phase.
    pub fn new(
        callback: CompilationProgressCallback,
        total: u64,
        phase_name: &'static str,
    ) -> Self {
        Self {
            callback,
            counter: Arc::new(AtomicU64::new(0)),
            total,
            phase_name,
        }
    }

    /// Notifies the callback that the next step in the phase has completed.
    pub fn notify(&self) -> Result<(), CompileError> {
        self.notify_steps(1)
    }

    /// Notifies the callback that the next N steps in the phase are completed.
    pub fn notify_steps(&self, steps: u64) -> Result<(), CompileError> {
        let step = self.counter.fetch_add(steps, Ordering::SeqCst) + steps;
        self.callback
            .notify(CompilationProgress::new(
                Some(Cow::Borrowed(self.phase_name)),
                Some(self.total),
                Some(step),
            ))
            .map_err(CompileError::from)
    }
}
