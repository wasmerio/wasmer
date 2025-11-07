use crate::vmcontext::{VMFunctionContext, VMTrampoline};
use crate::{
    InternalStoreHandle, StoreObjects, Trap, VMContext,  VMFunctionBody
};
use backtrace::Backtrace;
use core::ptr::{read, read_unaligned};
use corosensei::stack::DefaultStack;
use corosensei::trap::{CoroutineTrapHandler, TrapHandlerRegs};
use corosensei::{Coroutine, CoroutineResult, Yielder};
use scopeguard::defer;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::mem;
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull, null_mut};
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering, compiler_fence};
use std::sync::{LazyLock, Once, RwLock};
use wasmer_types::TrapCode;

#[derive(Default)]
pub struct StackPool {
    pool: RwLock<BTreeMap<usize, crossbeam_queue::SegQueue<DefaultStack>>>,
}

impl StackPool {
    pub fn new() -> Self {
        Self {
            pool: Default::default(),
        }
    }
    /// Get a stack from the stack pool or create a new one if none is available
    ///
    /// Tries to keep the stack size as close as possible to the requested size
    pub fn get_stack(&self, stack_size: usize) -> DefaultStack {
        {
            let pool_guard = self.pool.read().unwrap();
            if let Some(existing_queue) = pool_guard.get(&stack_size) {
                return existing_queue
                    .pop()
                    .unwrap_or_else(|| DefaultStack::new(stack_size).unwrap());
            };
        }
        DefaultStack::new(stack_size).unwrap()
    }
    /// Return a stack to the pool
    ///
    /// The stack size needs to be provided, because we can not get the stack size from the stack itself
    pub fn return_stack(&self, stack: DefaultStack, stack_size: usize) {
        {
            let pool_guard = self.pool.read().unwrap();
            if let Some(existing_queue) = pool_guard.get(&stack_size) {
                existing_queue.push(stack);
                return;
            };
        }
        let mut pool_guard = self.pool.write().unwrap();
        let new_queue = crossbeam_queue::SegQueue::new();
        new_queue.push(stack);
        pool_guard.insert(stack_size, new_queue);
    }
}