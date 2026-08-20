use std::borrow::Borrow;

use wasmer_types::{CompilationProgressCallback, CompileError};

/// Shared emitted-code reporter for one module compilation.
#[derive(Clone, Debug)]
pub(crate) struct OutputReporter {
    callback: CompilationProgressCallback,
    chunk_size: usize,
}

impl OutputReporter {
    pub(crate) fn new(callback: CompilationProgressCallback, chunk_size: usize) -> Self {
        debug_assert!(chunk_size > 0);
        Self {
            callback,
            chunk_size,
        }
    }

    #[inline]
    pub(crate) fn reserve(&self, amount: usize) -> Result<(), CompileError> {
        if amount == 0 {
            return Ok(());
        }
        self.callback.reserve(amount).map_err(CompileError::from)
    }
}

/// Local emitted-code accounting that reports bytes in configured chunks.
#[derive(Debug)]
pub(crate) struct LocalOutputReporter<T> {
    pub(super) shared: Option<T>,
    accounted: usize,
    pending: usize,
}

impl<T: Borrow<OutputReporter>> LocalOutputReporter<T> {
    pub(crate) fn new(shared: Option<T>) -> Self {
        Self {
            shared,
            accounted: 0,
            pending: 0,
        }
    }

    #[inline]
    fn reserve(&mut self, amount: usize) -> Result<(), CompileError> {
        let Some(shared) = self.shared.as_ref().map(Borrow::borrow) else {
            return Ok(());
        };
        let Some(new_pending) = self.pending.checked_add(amount) else {
            self.pending = 0;
            return Err(CompileError::Codegen(
                "singlepass output byte accounting overflowed".to_string(),
            ));
        };
        let committed = new_pending / shared.chunk_size * shared.chunk_size;
        if committed > 0
            && let Err(error) = shared.reserve(committed)
        {
            self.pending = 0;
            return Err(error);
        }
        self.pending = new_pending - committed;
        Ok(())
    }

    #[inline]
    pub(crate) fn check(&mut self, output_size: usize) -> Result<(), CompileError> {
        debug_assert!(output_size >= self.accounted);
        self.reserve(output_size.saturating_sub(self.accounted))?;
        self.accounted = output_size;
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<(), CompileError> {
        let pending = std::mem::take(&mut self.pending);
        if let Some(shared) = self.shared.as_ref().map(Borrow::borrow) {
            shared.reserve(pending)?;
        }
        Ok(())
    }
}

impl<T> Drop for LocalOutputReporter<T> {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.pending, 0,
            "local output reporter dropped without calling finish"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use wasmer_types::CompilationProgress;

    use super::*;

    fn reporter(chunk_size: usize, reserved: Arc<AtomicUsize>) -> Arc<OutputReporter> {
        let callback = CompilationProgressCallback::new(|_: CompilationProgress| Ok(()))
            .with_reserve_callback(move |amount| {
                reserved.fetch_add(amount, Ordering::Relaxed);
                Ok(())
            });
        Arc::new(OutputReporter::new(callback, chunk_size))
    }

    #[test]
    fn local_reservations_report_full_chunks_and_flush_the_remainder() {
        let reserved = Arc::new(AtomicUsize::new(0));
        let reporter = reporter(1024, Arc::clone(&reserved));
        let mut local = LocalOutputReporter::new(Some(reporter));

        local.reserve(1).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), 0);
        local.reserve(1023).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), 1024);
        local.reserve(1).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), 1024);
        local.finish().unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), 1025);
    }

    #[test]
    fn reservation_error_aborts_reporting() {
        let callback = CompilationProgressCallback::new(|_: CompilationProgress| Ok(()))
            .with_reserve_callback(|_| Err(wasmer_types::UserAbort::new("output limit")));
        let reporter = OutputReporter::new(callback, 1);
        let mut local = LocalOutputReporter::new(Some(&reporter));

        assert!(matches!(
            local.reserve(1),
            Err(CompileError::Aborted(error)) if error.reason() == "output limit"
        ));
    }

    #[test]
    fn parallel_local_reservations_report_the_exact_total() {
        const THREADS: usize = 8;
        const BYTES_PER_THREAD: usize = 1_000;
        let reserved = Arc::new(AtomicUsize::new(0));
        let reporter = reporter(128, Arc::clone(&reserved));

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let reporter = Arc::clone(&reporter);
                scope.spawn(move || {
                    let mut local = LocalOutputReporter::new(Some(reporter));
                    for _ in 0..BYTES_PER_THREAD {
                        local.reserve(1).unwrap();
                    }
                    local.finish().unwrap();
                });
            }
        });

        assert_eq!(reserved.load(Ordering::Relaxed), THREADS * BYTES_PER_THREAD);
    }
}
