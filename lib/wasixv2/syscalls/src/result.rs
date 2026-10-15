use wasixv2_abi::Errno;

use crate::memory::Fault;

/// Why a syscall did not produce a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysErr {
    Errno(Errno),
    /// Interrupted by a signal (`ERESTARTSYS`); the return-to-guest path reruns the call unless a
    /// handler without `SA_RESTART` ran, in which case it returns `EINTR`.
    Restart,
    /// Interrupted by a signal (`ERESTARTNOHAND`): `EINTR` once any handler ran, whatever its
    /// `SA_RESTART`; rerun otherwise. For calls Linux never restarts after a handler, such as
    /// `poll`, `pause` and `sigsuspend`.
    RestartNoHand,
}

impl From<Errno> for SysErr {
    fn from(errno: Errno) -> Self {
        Self::Errno(errno)
    }
}

impl From<Fault> for SysErr {
    fn from(_: Fault) -> Self {
        Self::Errno(Errno::EFAULT)
    }
}

pub type SysResult<T> = Result<T, SysErr>;
