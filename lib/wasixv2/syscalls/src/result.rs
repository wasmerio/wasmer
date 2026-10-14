use wasixv2_abi::Errno;

use crate::memory::Fault;

/// Why a syscall did not produce a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysErr {
    Errno(Errno),
    /// Interrupted by a signal; the return-to-guest path decides between rerunning the call and
    /// returning `EINTR`.
    Restart,
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
