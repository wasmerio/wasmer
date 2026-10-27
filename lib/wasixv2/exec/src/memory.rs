use wasixv2_syscalls::{Fault, LocalFuture, UserMemory};
use wasmer::AsyncFunctionEnvMut;

use crate::{
    ExecEnv,
    asyncify::{Access, Outside},
};

/// The calling instance's linear memory, reached through its async function environment.
pub struct GuestMemory<'a, S: 'static> {
    pub(crate) env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
}

impl<'a, S: 'static> GuestMemory<'a, S> {
    pub fn new(env: &'a AsyncFunctionEnvMut<ExecEnv<S>>) -> Self {
        Self { env }
    }
}

impl<S: 'static> UserMemory for GuestMemory<'_, S> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let memory = data.memory.as_ref().ok_or(Fault)?;
            memory
                .view(&*store)
                .read(addr.into(), buf)
                .map_err(|_| Fault)
        })
    }

    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let memory = data.memory.as_ref().ok_or(Fault)?;
            memory
                .view(&*store)
                .write(addr.into(), bytes)
                .map_err(|_| Fault)
        })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            data.memory
                .as_ref()
                .map_or(0, |memory| memory.view(&*store).data_size())
        })
    }

    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(crate::asyncify::whole_stack(self.env))
    }

    fn stack_pointer(&self) -> LocalFuture<'_, u32> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, mut store) = env.data_and_store_mut();
            let global = data.stack_pointer.as_ref();
            global.map_or(0, |global| global.get(&mut store).i32().unwrap_or(0) as u32)
        })
    }

    /// An atomic compare-and-exchange on the memory itself, so guest threads running at the same
    /// time on other host threads see the futex word change as one step.
    #[cfg(feature = "sys")]
    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        use std::sync::atomic::{AtomicU32, Ordering};
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let memory = data.memory.as_ref().ok_or(Fault)?;
            let view = memory.view(&*store);
            if !addr.is_multiple_of(4) || u64::from(addr) + 4 > view.data_size() {
                return Err(Fault);
            }
            // SAFETY: the word is in bounds and aligned, and linear memory stays mapped while
            // the store is borrowed; guest threads only race with this through atomics.
            let word = unsafe { AtomicU32::from_ptr(view.data_ptr().add(addr as usize).cast()) };
            let current = word.compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst);
            Ok(current.unwrap_or_else(|actual| actual))
        })
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let previous = data.memory.as_ref()?.grow(store, pages).ok()?;
            Some(previous.bytes().0 as u64)
        })
    }
}

/// The linear memory of an instance seen from outside any guest call, for the calls the
/// execution layer makes on the guest's behalf, such as the exit when an entry point returns.
pub(crate) struct OutsideMemory<'a, 'b, S: 'static> {
    pub(crate) cx: &'a Outside<'b, S>,
}

impl<S: Send + Sync + 'static> UserMemory for OutsideMemory<'_, '_, S> {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(self.cx.with(move |data, store| {
            let memory = data.memory.as_ref().ok_or(Fault)?;
            memory
                .view(&*store)
                .read(addr.into(), buf)
                .map_err(|_| Fault)
        }))
    }

    fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(self.cx.with(move |data, store| {
            let memory = data.memory.as_ref().ok_or(Fault)?;
            memory
                .view(&*store)
                .write(addr.into(), bytes)
                .map_err(|_| Fault)
        }))
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(self.cx.with(|data, store| {
            data.memory
                .as_ref()
                .map_or(0, |memory| memory.view(&*store).data_size())
        }))
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(self.cx.with(move |data, store| {
            let previous = data.memory.as_ref()?.grow(store, pages).ok()?;
            Some(previous.bytes().0 as u64)
        }))
    }
}
