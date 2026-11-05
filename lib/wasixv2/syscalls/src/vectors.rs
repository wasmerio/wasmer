//! Invocation-local authority for a compatibility ABI's vector layout.

use crate::{Fd, LocalFuture, RawGuestBuffer, SysResult, UserMemory};

/// The operation whose real guest array this capability describes. An edited syscall
/// cannot reuse a source acquired for a different operation or descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorOperation {
    Read,
    Write,
    ReadAt,
    WriteAt,
    Receive,
    Send,
}

/// A vector source borrowed for one ordinary syscall dispatch. This capability carries
/// neither a descriptor table nor a retained engine/store; native callers omit it.
pub trait IoVectorSource {
    fn matches(&self, operation: VectorOperation, fd: Fd, array: u32, count: u32) -> bool;
    fn count(&self) -> u32;
    fn raw(&self, index: u32) -> RawGuestBuffer;
    /// Validate the complete nested span, even when empty, at the point it is entered.
    /// The returned address remains wide: a valid empty one-past range need not fit u32.
    fn enter(&self, index: u32) -> LocalFuture<'_, SysResult<RawGuestBuffer>>;
    /// The real vector payload memory, independent of a shim's structured scratch arena.
    fn memory(&self) -> &dyn UserMemory;
    /// Compatibility message and fd-write socket gather policies; these are not native
    /// Linux iov_iter limits and never apply to ordinary callers.
    fn message_limit(&self) -> u64;
    fn coalesce_window(&self) -> u32;
    /// The source owns any compatibility-specific copy-fault verdict.
    fn fault(&self) -> crate::SysErr {
        crate::Errno::EFAULT.into()
    }
}
