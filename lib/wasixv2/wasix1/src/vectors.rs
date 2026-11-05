//! Width-aware decoding of the real guest vector array, with lazy nested validation.

use crate::{
    abi::{AbiWidth, Errno, GuestSize, Pointer},
    errno::V1Result,
    shim::Frame,
};
use std::cell::Cell;
use wasixv2_abi::{ConstIovecArray, Fd};
use wasixv2_syscalls::{
    Control, IoVectorSource, LocalFuture, RawGuestBuffer, SysResult, Syscalls, UserMemory,
    VectorOperation,
};

pub(crate) struct Vectors<'a> {
    memory: &'a dyn UserMemory,
    operation: VectorOperation,
    fd: Fd,
    array: u32,
    entries: Vec<RawGuestBuffer>,
    failure: Cell<Option<Errno>>,
}

impl<'a> Vectors<'a> {
    pub(crate) async fn read<S: Syscalls + Control + 'static>(
        frame: &'a Frame<'_, S>,
        operation: VectorOperation,
        fd: Fd,
        pointer: Pointer,
        count: GuestSize,
    ) -> V1Result<Self> {
        let stride = match frame.width() {
            AbiWidth::Wasm32 => 8,
            AbiWidth::Wasm64 => 16,
        };
        let bytes = count.0.checked_mul(stride).ok_or(Errno::OVERFLOW)?;
        // The array itself is checked now; nested ranges are checked only as entered.
        let end = pointer.0.checked_add(bytes).ok_or(Errno::OVERFLOW)?;
        let memory = frame.mem.guest();
        if end > memory.size().await {
            return Err(Errno::MEMVIOLATION.into());
        }
        let array = pointer.narrow()?;
        let count = u32::try_from(count.0).map_err(|_| Errno::OVERFLOW)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count as usize)
            .map_err(|_| Errno::NOMEM)?;
        let mut data = [0; 16];
        for index in 0..count {
            let address = pointer.0 + u64::from(index) * stride;
            let address = u32::try_from(address).map_err(|_| Errno::MEMVIOLATION)?;
            memory
                .read(address, &mut data[..stride as usize])
                .await
                .map_err(|_| Errno::MEMVIOLATION)?;
            let entry = match frame.width() {
                AbiWidth::Wasm32 => RawGuestBuffer::new(
                    u64::from(u32::from_le_bytes(data[..4].try_into().unwrap())),
                    u64::from(u32::from_le_bytes(data[4..8].try_into().unwrap())),
                ),
                AbiWidth::Wasm64 => RawGuestBuffer::new(
                    u64::from_le_bytes(data[..8].try_into().unwrap()),
                    u64::from_le_bytes(data[8..16].try_into().unwrap()),
                ),
            };
            entries.push(entry);
        }
        Ok(Self {
            memory,
            operation,
            fd,
            array,
            entries,
            failure: Cell::new(None),
        })
    }

    pub(crate) fn array(&self) -> ConstIovecArray {
        ConstIovecArray::new(self.array)
    }

    /// Restore the exact legacy validation verdict carried through ordinary dispatch.
    pub(crate) fn finish<T>(&self, result: SysResult<T>) -> V1Result<T> {
        let failure = self.failure.take();
        match result {
            Err(wasixv2_syscalls::SysErr::Errno(wasixv2_syscalls::Errno::EFAULT))
                if failure.is_some() =>
            {
                Err(failure.unwrap().into())
            }
            result => result.map_err(Into::into),
        }
    }
}

