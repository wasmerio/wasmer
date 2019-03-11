//! This module represents the runtime state of a module.

use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::mem;
use wasmer_runtime_core::{structures::BoxedMap, types::LocalFuncIndex};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OptState {
    Cranelift,
    Ascending,
    LLVM,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ShouldAscend {
    Yes,
    No,
}

pub struct FuncState {
    opt: Mutex<OptState>,
}

impl FuncState {
    /// Supplies the current optimization state to the closure and transitions to the next
    /// state depending on the return value.
    pub fn next_state(&self, chooser: impl FnOnce(OptState) -> ShouldAscend) {
        let mut state = self.opt.lock();
        let current_state = *state;
        let should_ascend = chooser(current_state);

        match should_ascend {
            ShouldAscend::No => return,
            ShouldAscend::Yes => {}
        }

        let next_state = match &*state {
            OptState::Cranelift => OptState::Ascending,
            OptState::Ascending => OptState::LLVM,
            OptState::LLVM => OptState::LLVM,
        };

        mem::replace(&mut *state, next_state);
    }
}

pub struct ModuleState {
    functions: BoxedMap<LocalFuncIndex, FuncState>,
    /// We want each module to have its own thread pool
    /// for compiling functions, since we don't want compiling
    /// one module to block any other modules.
    thread_pool: ThreadPool,
}

impl ModuleState {
    pub fn new(functions: BoxedMap<LocalFuncIndex, FuncState>) -> Self {
        Self {
            functions,
            thread_pool: ThreadPoolBuilder::new().build().unwrap(),
        }
    }
}
