use wasixv2_syscalls::{Fault, LocalFuture, UserMemory};
use wasmer::AsyncFunctionEnvMut;

use crate::ExecEnv;

/// The calling instance's linear memory, reached through its async function environment.
pub(crate) struct GuestMemory<'a, S: 'static> {
    pub(crate) env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
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

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async move {
            let mut env = self.env.write().await;
            let (data, store) = env.data_and_store_mut();
            let previous = data.memory.as_ref()?.grow(store, pages).ok()?;
            Some(previous.bytes().0 as u64)
        })
    }
}
