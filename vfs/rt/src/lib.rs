use std::any::Any;
use std::future::Future;
use std::pin::Pin;

use vfs_core::provider::VfsRuntime;

pub struct InlineTestRuntime;

impl VfsRuntime for InlineTestRuntime {
    fn spawn_blocking_boxed(
        &self,
        f: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
        Box::pin(async move {
            let handle = std::thread::spawn(f);
            match handle.join() {
                Ok(value) => value,
                Err(err) => std::panic::resume_unwind(err),
            }
        })
    }

    fn block_on_boxed<'a>(
        &'a self,
        fut: Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'a>>,
    ) -> Box<dyn Any + Send> {
        futures::executor::block_on(fut)
    }
}

#[cfg(feature = "tokio")]
pub struct TokioRuntime {
    handle: tokio::runtime::Handle,
}

#[cfg(feature = "tokio")]
impl TokioRuntime {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }
}

#[cfg(feature = "tokio")]
impl VfsRuntime for TokioRuntime {
    fn spawn_blocking_boxed(
        &self,
        f: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
        let handle = self.handle.clone();
        Box::pin(async move {
            let join = handle.spawn_blocking(move || f());
            match join.await {
                Ok(value) => value,
                Err(err) => std::panic::resume_unwind(err.into_panic()),
            }
        })
    }

    fn block_on_boxed<'a>(
        &'a self,
        fut: Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'a>>,
    ) -> Box<dyn Any + Send> {
        self.handle.block_on(fut)
    }
}
