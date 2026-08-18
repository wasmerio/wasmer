use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use wasmer_types::CompileError;

use crate::config::output_size_limit_error;

const LOCAL_BUDGET_CHUNK_SIZE: usize = 1024;

/// Shared emitted code allowance for one module compilation.
#[derive(Debug)]
pub(crate) struct OutputBudget {
    limit: usize,
    remaining: AtomicUsize,
}

impl OutputBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            remaining: AtomicUsize::new(limit),
        }
    }

    pub(crate) fn reserve(&self, delta: usize) -> Result<(), CompileError> {
        if delta == 0 {
            return Ok(());
        }

        match self
            .remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(delta)
            }) {
            Ok(_) => Ok(()),
            Err(_) => Err(output_size_limit_error(self.limit)),
        }
    }

    #[cfg(test)]
    fn remaining(&self) -> usize {
        self.remaining.load(Ordering::Relaxed)
    }
}

fn reserve_batched(
    shared: &OutputBudget,
    pending: &mut usize,
    delta: usize,
) -> Result<(), CompileError> {
    let new_pending = pending
        .checked_add(delta)
        .ok_or_else(|| output_size_limit_error(shared.limit))?;
    let committed = new_pending / LOCAL_BUDGET_CHUNK_SIZE * LOCAL_BUDGET_CHUNK_SIZE;
    if committed > 0 {
        shared.reserve(committed)?;
    }
    *pending = new_pending - committed;
    Ok(())
}

/// Function local output accounting that commits emitted bytes to the shared
/// module budget in chunks
#[derive(Debug)]
pub(crate) struct LocalOutputBudget {
    shared: Arc<OutputBudget>,
    pending: usize,
}

impl LocalOutputBudget {
    pub(crate) fn new(shared: Arc<OutputBudget>) -> Self {
        Self { shared, pending: 0 }
    }

    pub(crate) fn reserve(&mut self, delta: usize) -> Result<(), CompileError> {
        reserve_batched(&self.shared, &mut self.pending, delta)
    }

    pub(crate) fn finish(&mut self) -> Result<(), CompileError> {
        let pending = std::mem::take(&mut self.pending);
        self.shared.reserve(pending)
    }
}

/// Batched accounting for an assembler whose current output offset can be
/// sampled while it emits a trampoline.
pub(crate) struct EmittedOutputBudget<'a> {
    shared: Option<&'a OutputBudget>,
    accounted: usize,
    pending: usize,
}

impl<'a> EmittedOutputBudget<'a> {
    pub(crate) fn new(shared: Option<&'a OutputBudget>) -> Self {
        Self {
            shared,
            accounted: 0,
            pending: 0,
        }
    }

    pub(crate) fn check(&mut self, output_size: usize) -> Result<(), CompileError> {
        debug_assert!(output_size >= self.accounted);
        if let Some(shared) = self.shared {
            reserve_batched(
                shared,
                &mut self.pending,
                output_size.saturating_sub(self.accounted),
            )?;
        }
        self.accounted = output_size;
        Ok(())
    }

    pub(crate) fn finish(&mut self, output_size: usize) -> Result<(), CompileError> {
        self.check(output_size)?;
        if let Some(shared) = self.shared {
            let pending = std::mem::take(&mut self.pending);
            shared.reserve(pending)?;
        }
        Ok(())
    }
}

