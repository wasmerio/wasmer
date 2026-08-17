use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use wasmer_compiler::types::{function::FunctionBody, unwind::CompiledFunctionUnwindInfo};
use wasmer_types::CompileError;

use crate::config::output_size_limit_error;

const LOCAL_BUDGET_CHUNK_SIZE: usize = 1024;

/// Bytes stored after an already accounted function body for Windows unwind information.
pub(crate) fn windows_unwind_output_delta(body_len: usize, unwind_len: usize) -> usize {
    let padding = (4 - body_len % 4) % 4;
    padding.saturating_add(unwind_len)
}

/// Machine code allocation represented by a compiled function body.
pub(crate) fn function_output_size(body: &FunctionBody) -> usize {
    let body_len = body.body.len();
    let extra = match body.unwind_info.as_ref() {
        Some(CompiledFunctionUnwindInfo::WindowsX64(unwind)) => {
            windows_unwind_output_delta(body_len, unwind.len())
        }
        _ => 0,
    };
    body_len.saturating_add(extra)
}

/// Shared emitted code allowance for one module compilation.
#[derive(Debug)]
pub(crate) struct OutputBudget {
    limit: usize,
    total: AtomicUsize,
}

impl OutputBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            total: AtomicUsize::new(0),
        }
    }

    pub(crate) fn reserve(&self, delta: usize) -> Result<(), CompileError> {
        if delta == 0 {
            return Ok(());
        }

        match self
            .total
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                total
                    .checked_add(delta)
                    .filter(|&new_total| new_total <= self.limit)
            }) {
            Ok(_) => Ok(()),
            Err(total) => Err(output_size_limit_error(
                total.saturating_add(delta),
                self.limit,
            )),
        }
    }

    pub(crate) fn ensure_within_limit(&self) -> Result<(), CompileError> {
        let total = self.total.load(Ordering::Relaxed);
        if total > self.limit {
            return Err(output_size_limit_error(total, self.limit));
        }
        Ok(())
    }

    #[cfg(test)]
    fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
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
        let pending = self
            .pending
            .checked_add(delta)
            .ok_or_else(|| output_size_limit_error(usize::MAX, self.shared.limit))?;
        let committed = pending / LOCAL_BUDGET_CHUNK_SIZE * LOCAL_BUDGET_CHUNK_SIZE;
        if committed > 0 {
            self.shared.reserve(committed)?;
        }
        self.pending = pending - committed;
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<(), CompileError> {
        self.shared.reserve(self.pending)?;
        self.pending = 0;
        Ok(())
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
    fn function_output_includes_aligned_windows_unwind_info() {
        let body = FunctionBody {
            body: vec![0; 5],
            unwind_info: Some(CompiledFunctionUnwindInfo::WindowsX64(vec![0; 7])),
        };
        assert_eq!(function_output_size(&body), 15);

        let body = FunctionBody {
            body: vec![0; 5],
            unwind_info: Some(CompiledFunctionUnwindInfo::Dwarf),
        };
        assert_eq!(function_output_size(&body), 5);
    }

    #[test]
    fn reservation_limit_is_inclusive() {
        const LIMIT: usize = 10 * 1024 * 1024;
        let budget = OutputBudget::new(LIMIT);
        assert!(budget.reserve(LIMIT).is_ok());
        assert_eq!(budget.total(), LIMIT);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == format!(
                    "singlepass compiler output exceeds limit: {} > {LIMIT} bytes",
                    LIMIT + 1
                )
        ));
        assert_eq!(budget.total(), LIMIT);
    }

    #[test]
    fn local_reservations_commit_full_chunks_and_flush_the_remainder() {
        let budget = Arc::new(OutputBudget::new(10_000));
        let mut local = LocalOutputBudget::new(Arc::clone(&budget));
        local.reserve(1).unwrap();
        assert_eq!(budget.total(), 0);

        local.reserve(LOCAL_BUDGET_CHUNK_SIZE - 1).unwrap();
        assert_eq!(budget.total(), LOCAL_BUDGET_CHUNK_SIZE);

        local.reserve(1).unwrap();
        assert_eq!(budget.total(), LOCAL_BUDGET_CHUNK_SIZE);

        local.finish().unwrap();
        assert_eq!(budget.total(), LOCAL_BUDGET_CHUNK_SIZE + 1);
        assert!(budget.ensure_within_limit().is_ok());
    }

    #[test]
    fn local_remainder_is_checked_at_function_completion() {
        const LIMIT: usize = 1_500;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let mut local = LocalOutputBudget::new(Arc::clone(&budget));
        local.reserve(LOCAL_BUDGET_CHUNK_SIZE).unwrap();
        local.reserve(LIMIT - LOCAL_BUDGET_CHUNK_SIZE).unwrap();
        local.finish().unwrap();
        assert_eq!(budget.total(), LIMIT);

        let mut overflow = LocalOutputBudget::new(Arc::clone(&budget));
        overflow.reserve(1).unwrap();
        assert!(matches!(
            overflow.finish(),
            Err(CompileError::Codegen(message))
                if message == "singlepass compiler output exceeds limit: 1501 > 1500 bytes"
        ));
        assert_eq!(budget.total(), LIMIT);
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

        assert_eq!(budget.total(), THREADS * BYTES_PER_THREAD);
        assert!(budget.ensure_within_limit().is_ok());
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
        assert_eq!(budget.total(), LIMIT);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == "singlepass compiler output exceeds limit: 10001 > 10000 bytes"
        ));
    }
}
