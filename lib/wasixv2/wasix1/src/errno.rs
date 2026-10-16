//! How a v1 shim fails: with what a v2 syscall said, or with a v1 errno of its own.

use wasixv2_syscalls::{Fault, SysErr};

use crate::abi;

/// The error of a v1 shim body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A v2 syscall failed or was interrupted; [`crate::shim::call`] runs the return-to-guest
    /// path, which may rerun the body or turn the interruption into `EINTR`.
    Sys(SysErr),
    /// The shim's own verdict, in v1 numbering (for errnos with no Linux counterpart, such as
    /// `NOTCAPABLE`, or v1-specific choices).
    V1(abi::Errno),
}

pub type V1Result<T> = Result<T, Error>;

impl From<SysErr> for Error {
    fn from(err: SysErr) -> Self {
        Self::Sys(err)
    }
}

impl From<wasixv2_abi::Errno> for Error {
    fn from(errno: wasixv2_abi::Errno) -> Self {
        Self::Sys(SysErr::Errno(errno))
    }
}

impl From<Fault> for Error {
    fn from(_: Fault) -> Self {
        Self::Sys(SysErr::Errno(wasixv2_abi::Errno::EFAULT))
    }
}

impl From<abi::Errno> for Error {
    fn from(errno: abi::Errno) -> Self {
        Self::V1(errno)
    }
}

/// The value an errno-returning import hands the guest.
pub fn encode(result: Result<(), abi::Errno>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(errno) => i32::from(errno.0),
    }
}