impl Drop for LocalOutputBudget {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.pending, 0,
            "local output budget dropped without calling finish"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn reservation_limit_is_inclusive() {
        const LIMIT: usize = 10 * 1024 * 1024;
        let budget = OutputBudget::new(LIMIT);
        assert!(budget.reserve(LIMIT).is_ok());
        assert_eq!(budget.remaining(), 0);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == format!(
                    "singlepass compiler output exceeds limit of {LIMIT} bytes"
                )
        ));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn local_reservations_commit_full_chunks_and_flush_the_remainder() {
        let budget = Arc::new(OutputBudget::new(10_000));
        let mut local = LocalOutputBudget::new(Arc::clone(&budget));
        local.reserve(1).unwrap();
        assert_eq!(budget.remaining(), 10_000);

        local.reserve(LOCAL_BUDGET_CHUNK_SIZE - 1).unwrap();
        assert_eq!(budget.remaining(), 10_000 - LOCAL_BUDGET_CHUNK_SIZE);

        local.reserve(1).unwrap();
        assert_eq!(budget.remaining(), 10_000 - LOCAL_BUDGET_CHUNK_SIZE);

        local.finish().unwrap();
        assert_eq!(budget.remaining(), 10_000 - LOCAL_BUDGET_CHUNK_SIZE - 1);
    }

    #[test]
    fn emitted_output_checks_commit_chunks_and_finish_flushes_the_remainder() {
        let budget = OutputBudget::new(10_000);
        let mut emitted = EmittedOutputBudget::new(Some(&budget));

        emitted.check(1).unwrap();
        assert_eq!(budget.remaining(), 10_000);

        emitted.check(LOCAL_BUDGET_CHUNK_SIZE).unwrap();
        assert_eq!(budget.remaining(), 10_000 - LOCAL_BUDGET_CHUNK_SIZE);

        emitted.finish(LOCAL_BUDGET_CHUNK_SIZE + 1).unwrap();
        assert_eq!(budget.remaining(), 10_000 - LOCAL_BUDGET_CHUNK_SIZE - 1);
    }

    #[test]
    fn emitted_output_check_stops_when_a_full_chunk_exceeds_the_limit() {
        let budget = OutputBudget::new(LOCAL_BUDGET_CHUNK_SIZE - 1);
        let mut emitted = EmittedOutputBudget::new(Some(&budget));

        assert!(matches!(
            emitted.check(LOCAL_BUDGET_CHUNK_SIZE),
            Err(CompileError::Codegen(message))
                if message == format!(
                    "singlepass compiler output exceeds limit of {} bytes",
                    LOCAL_BUDGET_CHUNK_SIZE - 1
                )
        ));
        assert_eq!(budget.remaining(), LOCAL_BUDGET_CHUNK_SIZE - 1);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "local output budget dropped without calling finish")]
    fn dropping_unfinished_local_budget_panics() {
        let budget = Arc::new(OutputBudget::new(10_000));
        let mut local = LocalOutputBudget::new(budget);
        local.reserve(1).unwrap();
    }

    #[test]
    fn local_remainder_is_checked_at_function_completion() {
        const LIMIT: usize = 1_500;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let mut local = LocalOutputBudget::new(Arc::clone(&budget));
        local.reserve(LOCAL_BUDGET_CHUNK_SIZE).unwrap();
        local.reserve(LIMIT - LOCAL_BUDGET_CHUNK_SIZE).unwrap();
        local.finish().unwrap();
        assert_eq!(budget.remaining(), 0);

        let mut overflow = LocalOutputBudget::new(Arc::clone(&budget));
        overflow.reserve(1).unwrap();
        assert!(matches!(
            overflow.finish(),
            Err(CompileError::Codegen(message))
                if message == "singlepass compiler output exceeds limit of 1500 bytes"
        ));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn parallel_local_reservations_preserve_the_exact_total() {
        const THREADS: usize = 8;
        const BYTES_PER_THREAD: usize = 1_000;
        let budget = Arc::new(OutputBudget::new(THREADS * BYTES_PER_THREAD));

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let budget = Arc::clone(&budget);
                scope.spawn(move || {
                    let mut local = LocalOutputBudget::new(budget);
                    for _ in 0..BYTES_PER_THREAD {
                        local.reserve(1).unwrap();
                    }
                    local.finish().unwrap();
                });
            }
        });

        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn parallel_reservations_cannot_exceed_limit() {
        const LIMIT: usize = 10_000;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let successful = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let budget = Arc::clone(&budget);
                let successful = Arc::clone(&successful);
                scope.spawn(move || {
                    for _ in 0..2_000 {
                        if budget.reserve(1).is_ok() {
                            successful.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        assert_eq!(successful.load(Ordering::Relaxed), LIMIT);
        assert_eq!(budget.remaining(), 0);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == "singlepass compiler output exceeds limit of 10000 bytes"
        ));
    }
}
