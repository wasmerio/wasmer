//! Hand-written stand-in for the output of `wasixv2-codegen`, covering only the syscalls of the
//! first slice. Replace it with generated code once the WIT generator exists.

use wasixv2_abi::Fd;

use crate::{
    caller::Caller,
    memory::{UserSlice, UserSliceMut},
    result::SysResult,
};

// The futures are deliberately not `Send`: wasmer's async host calls are not.
#[allow(async_fn_in_trait)]
pub trait Syscalls {
    async fn read(&self, caller: &Caller<'_>, fd: Fd, buf: UserSliceMut) -> SysResult<u32>;
    async fn write(&self, caller: &Caller<'_>, fd: Fd, buf: UserSlice) -> SysResult<u32>;
    async fn exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()>;
}

/// One syscall with its arguments, for layers that treat every call alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syscall {
    Read { fd: Fd, buf: UserSliceMut },
    Write { fd: Fd, buf: UserSlice },
    ExitGroup { status: i32 },
}

/// The value a [`Syscall`] produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallRet {
    Read(u32),
    Write(u32),
    ExitGroup,
}

pub async fn dispatch<S: Syscalls + ?Sized>(
    syscalls: &S,
    caller: &Caller<'_>,
    call: Syscall,
) -> SysResult<SyscallRet> {
    Ok(match call {
        Syscall::Read { fd, buf } => SyscallRet::Read(syscalls.read(caller, fd, buf).await?),
        Syscall::Write { fd, buf } => SyscallRet::Write(syscalls.write(caller, fd, buf).await?),
        Syscall::ExitGroup { status } => {
            syscalls.exit_group(caller, status).await?;
            SyscallRet::ExitGroup
        }
    })
}

/// A layer that sees every syscall as a [`Syscall`] value.
#[allow(async_fn_in_trait)]
pub trait Layer {
    async fn call<S: Syscalls>(
        &self,
        caller: &Caller<'_>,
        call: Syscall,
        next: &S,
    ) -> SysResult<SyscallRet>;
}

/// Runs `layer` in front of `inner`.
pub struct Layered<L, S> {
    pub layer: L,
    pub inner: S,
}

impl<L, S> Layered<L, S> {
    pub fn new(layer: L, inner: S) -> Self {
        Self { layer, inner }
    }
}

fn mismatch(call: &str) -> ! {
    unreachable!("a layer answered `{call}` with another syscall's result")
}

impl<L: Layer, S: Syscalls> Syscalls for Layered<L, S> {
    async fn read(&self, caller: &Caller<'_>, fd: Fd, buf: UserSliceMut) -> SysResult<u32> {
        let call = Syscall::Read { fd, buf };
        match self.layer.call(caller, call, &self.inner).await? {
            SyscallRet::Read(n) => Ok(n),
            _ => mismatch("read"),
        }
    }

    async fn write(&self, caller: &Caller<'_>, fd: Fd, buf: UserSlice) -> SysResult<u32> {
        let call = Syscall::Write { fd, buf };
        match self.layer.call(caller, call, &self.inner).await? {
            SyscallRet::Write(n) => Ok(n),
            _ => mismatch("write"),
        }
    }

    async fn exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let call = Syscall::ExitGroup { status };
        match self.layer.call(caller, call, &self.inner).await? {
            SyscallRet::ExitGroup => Ok(()),
            _ => mismatch("exit_group"),
        }
    }
}

impl<L, S: crate::Control> crate::Control for Layered<L, S> {
    fn take_pending(&self, task: crate::TaskId) -> Option<crate::GuestAction> {
        self.inner.take_pending(task)
    }
}
