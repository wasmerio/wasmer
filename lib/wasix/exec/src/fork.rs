//! The wasixv2 imports of the asyncify build configuration, on top of [`crate::asyncify`]:
//! fork (`clone` or `clone3` without `CLONE_VM`), and `stack-save` and `stack-restore`, which are
//! libc's `setjmp` and `longjmp` there. Modules in other configurations keep the generated
//! imports, whose calls the kernel answers with `ENOSYS`.

use std::sync::Arc;

use wasixv2_abi::Signal;
use wasixv2_syscalls::{Control, Errno, SysErr, SysResult, Syscalls, TaskId};
use wasmer::{AsyncFunctionEnvMut, Function, FunctionEnv, Imports, RuntimeError, Store, Value};

use crate::{
    ExecEnv, IMPORT_MODULE,
    asyncify::{self, Access, Request, Resume, StackId},
    process,
};

/// clone's writes that wait for the copy of the address space (see [`GuestAction::Fork`]).
///
/// [`GuestAction::Fork`]: wasixv2_syscalls::GuestAction::Fork
struct Writes {
    /// The child's pid as the parent sees it, which clone returns there.
    pid: i32,
    /// The child's tid as it sees it itself.
    tid: i32,
    parent_tid: Option<u32>,
    /// Where the parent's pidfd goes, and its number.
    pidfd: Option<(u32, i32)>,
    child_tid: Option<u32>,
    tls: Option<u32>,
    /// `CLONE_VFORK`: the parent waits for the child before clone returns.
    vfork: bool,
    /// The forking import's own data ([`asyncify::import::set_fork_payload`]).
    payload: Option<asyncify::Tag>,
}

/// The payload of a fork's `tag`.
pub(crate) fn payload(tag: &asyncify::Tag) -> Option<asyncify::Tag> {
    tag.downcast_ref::<Writes>()?.payload.clone()
}

/// The child's pid as the parent sees it, of a fork's `tag`.
pub(crate) fn child_pid(tag: &asyncify::Tag) -> Option<i32> {
    Some(tag.downcast_ref::<Writes>()?.pid)
}

/// Replaces the imports that unwind the stack with their asyncify versions, for an instrumented
/// module.
pub(crate) fn imports<S>(store: &mut Store, env: &FunctionEnv<ExecEnv<S>>, imports: &mut Imports)
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    // The rewinding guest calls clone (or clone3) again; the kernel must see it only once.
    for name in ["clone", "clone3"] {
        let Some(wasmer::Extern::Function(clone)) = imports.get_export(IMPORT_MODULE, name) else {
            continue;
        };
        let ty = clone.ty(store);
        let resumable = Function::new_with_env_async(store, env, ty, move |env, args| {
            let clone = clone.clone();
            let args = args.to_vec();
            async move {
                if let Some(resume) = asyncify::resumed(&env).await? {
                    let ret = match fork_returned(&env, resume).await? {
                        (ret, false) => ret,
                        (pid, true) => vfork_returned(&env, pid).await?,
                    };
                    return Ok(vec![Value::I32(ret)]);
                }
                Ok(clone
                    .call_async(&env.as_store_async(), args)
                    .await?
                    .into_vec())
            }
        });
        imports.define(IMPORT_MODULE, name, resumable);
    }
    imports.define(
        IMPORT_MODULE,
        "stack_save",
        Function::new_typed_with_env_async(store, env, stack_save::<S>),
    );
    imports.define(
        IMPORT_MODULE,
        "stack_restore",
        Function::new_typed_with_env_async(store, env, stack_restore::<S>),
    );
}

/// The return-to-guest half of fork, for the kernel's [`GuestAction::Fork`]: unwinds the stack
/// so the outermost level copies the instance for `child`. The import then returns, and clone
/// returns once the guest rewinds into it ([`fork_returned`]).
///
/// [`GuestAction::Fork`]: wasixv2_syscalls::GuestAction::Fork
pub(crate) async fn start<S: Syscalls + Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    syscalls: &S,
    child: TaskId,
    (pid, parent_tid, pidfd, vfork): (i32, Option<u32>, Option<(u32, i32)>, bool),
    (tid, child_tid, tls): (i32, Option<u32>, Option<u32>),
) -> SysResult<()> {
    let payload = env.write().await.data_mut().fork_payload.take();
    let tag = Arc::new(Writes {
        pid,
        tid,
        parent_tid,
        pidfd,
        child_tid,
        tls,
        vfork,
        payload,
    });
    let shared = syscalls.shared_mappings(child);
    let wiped = syscalls.wipe_on_fork(child);
    let request = Request::Fork {
        child,
        tag,
        shared,
        wiped,
    };
    match asyncify::unwind(env, request).await {
        Ok(()) => Ok(()),
        Err(err) => {
            // The kernel made the child already; it never runs.
            tracing::warn!(%err, "cannot unwind the stack for fork");
            process::fault(syscalls, child, Signal::SIGSEGV.0);
            syscalls.release(child);
            Err(SysErr::Errno(Errno::ENOMEM))
        }
    }
}