impl IoVectorSource for Vectors<'_> {
    fn matches(&self, operation: VectorOperation, fd: Fd, array: u32, count: u32) -> bool {
        self.operation == operation
            && self.fd == fd
            && self.array == array
            && self.entries.len() == count as usize
    }
    fn count(&self) -> u32 {
        self.entries.len() as u32
    }
    fn raw(&self, index: u32) -> RawGuestBuffer {
        self.entries[index as usize]
    }
    fn memory(&self) -> &dyn UserMemory {
        self.memory
    }
    fn message_limit(&self) -> u64 {
        65_535 - 8
    }
    fn coalesce_window(&self) -> u32 {
        if self.operation == VectorOperation::Write {
            1024 * 1024
        } else {
            0
        }
    }
    fn fault(&self) -> wasixv2_syscalls::SysErr {
        self.failure.set(Some(Errno::MEMVIOLATION));
        wasixv2_syscalls::Errno::EFAULT.into()
    }
    fn enter(&self, index: u32) -> LocalFuture<'_, SysResult<RawGuestBuffer>> {
        Box::pin(async move {
            let span = self.raw(index);
            let failure = match span.addr.checked_add(span.len) {
                None => Some(Errno::OVERFLOW),
                Some(end) if end > self.memory.size().await => Some(Errno::MEMVIOLATION),
                Some(_) => None,
            };
            if let Some(failure) = failure {
                self.failure.set(Some(failure));
                return Err(wasixv2_syscalls::Errno::EFAULT.into());
            }
            Ok(span)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errno::Error;
    use wasixv2_syscalls::{
        Caller, Fault, Layer, Layered, MaybeFuture, SysErr, Syscall, SyscallRet, TaskId,
    };

    struct Memory;
    impl UserMemory for Memory {
        fn size(&self) -> LocalFuture<'_, u64> {
            Box::pin(async { 1u64 << 32 })
        }
        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
        fn read<'a>(&'a self, _: u32, _: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async { Err(Fault) })
        }
        fn write<'a>(&'a self, _: u32, _: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async { Err(Fault) })
        }
    }
    fn source(span: RawGuestBuffer) -> Vectors<'static> {
        Vectors {
            memory: &Memory,
            operation: VectorOperation::Read,
            fd: Fd(3),
            array: 0x100,
            entries: vec![span],
            failure: Cell::new(None),
        }
    }
    fn ready<F: std::future::Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("source fixture must not wait"),
        }
    }

    #[test]
    fn full_and_empty_memory32_spans_keep_their_original_u64_values() {
        for span in [
            RawGuestBuffer::new(0, 1u64 << 32),
            RawGuestBuffer::new(1u64 << 32, 0),
        ] {
            assert_eq!(ready(source(span).enter(0)).unwrap(), span);
        }
        let source = source(RawGuestBuffer::new(1u64 << 40, 0));
        assert!(ready(source.enter(0)).is_err());
        assert_eq!(
            source.finish::<()>(Err(wasixv2_syscalls::Errno::EFAULT.into())),
            Err(Error::V1(Errno::MEMVIOLATION))
        );
        assert!(!source.matches(VectorOperation::Write, Fd(3), 0x100, 1));
        assert!(!source.matches(VectorOperation::Read, Fd(4), 0x100, 1));
        assert!(!source.matches(VectorOperation::Read, Fd(3), 0x108, 1));
        assert!(!source.matches(VectorOperation::Read, Fd(3), 0x100, 2));
    }

    struct Probe {
        enter: bool,
    }
    impl Syscalls for Probe {
        fn readv<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            _: Fd,
            _: ConstIovecArray,
            _: u32,
        ) -> MaybeFuture<'a, SysResult<u64>> {
            MaybeFuture::Future(Box::pin(async move {
                if self.enter {
                    caller.io_vectors().unwrap().enter(0).await?;
                }
                Err(wasixv2_syscalls::Errno::EFAULT.into())
            }))
        }
    }
    struct ReplaceFault;
    impl Layer<Probe> for ReplaceFault {
        fn call<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            call: Syscall,
            next: &'a Probe,
        ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
            wasixv2_syscalls::dispatch(next, caller, call).map(|result| match result {
                Err(SysErr::Errno(wasixv2_syscalls::Errno::EFAULT)) => {
                    Err(wasixv2_syscalls::Errno::EPERM.into())
                }
                result => result,
            })
        }
    }

    #[test]
    fn an_outer_layer_can_replace_a_validation_fault_without_losing_its_errno() {
        let source = source(RawGuestBuffer::new(1u64 << 40, 1));
        let caller = Caller::new(TaskId(1), &Memory).with_io_vectors(&source);
        let stack = Layered::new(ReplaceFault, Probe { enter: true });
        let result = ready(stack.readv(&caller, Fd(3), source.array(), source.count()));
        assert_eq!(
            source.finish(result),
            Err(Error::Sys(wasixv2_syscalls::Errno::EPERM.into()))
        );
        assert_eq!(source.failure.get(), None);
    }

    #[test]
    fn unrelated_kernel_faults_do_not_become_legacy_span_faults() {
        let source = source(RawGuestBuffer::new(0x1000, 1));
        let caller = Caller::new(TaskId(1), &Memory).with_io_vectors(&source);
        let result =
            ready(Probe { enter: false }.readv(&caller, Fd(3), source.array(), source.count()));
        assert_eq!(
            source.finish(result),
            Err(Error::Sys(wasixv2_syscalls::Errno::EFAULT.into()))
        );
    }
}
