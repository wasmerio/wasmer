use std::num::NonZeroUsize;

use wasmer_types::{CompilationProgressCallback, CompileError};

/// Compilation progress reporting binary size in defined chunks.
#[derive(Debug)]
pub(crate) struct ChunkedOutputReporter<'a> {
    progress_callback: Option<&'a CompilationProgressCallback>,
    chunk_size: NonZeroUsize,
    // Number of bytes already reported to the progress callback.
    accounted: usize,
    // Currently allocated bytes.
    current: usize,
}

impl<'a> ChunkedOutputReporter<'a> {
    pub(crate) fn new(progress_callback: Option<&'a CompilationProgressCallback>) -> Self {
        let Some((progress_callback, chunk_size)) = progress_callback.and_then(|cb| {
            cb.reserve_size_chunk_size()
                .map(|chunk_size| (cb, chunk_size))
        }) else {
            return Self {
                progress_callback: None,
                chunk_size: NonZeroUsize::MAX,
                accounted: 0,
                current: 0,
            };
        };

        Self {
            progress_callback: Some(progress_callback),
            chunk_size,
            accounted: 0,
            current: 0,
        }
    }

    #[inline]
    pub(crate) fn check(&mut self, output_size: usize) -> Result<(), CompileError> {
        let Some(progress_callback) = self.progress_callback.as_ref() else {
            return Ok(());
        };

        debug_assert!(output_size >= self.current);
        self.current = output_size;
        let pending = self.current - self.accounted;

        if pending >= self.chunk_size.get() {
            // report the entire difference
            self.accounted = self.current;
            progress_callback.reserve_size(pending)?;
        }

        Ok(())
    }

    pub(crate) fn finish(mut self, output_size: usize) -> Result<(), CompileError> {
        let Some(progress_callback) = self.progress_callback.as_ref() else {
            return Ok(());
        };

        debug_assert!(output_size >= self.current);
        self.current = output_size;
        let pending = self.current - self.accounted;
        self.accounted = self.current;

        Ok(progress_callback.reserve_size(pending)?)
    }
}

impl<'a> Drop for ChunkedOutputReporter<'a> {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.current, self.accounted,
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

    use wasmer_types::{CompilationProgress, UserAbort};

    use super::*;

    const CHUNK_SIZE: usize = 1024;

    struct OutputBudget {
        limit: usize,
        remaining: AtomicUsize,
    }

    impl OutputBudget {
        fn new(limit: usize) -> Self {
            Self {
                limit,
                remaining: AtomicUsize::new(limit),
            }
        }

