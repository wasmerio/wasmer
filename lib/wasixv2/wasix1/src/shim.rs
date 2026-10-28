//! The frame every v1 import runs in, and the one pattern every shim follows:
//!
//! ```text
//! async fn fd_close<S: Syscalls + Control + 'static>(env: Env<S>, fd: i32) -> Result<i32, RuntimeError> {
//!     shim::errno(&env, async |frame| {
//!         frame.syscalls.close(&frame.caller(), Fd(fd)).await?;
//!         Ok(())
//!     }).await
//! }
//! ```
//!
//! The body translates arguments, makes v2 calls through [`Frame::syscalls`] with
//! [`Frame::caller`], writes results to the guest and fails with `?` on the first error.
//! [`call`] then runs the return-to-guest path ([`wasixv2_exec::complete`]): signal handlers,
//! exits, task switches. It reruns the body when Linux would restart the call, and encodes the
//! result in v1 numbering. A body must not make further v2 calls after one failed, and must not
//! write guest-visible results before the last call that can be interrupted.

use std::sync::Arc;

use wasixv2_abi::{GuestLayout, GuestPtrMut};
use wasixv2_exec::{AbiInstance, ExecEnv, GuestMemory, Outcome, complete};
use wasixv2_syscalls::{Caller, Control, SysErr, Syscalls, TaskId, read_value, write_value};
use wasmer::{AsyncFunctionEnvMut, RuntimeError};

use crate::{
    abi::{self, Errno},
    errno::{Error, V1Result},
    instance::Wasix1Instance,
    memory::CompatMemory,
    process::{ProcState, Startup},
};

/// The environment every v1 import receives.
pub type Env<S> = AsyncFunctionEnvMut<ExecEnv<S>>;

/// What one import call works with.
pub struct Frame<'a, S: 'static> {
    pub env: &'a Env<S>,
    pub syscalls: &'a S,
    /// The calling task.
    pub task: TaskId,
    /// Guest memory plus this call's scratch window.
    pub mem: CompatMemory<'a, S>,
    abi: Arc<dyn AbiInstance<S>>,
}

impl<'a, S: Syscalls + Control + 'static> Frame<'a, S> {
    /// The caller to pass to every v2 call: this task, with this call's memory.
    pub fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.mem)
    }

    pub(crate) fn instance(&self) -> &Wasix1Instance {
        self.abi
            .as_any()
            .downcast_ref()
            .expect("v1 imports run in v1 instances")
    }

    /// The calling task's pid, which it asks the kernel for only the first time in its
    /// instance (see [`Wasix1Instance::cached_pid`]).
    pub async fn pid(&self) -> V1Result<TaskId> {
        let instance = self.instance();
        if let Some(pid) = instance.cached_pid(self.task) {
            return Ok(pid);
        }
        let pid = TaskId(self.syscalls.getpid(&self.caller()).await? as u32);
        instance.cache_pid(self.task, pid);
        Ok(pid)
    }

    /// The compat state of the calling task's process.
    pub async fn proc(&self) -> V1Result<Arc<ProcState>> {
        let pid = self.pid().await?;
        Ok(self.instance().procs().get_or_create(pid))
    }

    /// The process's arguments, environment and preopens, loaded on first use.
    pub async fn startup(&self) -> V1Result<Arc<Startup>> {
        let proc = self.proc().await?;
        Ok(crate::calls::startup::load(self, &proc).await)
    }

    /// Reads a `T` from guest memory (never scratch) at `addr`.
    pub async fn read<T: GuestLayout>(&self, addr: u32) -> V1Result<T> {
        Ok(read_value(self.mem.guest(), wasixv2_abi::GuestPtr::new(addr)).await?)
    }

    /// Writes `value` to guest memory (never scratch) at `addr`.
    pub async fn write<T: GuestLayout>(&self, addr: u32, value: &T) -> V1Result<()> {
        Ok(write_value(self.mem.guest(), GuestPtrMut::new(addr), value).await?)
    }

    /// Writes `bytes` to guest memory at `addr`.
    pub async fn write_bytes(&self, addr: u32, bytes: &[u8]) -> V1Result<()> {
        use wasixv2_syscalls::UserMemory;
        Ok(self.mem.guest().write(addr, bytes).await?)
    }
}

/// How a v1 call ended, once the return-to-guest path ran.
pub enum Done<T> {
    Ok(T),
    Err(Errno),
    /// The instance runs as another task now, and the call returns this to it
    /// ([`Outcome::Switched`]).
    Switched(i32),
    /// The guest's stack unwinds for asyncify ([`Outcome::Unwinding`]): the import returns
    /// anything, and finds out how it ended when the guest rewinds into it.
    Unwinding,
}

/// Runs `body` as one v1 call (see the module docs).
pub async fn call<S, T>(
    env: &Env<S>,
    mut body: impl AsyncFnMut(&Frame<'_, S>) -> V1Result<T>,
) -> Result<Done<T>, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let (syscalls, task, abi) = {
        let guard = env.read().await;
        let data = guard.data();
        let abi = data
            .abi()
            .cloned()
            .ok_or_else(|| RuntimeError::new("a v1 import ran before its instance was bound"))?;
        (data.syscalls().clone(), data.task(), abi)
    };
    let frame = Frame {
        env,
        syscalls: &*syscalls,
        task,
        mem: CompatMemory::new(GuestMemory::new(env)),
        abi,
    };
    if !frame.instance().started() {
        // Preopens must exist before the guest's first look at its descriptors.
        if frame.startup().await.is_ok() {
            frame.instance().set_started();
        }
    }
    loop {
        let (ret, own) = match body(&frame).await {
            Ok(value) => (Ok(Some(value)), None),
            Err(Error::Sys(err)) => (Err::<Option<T>, SysErr>(err), None),
            Err(Error::V1(errno)) => (Ok(None), Some(errno)),
        };
        return Ok(match complete(env, &*syscalls, task, ret).await {
            // A body cannot go on halfway, so a call that saved how to continue
            // (`ERESTART_RESTARTBLOCK`) runs again from the start too.
            Outcome::Rerun | Outcome::Restart => continue,
            Outcome::Return(Ok(Some(value))) => Done::Ok(value),
            Outcome::Return(Ok(None)) => Done::Err(own.expect("set with the `None` value")),
            Outcome::Return(Err(errno)) => Done::Err(Errno::from_linux(errno)),
            Outcome::Switched(value) => Done::Switched(value),
            Outcome::Unwinding => Done::Unwinding,
            Outcome::Unwind(err) => return Err(err),
        });
    }
}

/// [`call`] for an import that returns a v1 errno: 0 on success (and after a task switch,
/// whose shims read the new task themselves).
pub async fn errno<S>(
    env: &Env<S>,
    body: impl AsyncFnMut(&Frame<'_, S>) -> V1Result<()>,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    Ok(match call(env, body).await? {
        Done::Ok(()) | Done::Switched(_) | Done::Unwinding => 0,
        Done::Err(errno) => i32::from(errno.0),
    })
}

/// [`call`] for an import without a result. A failure has nowhere to go and is logged.
pub async fn void<S>(
    env: &Env<S>,
    name: &'static str,
    body: impl AsyncFnMut(&Frame<'_, S>) -> V1Result<()>,
) -> Result<(), RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    if let Done::Err(errno) = call(env, body).await? {
        tracing::debug!(call = name, errno = errno.0, "WASIX v1 call failed");
    }
    Ok(())
}

/// The v1 errno `errno`, as a body's error.
pub fn fail<T>(errno: abi::Errno) -> V1Result<T> {
    Err(Error::V1(errno))
}
