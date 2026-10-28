//! The wasixv2 imports of the asyncify build configuration, on top of [`crate::asyncify`]:
//! fork (`clone` without `CLONE_VM`), and `stack-save` and `stack-restore`, which are libc's
//! `setjmp` and `longjmp` there. Modules in other configurations keep the generated imports,
//! whose calls the kernel answers with `ENOSYS`.

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
    parent_tid: Option<u32>,
    child_tid: Option<u32>,
    tls: Option<u32>,
    /// The forking import's own data ([`asyncify::import::set_fork_payload`]).
    payload: Option<asyncify::Tag>,
}

/// The payload of a fork's `tag`.
pub(crate) fn payload(tag: &asyncify::Tag) -> Option<asyncify::Tag> {
    tag.downcast_ref::<Writes>()?.payload.clone()
}

/// Replaces the imports that unwind the stack with their asyncify versions, for an instrumented
/// module.
pub(crate) fn imports<S>(store: &mut Store, env: &FunctionEnv<ExecEnv<S>>, imports: &mut Imports)
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    // The rewinding guest calls clone again; the kernel must see it only once.
    if let Some(wasmer::Extern::Function(clone)) = imports.get_export(IMPORT_MODULE, "clone") {
        let ty = clone.ty(store);
        let resumable = Function::new_with_env_async(store, env, ty, move |env, args| {
            let clone = clone.clone();
            let args = args.to_vec();
            async move {
                if let Some(resume) = asyncify::resumed(&env).await? {
                    return Ok(vec![Value::I32(fork_returned(&env, resume).await?)]);
                }
                Ok(clone
                    .call_async(&env.as_store_async(), args)
                    .await?
                    .into_vec())
            }
        });
        imports.define(IMPORT_MODULE, "clone", resumable);
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
    parent_tid: Option<u32>,
    child_tid: Option<u32>,
    tls: Option<u32>,
) -> SysResult<()> {
    let payload = env.write().await.data_mut().fork_payload.take();
    let tag = Arc::new(Writes {
        parent_tid,
        child_tid,
        tls,
        payload,
    });
    let shared = syscalls.shared_mappings(child);
    let request = Request::Fork { child, tag, shared };
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
/// `CLONE_PARENT_SETTID`'s write, and 0 in the child, after `CLONE_CHILD_SETTID`'s write and
/// `CLONE_SETTLS`.
async fn fork_returned<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    resume: Resume,
) -> Result<i32, RuntimeError> {
    let Resume::Forked { child, tag } = resume else {
        return Err(RuntimeError::new(
            "clone resumed from something other than a fork",
        ));
    };
    let writes = tag
        .downcast_ref::<Writes>()
        .ok_or_else(|| RuntimeError::new("clone resumed from another ABI's fork"))?;
    env.with(|data, store| {
        // Linux ignores faults on both writes.
        match child {
            Some(child) => {
                if let Some(addr) = writes.parent_tid {
                    let _ = data.write_memory(&*store, addr, &child.0.to_le_bytes());
                }
                Ok(child.0 as i32)
            }
            None => {
                if let Some(addr) = writes.child_tid {
                    let _ = data.write_memory(&*store, addr, &data.task.0.to_le_bytes());
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
                Ok(0)
            }
        }
    })
    .await
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
