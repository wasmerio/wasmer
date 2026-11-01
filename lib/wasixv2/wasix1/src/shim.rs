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

use std::{cell::RefCell, sync::Arc};

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
    unwind: wasixv2_exec::GuestUnwind,
    descriptors: RefCell<Option<wasixv2_syscalls::DescriptorSet>>,
    installations: RefCell<Option<wasixv2_syscalls::DescriptorInstallations>>,
}

impl<'a, S: Syscalls + Control + 'static> Frame<'a, S> {
    /// The caller to pass to every v2 call: this task, with this call's memory.
    pub fn caller(&self) -> Caller<'_> {
        let mut caller = Caller::new(self.task, &self.mem);
        if let Some(set) = &*self.descriptors.borrow() {
            caller = caller.with_descriptors(set);
        }
        if let Some(plan) = &*self.installations.borrow() {
            caller = caller.with_descriptor_installations(plan);
        }
        caller
    }

    pub(crate) fn bind_descriptors(
        &self,
        fds: &[wasixv2_abi::Fd],
    ) -> V1Result<wasixv2_syscalls::DescriptorSet> {
        if let Some(set) = &*self.descriptors.borrow() {
            if fds.iter().any(|&fd| set.get(fd).is_none()) {
                return fail(Errno::BADF);
            }
            return Ok(set.clone());
        }
        let set = self
            .syscalls
            .snapshot_descriptors(self.task, fds)
            .map_err(SysErr::from)?;
        self.replace_descriptors(set.clone());
        Ok(set)
    }

    pub(crate) fn replace_descriptors(&self, set: wasixv2_syscalls::DescriptorSet) {
        *self.descriptors.borrow_mut() = Some(set);
    }

    pub(crate) fn install_descriptors(
        &self,
        attachments: Vec<Option<wasixv2_syscalls::DescriptorAttachment>>,
    ) -> V1Result<wasixv2_syscalls::DescriptorInstallations> {
        self.bind_descriptors(&[])?;
        let plan = wasixv2_syscalls::DescriptorInstallations::new(attachments);
        *self.installations.borrow_mut() = Some(plan.clone());
        Ok(plan)
    }

    pub(crate) fn use_installed(
        &self,
        plan: &wasixv2_syscalls::DescriptorInstallations,
        index: usize,
    ) -> V1Result<()> {
        let result = plan.result(index).ok_or(Errno::IO)?;
        self.replace_descriptors(result.clone());
        *self.installations.borrow_mut() = None;
        Ok(())
    }

    /// The original guest error raised by an engine capability in this import.
    pub(crate) fn unwind(&self) -> &wasixv2_exec::GuestUnwind {
        &self.unwind
    }

    pub(crate) fn instance(&self) -> &Wasix1Instance {
        self.abi
            .as_any()
            .downcast_ref()
            .expect("v1 imports run in v1 instances")
    }

    /// The calling task's pid, as it sees it (`getpid`).
    pub async fn pid(&self) -> V1Result<i32> {
        Ok(self.syscalls.getpid(&self.caller()).await?)
    }

    /// The calling task's process, by the kernel's id for it, whatever pid it has: what its
    /// compat state is kept under. Asked for only the first time in the instance (see
    /// [`Wasix1Instance::cached_process`]).
    pub fn process(&self) -> TaskId {
        let instance = self.instance();
        if let Some(process) = instance.cached_process(self.task) {
            return process;
        }
        let process = self.syscalls.tgid(self.task);
        instance.cache_process(self.task, process);
        process
    }

    /// The compat state of the calling task's process.
    pub async fn proc(&self) -> V1Result<Arc<ProcState>> {
        Ok(self.instance().procs().get_or_create(self.process()))
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
        data.enter();
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
        unwind: wasixv2_exec::GuestUnwind::default(),
        descriptors: RefCell::new(None),
        installations: RefCell::new(None),
    };
    if !frame.instance().started() {
        // Preopens must exist before the guest's first look at its descriptors.
        if frame.startup().await.is_ok() {
            frame.instance().set_started();
        }
    }
    loop {
        *frame.descriptors.borrow_mut() = None;
        *frame.installations.borrow_mut() = None;
        let (ret, own) = match body(&frame).await {
            Ok(value) => (Ok(Some(value)), None),
            Err(Error::Sys(err)) => (Err::<Option<T>, SysErr>(err), None),
            Err(Error::V1(errno)) => (Ok(None), Some(errno)),
        };
        let ret = frame.unwind().finish(env, ret).await?;
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
