//! Immediate results and borrowed, deliberately non-Send syscall continuations.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::LocalFuture;

/// A syscall result that is already available, or work that may suspend.
///
/// Constructing `Ready` never constructs or allocates a deferred syscall future.
/// Deferred bodies borrow their caller and memory and need not be `Send`.
#[non_exhaustive]
#[must_use = "syscall continuations must be awaited or explicitly dropped"]
pub enum MaybeFuture<'a, T> {
    Ready(T),
    Future(LocalFuture<'a, T>),
    #[doc(hidden)]
    Completed,
}

// Only the boxed continuation is structurally pinned. Moving an unpolled result
// cannot move its continuation's allocation or expose a pinned reference to T.
impl<T> Unpin for MaybeFuture<'_, T> {}

impl<'a, T: 'a> MaybeFuture<'a, T> {
    pub fn map<U: 'a>(self, map: impl FnOnce(T) -> U + 'a) -> MaybeFuture<'a, U> {
        match self {
            Self::Ready(value) => MaybeFuture::Ready(map(value)),
            Self::Future(future) => MaybeFuture::Future(Box::pin(async move { map(future.await) })),
            Self::Completed => panic!("completed syscall result"),
        }
    }
}

impl<T> Future for MaybeFuture<'_, T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let this = self.get_mut();
        match this {
            Self::Ready(_) => {
                let Self::Ready(value) = std::mem::replace(this, Self::Completed) else {
                    unreachable!()
                };
                Poll::Ready(value)
            }
            Self::Future(future) => match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    *this = Self::Completed;
                    Poll::Ready(value)
                }
                Poll::Pending => Poll::Pending,
            },
            Self::Completed => panic!("syscall result polled after completion"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn ready_maps_without_building_a_continuation() {
        let called = Cell::new(false);
        let result = MaybeFuture::Ready(7).map(|value| {
            called.set(true);
            value + 1
        });
        assert!(called.get());
        assert!(matches!(result, MaybeFuture::Ready(8)));
    }

    #[test]
    fn deferred_non_send_work_is_lazy_and_cancelled_by_drop() {
        struct OnDrop(Rc<Cell<bool>>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let dropped = Rc::new(Cell::new(false));
        let guard = OnDrop(dropped.clone());
        let polled = Rc::new(Cell::new(false));
        let marker = polled.clone();
        let mut future = MaybeFuture::Future(Box::pin(async move {
            let _guard = guard;
            marker.set(true);
            std::future::pending::<()>().await;
            1
        }))
        .map(|value| value + 1);
        assert!(!polled.get());
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Pending);
        assert!(polled.get());
        drop(future);
        assert!(dropped.get());
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::{
        Caller, Fault, SysResult, Syscall, SyscallRet, Syscalls, TaskId, UserMemory, UserSliceMut,
        dispatch,
    };
    use std::cell::Cell;

    struct NoMemory;
    impl UserMemory for NoMemory {
        fn read<'a>(&'a self, _: u32, _: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async { Err(Fault) })
        }
        fn write<'a>(&'a self, _: u32, _: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async { Err(Fault) })
        }
        fn size(&self) -> LocalFuture<'_, u64> {
            Box::pin(async { 0 })
        }
        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
    }

    struct LargeRead {
        read_constructed: Cell<bool>,
    }
    impl Syscalls for LargeRead {
        fn read<'a>(
            &'a self,
            _: &'a Caller<'_>,
            _: crate::Fd,
            _: UserSliceMut,
        ) -> MaybeFuture<'a, SysResult<u32>> {
            self.read_constructed.set(true);
            MaybeFuture::Future(Box::pin(async {
                let bytes = [0u8; 65536];
                std::future::pending::<()>().await;
                Ok(std::hint::black_box(bytes).len() as u32)
            }))
        }
        fn getpid<'a>(&'a self, _: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
            MaybeFuture::Ready(Ok(7))
        }
    }

    #[test]
    fn ready_dispatch_never_constructs_an_unrelated_large_read() {
        let syscalls = LargeRead {
            read_constructed: Cell::new(false),
        };
        let caller = Caller::new(TaskId(1), &NoMemory);
        let mut future = dispatch(&syscalls, &caller, Syscall::Getpid);
        let size = std::mem::size_of_val(&future);
        eprintln!("ready getpid dispatch state: {size} bytes");
        assert!(size < 1024);
        assert!(!syscalls.read_constructed.get());
        assert!(matches!(
            future,
            MaybeFuture::Ready(Ok(SyscallRet::Getpid(7)))
        ));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert_eq!(
            Pin::new(&mut future).poll(&mut cx),
            Poll::Ready(Ok(SyscallRet::Getpid(7)))
        );
        assert!(!syscalls.read_constructed.get());
    }

    struct AsyncGetpid;
    impl Syscalls for AsyncGetpid {
        fn getpid<'a>(&'a self, _: &'a Caller<'_>) -> MaybeFuture<'a, SysResult<i32>> {
            MaybeFuture::Future(Box::pin(std::future::pending()))
        }
    }

    #[test]
    fn custom_getpid_still_suspends() {
        let caller = Caller::new(TaskId(1), &NoMemory);
        let mut future = dispatch(&AsyncGetpid, &caller, Syscall::Getpid);
        let size = std::mem::size_of_val(&future);
        eprintln!("pending getpid dispatch state: {size} bytes");
        assert!(size < 1024);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Pending);
    }
}