/// What clone returns once the guest rewound into it: the child's pid in the parent, after
/// `CLONE_PARENT_SETTID`'s and `CLONE_PIDFD`'s writes, and 0 in the child, after `CLONE_CHILD_SETTID`'s write and
/// `CLONE_SETTLS`; and whether the parent has yet to wait for its vfork child.
async fn fork_returned<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    resume: Resume,
) -> Result<(i32, bool), RuntimeError> {
    let Resume::Forked { child, tag } = resume else {
        return Err(RuntimeError::new(
            "clone resumed from something other than a fork",
        ));
    };
    let writes = tag
        .downcast_ref::<Writes>()
        .ok_or_else(|| RuntimeError::new("clone resumed from another ABI's fork"))?;
    env.with(|data, store| {
        // Linux ignores faults on the tid writes; the kernel checked the pidfd's would land.
        match child {
            Some(_) => {
                if let Some(addr) = writes.parent_tid {
                    let _ = data.write_memory(&*store, addr, &writes.pid.to_le_bytes());
                }
                if let Some((addr, fd)) = writes.pidfd {
                    let _ = data.write_memory(&*store, addr, &fd.to_le_bytes());
                }
                Ok((writes.pid, writes.vfork))
            }
            None => {
                if let Some(addr) = writes.child_tid {
                    let _ = data.write_memory(&*store, addr, &writes.tid.to_le_bytes());
                }
                let tls_base = data.abi.as_ref().and_then(|abi| {
                    let instance = abi
                        .as_any()
                        .downcast_ref::<crate::wasixv2::Wasixv2Instance>();
                    instance?.tls_base()
                });
                if let (Some(tls), Some(global)) = (writes.tls, tls_base) {
                    global.set(store, Value::I32(tls as i32))?;
                }
                Ok((0, false))
            }
        }
    })
    .await
}

/// The parent's end of a fork with `CLONE_VFORK`, once its child runs: clone returns `pid`
/// only after the child is done with its copy of the memory, as `kernel_clone` waits in
/// `wait_for_vfork_done`, and through the return-to-guest path, where a fatal signal that
/// ended the wait takes the parent.
async fn vfork_returned<S: Syscalls + Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    pid: i32,
) -> Result<i32, RuntimeError> {
    let (syscalls, task) = {
        let env = env.read().await;
        let data = env.data();
        (data.syscalls.clone(), data.task)
    };
    syscalls.vfork_wait(task).await;
    crate::return_to_guest(env, &*syscalls, task, Ok(pid))
        .await
        .unwrap_or(Ok(pid))
}

/// `stack-snapshot`: the saved stack's slot, generation and level.
const SNAPSHOT_SIZE: usize = 24;

/// `stack-save`: 0 once the stack is saved, and 1 with `val` written when `stack-restore`
/// resumes it.
async fn stack_save<S: 'static>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    snapshot: i32,
    val: i32,
) -> Result<i32, RuntimeError> {
    let (snapshot, val) = (snapshot as u32, val as u32);
    let bytes = match asyncify::resumed(&env).await? {
        None => {
            asyncify::unwind(&env, Request::Save { slot: snapshot }).await?;
            return Ok(0);
        }
        Some(Resume::Saved(id)) => {
            let mut bytes = [0u8; SNAPSHOT_SIZE];
            bytes[..8].copy_from_slice(&u64::from(id.slot).to_le_bytes());
            bytes[8..16].copy_from_slice(&id.generation.to_le_bytes());
            bytes[16..].copy_from_slice(&u64::from(id.level).to_le_bytes());
            (snapshot, bytes.to_vec(), 0)
        }
        Some(Resume::Restored(value)) => (val, value.to_le_bytes().to_vec(), 1),
        Some(Resume::Forked { .. }) => {
            return Err(RuntimeError::new("stack_save resumed from a fork"));
        }
    };
    let (addr, bytes, ret) = bytes;
    write(&env, addr, &bytes).await?;
    Ok(ret)
}

/// `stack-restore`: resumes the `stack-save` that filled `snapshot`.
async fn stack_restore<S: 'static>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    snapshot: i32,
    val: i32,
) -> Result<(), RuntimeError> {
    let mut bytes = [0u8; SNAPSHOT_SIZE];
    read(&env, snapshot as u32, &mut bytes).await?;
    let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let id = StackId {
        slot: word(0) as u32,
        generation: word(8),
        level: word(16) as u32,
    };
    Err(asyncify::restore(&env, id, val).await)
}

async fn write<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: u32,
    bytes: &[u8],
) -> Result<(), RuntimeError> {
    env.with(|data, store| {
        data.write_memory(&*store, addr, bytes)
            .map_err(|_| RuntimeError::new("stack snapshot outside guest memory"))
    })
    .await
}

async fn read<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    addr: u32,
    bytes: &mut [u8],
) -> Result<(), RuntimeError> {
    env.with(|data, store| {
        data.read_memory(&*store, addr, bytes)
            .map_err(|_| RuntimeError::new("stack snapshot outside guest memory"))
    })
    .await
}