        fn reserve(&self, amount: usize) -> Result<(), UserAbort> {
            self.remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(amount)
                })
                .map(|_| ())
                .map_err(|_| {
                    UserAbort::new(format!(
                        "singlepass compiler output exceeds limit of {} bytes",
                        self.limit
                    ))
                })
        }

        fn remaining(&self) -> usize {
            self.remaining.load(Ordering::Relaxed)
        }
    }

    fn callback<F>(chunk_size: usize, reserve: F) -> CompilationProgressCallback
    where
        F: Fn(usize) -> Result<(), UserAbort> + Send + Sync + 'static,
    {
        CompilationProgressCallback::new(|_: CompilationProgress| Ok(()))
            .with_reserve_size_callback(reserve, NonZeroUsize::new(chunk_size).unwrap())
    }

    fn budget_callback(
        budget: Arc<OutputBudget>,
        chunk_size: usize,
    ) -> CompilationProgressCallback {
        callback(chunk_size, move |amount| budget.reserve(amount))
    }

    #[test]
    fn reservation_limit_is_inclusive() {
        const LIMIT: usize = 10 * 1024 * 1024;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let progress_callback = budget_callback(Arc::clone(&budget), CHUNK_SIZE);

        ChunkedOutputReporter::new(Some(&progress_callback))
            .finish(LIMIT)
            .unwrap();
        assert_eq!(budget.remaining(), 0);

        assert!(matches!(
            ChunkedOutputReporter::new(Some(&progress_callback)).finish(1),
            Err(CompileError::Aborted(error))
                if error.reason() == format!(
                    "singlepass compiler output exceeds limit of {LIMIT} bytes"
                )
        ));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn local_reports_commit_full_chunks_and_flush_the_remainder() {
        let reserved = Arc::new(AtomicUsize::new(0));
        let progress_callback = callback(CHUNK_SIZE, {
            let reserved = Arc::clone(&reserved);
            move |amount| {
                reserved.fetch_add(amount, Ordering::Relaxed);
                Ok(())
            }
        });
        let mut reporter = ChunkedOutputReporter::new(Some(&progress_callback));

        reporter.check(1).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), 0);

        reporter.check(CHUNK_SIZE).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), CHUNK_SIZE);

        reporter.check(CHUNK_SIZE + 1).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), CHUNK_SIZE);

        reporter.finish(CHUNK_SIZE + 1).unwrap();
        assert_eq!(reserved.load(Ordering::Relaxed), CHUNK_SIZE + 1);
    }

    #[test]
    fn output_checks_commit_chunks_and_finish_flushes_the_remainder() {
        let budget = Arc::new(OutputBudget::new(10_000));
        let progress_callback = budget_callback(Arc::clone(&budget), CHUNK_SIZE);
        let mut reporter = ChunkedOutputReporter::new(Some(&progress_callback));

        reporter.check(1).unwrap();
        assert_eq!(budget.remaining(), 10_000);

        reporter.check(CHUNK_SIZE).unwrap();
        assert_eq!(budget.remaining(), 10_000 - CHUNK_SIZE);

        reporter.check(CHUNK_SIZE + 1).unwrap();
        reporter.finish(CHUNK_SIZE + 1).unwrap();
        assert_eq!(budget.remaining(), 10_000 - CHUNK_SIZE - 1);
    }

    #[test]
    fn output_check_stops_when_a_full_chunk_exceeds_the_limit() {
        let budget = Arc::new(OutputBudget::new(CHUNK_SIZE - 1));
        let progress_callback = budget_callback(Arc::clone(&budget), CHUNK_SIZE);
        let mut reporter = ChunkedOutputReporter::new(Some(&progress_callback));

        assert!(matches!(
            reporter.check(CHUNK_SIZE),
            Err(CompileError::Aborted(error))
                if error.reason() == format!(
                    "singlepass compiler output exceeds limit of {} bytes",
                    CHUNK_SIZE - 1
                )
        ));
        assert_eq!(budget.remaining(), CHUNK_SIZE - 1);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "local output reporter dropped without calling finish")]
    fn dropping_unfinished_local_reporter_panics() {
        let progress_callback = callback(CHUNK_SIZE, |_| Ok(()));
        let mut reporter = ChunkedOutputReporter::new(Some(&progress_callback));
        reporter.check(1).unwrap();
    }

    #[test]
    fn local_remainder_is_checked_at_function_completion() {
        const LIMIT: usize = 1_500;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let progress_callback = budget_callback(Arc::clone(&budget), CHUNK_SIZE);
        let mut reporter = ChunkedOutputReporter::new(Some(&progress_callback));

        reporter.check(CHUNK_SIZE).unwrap();
        reporter.check(LIMIT).unwrap();
        reporter.finish(LIMIT).unwrap();
        assert_eq!(budget.remaining(), 0);

        let overflow = ChunkedOutputReporter::new(Some(&progress_callback));
        assert!(matches!(
            overflow.finish(1),
            Err(CompileError::Aborted(error))
                if error.reason()
                    == "singlepass compiler output exceeds limit of 1500 bytes"
        ));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn parallel_local_reports_preserve_the_exact_total() {
        const THREADS: usize = 8;
        const BYTES_PER_THREAD: usize = 1_000;
        let budget = Arc::new(OutputBudget::new(THREADS * BYTES_PER_THREAD));
        let progress_callback = budget_callback(Arc::clone(&budget), CHUNK_SIZE);

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let progress_callback = &progress_callback;
                scope.spawn(move || {
                    let mut reporter = ChunkedOutputReporter::new(Some(progress_callback));
                    for output_size in 1..=BYTES_PER_THREAD {
                        reporter.check(output_size).unwrap();
                    }
                    reporter.finish(BYTES_PER_THREAD).unwrap();
                });
            }
        });

        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn parallel_reports_cannot_exceed_limit() {
        const LIMIT: usize = 10_000;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let successful = Arc::new(AtomicUsize::new(0));
        let progress_callback = budget_callback(Arc::clone(&budget), 1);

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let progress_callback = &progress_callback;
                let successful = Arc::clone(&successful);
                scope.spawn(move || {
                    let mut reporter = ChunkedOutputReporter::new(Some(progress_callback));
                    for output_size in 1..=2_000 {
                        if reporter.check(output_size).is_ok() {
                            successful.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    reporter.finish(2_000).unwrap();
                });
            }
        });

        assert_eq!(successful.load(Ordering::Relaxed), LIMIT);
        assert_eq!(budget.remaining(), 0);
        assert!(matches!(
            ChunkedOutputReporter::new(Some(&progress_callback)).finish(1),
            Err(CompileError::Aborted(error))
                if error.reason() == "singlepass compiler output exceeds limit of 10000 bytes"
        ));
    }
}
