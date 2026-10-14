//! The `Syscalls` trait with one async method per syscall, the request enum, the `Layered` adapter,
//! user-memory reference types, result and restart types, and the return-to-guest interface.
//!
//! Everything above the kernel talks to it only through this crate.

mod caller;
mod control;
mod generated;
mod memory;
mod result;

pub use caller::{Caller, TaskId};
pub use control::{Control, GuestAction};
pub use generated::{Layer, Layered, Syscall, SyscallRet, Syscalls, dispatch};
pub use memory::{Fault, LocalFuture, UserMemory, UserSlice, UserSliceMut};
pub use result::{SysErr, SysResult};
pub use wasixv2_abi::{Errno, Fd};
