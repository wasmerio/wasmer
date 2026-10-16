//! Processes: fork, vfork, exec, spawn, join and the parent pid.
//!
//! - `proc_fork_env` is vfork as wasix-libc's wasm-exceptions builds do it: the kernel's
//!   `clone(CLONE_VM | CLONE_VFORK)` makes the instance run as the child, the call returns 0
//!   to it, and once the child's `proc_exec*` succeeds or `proc_exit2` runs, that call returns
//!   in the parent instead, and libc longjmps to the vfork point
//!   ([`crate::instance::Wasix1Instance`]'s `vfork_resume`).
//! - `proc_fork(copy_memory = false)` is vfork in libc's asyncify builds: the stack is saved at
//!   the call, the instance continues as the child, and the parent resumes on the saved stack,
//!   where `proc_fork` returns the child's pid.
//! - `proc_fork(copy_memory = true)` is the kernel's fork (`clone` without `CLONE_VM`), which
//!   the execution layer serves for asyncify builds by copying the instance; the child takes a
//!   copy of the compat state with it.
//! - `proc_spawn2`/`proc_spawn3` are `posix_spawn` inside one call: vfork, the file actions and
//!   signal dispositions as the child, `execve`, and the parent resumes, as musl's
//!   `posix_spawn` does with `CLONE_VM | CLONE_VFORK`.
//! - `proc_join` is `waitid`.
//!
//! A vfork child runs in its parent's instance, so its compat state is registered by its pid
//! when it starts and dropped when the parent resumes.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use wasixv2_abi::{
    AtFd, FcntlCmd, Fd, GuestLayout, Idtype, OpenFlags, RusagePtr, S32Ptr, SaFlags, SiCode,
    Siginfo, SignalHandler, WaitOptions,
};
use wasixv2_exec::{
    AbiInstance,
    asyncify::import::{self, Resumed},
};
use wasixv2_syscalls::{Control, Errno as LinuxErrno, SysErr, Syscalls, TaskId, UserMemory};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Bool, Errno,
        fs::{Fdflags, Fdflagsext, Lookupflags, Oflags, Rights},
        proc::{
            Disposition, JoinFlags, JoinStatus, JoinStatusType, OptionPid, OptionTag,
            ProcSpawnFdOp, ProcSpawnFdOpName, Signal, SignalDisposition,
        },
    },
    calls::{signal, tty},
    errno::{Error, V1Result},
    instance::{Vfork, Wasix1Instance},
    process::ProcState,
    shim::{self, Done, Env, Frame},
};

/// Per-process compat state for process management.
#[derive(Debug, Default)]
pub struct State {
    /// Whether `callback_signal` installed the kernel handlers (see `signal.rs`).
    pub(crate) signals_installed: AtomicBool,
    pub(crate) tty: tty::State,
}

impl State {
    /// A fork or vfork child's copy: it inherits the parent's signal handlers and terminal.
    pub(crate) fn fork(&self) -> Self {
        Self {
            signals_installed: AtomicBool::new(self.signals_installed.load(Ordering::Acquire)),
            tty: self.tty.fork(),
        }
    }
}

const SIGCHLD: u32 = 17;

/// vfork: `clone(CLONE_VM | CLONE_VFORK | SIGCHLD)`.
const VFORK: u32 = wasixv2_abi::CloneFlags::CLONE_VM.0 as u32
    | wasixv2_abi::CloneFlags::CLONE_VFORK.0 as u32
    | SIGCHLD;

/// The status of a spawned child whose `execve` failed: the shells' "cannot execute".
const EXEC_FAILED: i32 = 127;

/// The search path when the caller asks for a search but passes none (musl's default).
const DEFAULT_PATH: &[u8] = b"/usr/local/bin:/bin:/usr/bin";

/// Longest argument, environment entry or path a v1 call may pass (Linux's `MAX_ARG_STRLEN`).
const MAX_STRING: usize = 32 * 4096;

// ---- guest strings ----

/// `len` bytes of guest memory at `addr`.
pub(crate) async fn read_bytes<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    addr: u32,
    len: u32,
) -> V1Result<Vec<u8>> {
    let mut bytes = vec![0; len as usize];
    frame.mem.guest().read(addr, &mut bytes).await?;
    Ok(bytes)
}

/// The NUL-terminated string at `addr`.
async fn read_cstr<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    addr: u32,
) -> V1Result<Vec<u8>> {
    const CHUNK: u64 = 256;
    let size = frame.mem.guest().size().await;
    let mut out = Vec::new();
    let mut at = u64::from(addr);
    while out.len() <= MAX_STRING {
        let len = CHUNK.min(size.saturating_sub(at));
        if len == 0 {
            return Err(LinuxErrno::EFAULT.into());
        }
        let mut chunk = vec![0; len as usize];
        frame.mem.guest().read(at as u32, &mut chunk).await?;
        if let Some(nul) = chunk.iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..nul]);
            return Ok(out);
        }
        out.extend_from_slice(&chunk);
        at += len;
    }
    shim::fail(Errno::TOOBIG)
}

/// `count` strings through the pointer array at `ptrs`.
async fn read_cstr_array<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptrs: u32,
    count: u32,
) -> V1Result<Vec<Vec<u8>>> {
    let mut strings = Vec::with_capacity(count.min(4096) as usize);
    for i in 0..count {
        let ptr: u32 = frame.read(ptrs.wrapping_add(4 * i)).await?;
        strings.push(read_cstr(frame, ptr).await?);
    }
    Ok(strings)
}

/// The string `(ptr, len)`, or `None` for a NULL `ptr`.
async fn read_optional<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptr: i32,
    len: i32,
) -> V1Result<Option<Vec<u8>>> {
    match ptr {
        0 => Ok(None),
        ptr => Ok(Some(read_bytes(frame, ptr as u32, len as u32).await?)),
    }
}

/// v1's newline-separated argument list: one argument per line, a trailing newline ends the
/// last.
fn split_args(list: &[u8]) -> Vec<Vec<u8>> {
    let end = list
        .iter()
        .rposition(|&b| b != b'\n' && b != b'\r')
        .map_or(0, |last| last + 1);
    if end == 0 {
        return Vec::new();
    }
    list[..end]
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
        .collect()
}

/// v1's newline-separated environment: `KEY=VALUE` entries, empty lines skipped.
fn split_env(list: &[u8]) -> V1Result<Vec<Vec<u8>>> {
    list.split(|&b| b == b'\n' || b == b'\r')
        .filter(|entry| !entry.is_empty())
        .map(|entry| match entry.contains(&b'=') {
            true => Ok(entry.to_vec()),
            false => shim::fail(Errno::INVAL),
        })
        .collect()
}

fn check_env(env: Vec<Vec<u8>>) -> V1Result<Vec<Vec<u8>>> {
    match env.iter().all(|entry| entry.contains(&b'=')) {
        true => Ok(env),
        false => shim::fail(Errno::INVAL),
    }
}

// ---- exec ----

/// A program to run: what `proc_exec*` and `proc_spawn*` decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Command {
    pub(crate) name: Vec<u8>,
    pub(crate) args: Vec<Vec<u8>>,
    /// `None`: the environment the process started with.
    pub(crate) env: Option<Vec<Vec<u8>>>,
    /// The search path, when the caller asked for a search.
    pub(crate) path: Option<Vec<u8>>,
}

impl Command {
    fn new(
        name: Vec<u8>,
        mut args: Vec<Vec<u8>>,
        env: Option<Vec<Vec<u8>>>,
        search: bool,
        path: Option<Vec<u8>>,
    ) -> Self {
        // POSIX programs expect an `argv[0]`.
        if args.is_empty() {
            args.push(name.clone());
        }
        let path = search.then(|| path.unwrap_or_else(|| DEFAULT_PATH.to_vec()));
        Self {
            name,
            args,
            env,
            path,
        }
    }

    /// The files `execve` tries, in order, as musl's `execvp` builds them.
    fn candidates(&self) -> Vec<Vec<u8>> {
        match &self.path {
            Some(path) if !self.name.contains(&b'/') && !self.name.is_empty() => path
                .split(|&b| b == b':')
                .map(|dir| {
                    let mut file = dir.to_vec();
                    if !dir.is_empty() {
                        file.push(b'/');
                    }
                    file.extend_from_slice(&self.name);
                    file
                })
                .collect(),
            _ => vec![self.name.clone()],
        }
    }
}

/// The `name, args, envs, search_path, path` arguments of the newline-list calls.
#[allow(clippy::too_many_arguments)]
async fn command_from_lists<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    name: (i32, i32),
    args: (i32, i32),
    envs: (i32, i32),
    search: bool,
    path: (i32, i32),
) -> V1Result<Command> {
    let name = read_bytes(frame, name.0 as u32, name.1 as u32).await?;
    let args = split_args(
        &read_optional(frame, args.0, args.1)
            .await?
            .unwrap_or_default(),
    );
    let env = match read_optional(frame, envs.0, envs.1).await? {
        Some(list) => Some(split_env(&list)?),
        None => None,
    };
    let path = read_optional(frame, path.0, path.1).await?;
    Ok(Command::new(name, args, env, search, path))
}

/// The same with argument and environment arrays (`proc_exec4`, `proc_spawn3`).
async fn command_from_arrays<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    name: (i32, i32),
    args: (i32, i32),
    envs: (i32, i32),
    search: bool,
    path: (i32, i32),
) -> V1Result<Command> {
    let name = read_bytes(frame, name.0 as u32, name.1 as u32).await?;
    let args = read_cstr_array(frame, args.0 as u32, args.1 as u32).await?;
    let env = match envs.0 {
        0 => None,
        ptrs => Some(check_env(
            read_cstr_array(frame, ptrs as u32, envs.1 as u32).await?,
        )?),
    };
    let path = read_optional(frame, path.0, path.1).await?;
    Ok(Command::new(name, args, env, search, path))
}

/// `execve`s `command`, searching its path as musl's `execvp` does: a missing file or
/// directory tries the next candidate, a permission error is remembered, anything else ends
/// the search. Returns only on failure, or on success with the image change pending.
async fn exec<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    command: &Command,
) -> V1Result<()> {
    let env = match &command.env {
        Some(env) => env.clone(),
        None => frame.startup().await?.env.clone(),
    };
    fn slices(strings: &[Vec<u8>]) -> Vec<&[u8]> {
        strings.iter().map(Vec::as_slice).collect()
    }
    let argv = frame.mem.alloc_cstr_array(&slices(&command.args))?;
    let envp = frame.mem.alloc_cstr_array(&slices(&env))?;
    let mut denied = false;
    for candidate in command.candidates() {
        let file = frame.mem.alloc_cstr(&candidate)?;
        match frame
            .syscalls
            .execve(&frame.caller(), file, argv, envp)
            .await
        {
            Ok(_) => return Ok(()),
            Err(SysErr::Errno(LinuxErrno::EACCES)) => denied = true,
            Err(SysErr::Errno(LinuxErrno::ENOENT | LinuxErrno::ENOTDIR))
                if command.path.is_some() => {}
            Err(err) => return Err(err.into()),
        }
    }
    Err(match denied {
        true => LinuxErrno::EACCES,
        false => LinuxErrno::ENOENT,
    }
    .into())
}

/// `exec` for the calls without a result: a failure ends the process.
async fn exec_or_exit<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    command: &Command,
) -> V1Result<()> {
    match exec(frame, command).await {
        Err(Error::Sys(SysErr::Restart | SysErr::RestartNoHand)) => {
            Err(Error::Sys(SysErr::Restart))
        }
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::debug!(?err, name = %String::from_utf8_lossy(&command.name), "proc_exec failed");
            frame
                .syscalls
                .exit_group(&frame.caller(), EXEC_FAILED)
                .await?;
            Ok(())
        }
    }
}

/// `proc_exec(name, args)`: never returns, except in a vfork parent.
pub(crate) async fn proc_exec<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
) -> Result<(), RuntimeError> {
    shim::void(&env, "proc_exec", async |frame| {
        let command = command_from_lists(
            frame,
            (name, name_len),
            (args, args_len),
            (0, 0),
            false,
            (0, 0),
        )
        .await?;
        exec_or_exit(frame, &command).await
    })
    .await
}

/// `proc_exec2(name, args, envs)`.
pub(crate) async fn proc_exec2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
    envs: i32,
    envs_len: i32,
) -> Result<(), RuntimeError> {
    shim::void(&env, "proc_exec2", async |frame| {
        let command = command_from_lists(
            frame,
            (name, name_len),
            (args, args_len),
            (envs, envs_len),
            false,
            (0, 0),
        )
        .await?;
        exec_or_exit(frame, &command).await
    })
    .await
}

/// `proc_exec3(name, args, envs, search_path, path)`: returns only on failure, or with 0 in
/// a vfork parent.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proc_exec3<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
    envs: i32,
    envs_len: i32,
    search_path: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let command = command_from_lists(
            frame,
            (name, name_len),
            (args, args_len),
            (envs, envs_len),
            search_path as u8 == Bool::TRUE.0,
            (path, path_len),
        )
        .await?;
        exec(frame, &command).await
    })
    .await
}

/// `proc_exec4(name, argv, argc, envp, envc, search_path, path)`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proc_exec4<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
    envs: i32,
    envs_len: i32,
    search_path: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let command = command_from_arrays(
            frame,
            (name, name_len),
            (args, args_len),
            (envs, envs_len),
            search_path as u8 == Bool::TRUE.0,
            (path, path_len),
        )
        .await?;
        exec(frame, &command).await
    })
    .await
}

// ---- fork and vfork ----

/// The v1 half of the instance behind `env`.
async fn with_instance<S: 'static, R>(
    env: &Env<S>,
    f: impl FnOnce(&Wasix1Instance) -> R,
) -> Result<R, RuntimeError> {
    let abi: Arc<dyn AbiInstance<S>> = env
        .read()
        .await
        .data()
        .abi()
        .cloned()
        .ok_or_else(|| RuntimeError::new("a v1 import ran before its instance was bound"))?;
    let instance = abi
        .as_any()
        .downcast_ref::<Wasix1Instance>()
        .ok_or_else(|| RuntimeError::new("v1 imports run in v1 instances"))?;
    Ok(f(instance))
}

/// Whether the calling task is a vfork child. v1 refuses to vfork again from one (`NOTSUP`):
/// wasix-libc keeps two vfork points only (the one to restore and a free one).
async fn in_vfork_child<S: 'static>(env: &Env<S>) -> Result<bool, RuntimeError> {
    let task = env.read().await.data().task();
    with_instance(env, |instance| instance.is_vfork_child(task)).await
}

/// Starts a vfork child of the calling task: the instance runs as the child afterwards.
/// Returns the parent's compat state, or the errno of a failed `clone`.
async fn vfork<S: Syscalls + Control + 'static>(
    env: &Env<S>,
) -> Result<Result<Arc<ProcState>, i32>, RuntimeError> {
    let mut parent = None;
    let done = shim::call(env, async |frame| {
        parent = Some(frame.proc().await?);
        frame
            .syscalls
            .clone(&frame.caller(), VFORK, 0, S32Ptr::NULL, 0, S32Ptr::NULL)
            .await?;
        Ok(())
    })
    .await?;
    match done {
        Done::Switched(_) => Ok(Ok(parent.expect("read before the clone"))),
        Done::Err(errno) => Ok(Err(i32::from(errno.0))),
        Done::Ok(()) | Done::Unwinding => Err(RuntimeError::new(
            "vfork returned without switching to the child",
        )),
    }
}

/// In a vfork child that just started: registers its copy of `parent`'s compat state, and
/// how the parent resumes. Returns the child's pid.
fn start_vfork_child<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    parent: &ProcState,
    resume: Option<wasixv2_exec::asyncify::StackId>,
) -> TaskId {
    let child = frame.task;
    let state = Arc::new(parent.fork());
    frame.instance().procs().replace(child, state.clone());
    frame.instance().add_vfork(child, Vfork { state, resume });
    child
}

/// `proc_fork_env(pid_out)`: vfork without moving control flow (see the module docs). Returns
/// 0 to the child; the parent finds the child's pid at `pid_out` after its longjmp.
pub(crate) async fn proc_fork_env<S: Syscalls + Control + 'static>(
    env: Env<S>,
    pid_out: i32,
) -> Result<i32, RuntimeError> {
    if in_vfork_child(&env).await? {
        return Ok(i32::from(Errno::NOTSUP.0));
    }
    let parent = match vfork(&env).await? {
        Ok(parent) => parent,
        Err(errno) => return Ok(errno),
    };
    shim::errno(&env, async |frame| {
        let child = start_vfork_child(frame, &parent, None);
        frame.write(pid_out as u32, &child.0).await
    })
    .await
}

/// `proc_fork(copy_memory, pid_out)`: fork, or vfork without a memory copy (see the module
/// docs). Both need the asyncify build configuration; others get `NOSYS` from the kernel for
/// a fork and `NOTSUP` for a vfork, which they do with `proc_fork_env`.
pub(crate) async fn proc_fork<S: Syscalls + Control + 'static>(
    env: Env<S>,
    copy_memory: i32,
    pid_out: i32,
) -> Result<i32, RuntimeError> {
    let pid_out = pid_out as u32;
    let write_pid =
        async |pid: u32| shim::errno(&env, async |frame| frame.write(pid_out, &pid).await).await;
    match import::resumed(&env).await? {
        None => {}
        // The fork parent, with the child running on its own copy.
        Some(Resumed::Forked {
            child: Some(child), ..
        }) => return write_pid(child.0).await,
        // The fork child, on a fresh instance holding the parent's memory and stack.
        Some(Resumed::Forked {
            child: None,
            payload,
        }) => {
            let state = payload
                .and_then(|payload| payload.downcast::<ProcState>().ok())
                .ok_or_else(|| RuntimeError::new("a v1 fork child without its parent's state"))?;
            // The state carries the startup (and preopens) the child inherited.
            with_instance(&env, |instance| instance.set_started()).await?;
            let state = Some(state);
            return shim::errno(&env, async |frame| {
                let pid = frame.syscalls.getpid(&frame.caller()).await?;
                if let Some(state) = &state {
                    frame.instance().adopt(TaskId(pid as u32), state.clone());
                }
                frame.write(pid_out, &0u32).await
            })
            .await;
        }
        // A vfork parent: the child ended, and the stack saved at the vfork point is back.
        Some(Resumed::Restored(child)) => return write_pid(child as u32).await,
        // The vfork point is saved: become the child.
        Some(Resumed::Saved(point)) => {
            let parent = match vfork(&env).await? {
                Ok(parent) => parent,
                Err(errno) => return Ok(errno),
            };
            return shim::errno(&env, async |frame| {
                start_vfork_child(frame, &parent, Some(point));
                frame.write(pid_out, &0u32).await
            })
            .await;
        }
    }
    if copy_memory as u8 == Bool::TRUE.0 {
        // The guest unwinds once the kernel made the child, and calls again on both sides.
        return shim::errno(&env, async |frame| {
            let state = frame.proc().await?.fork();
            import::set_fork_payload(frame.env, Arc::new(state)).await;
            frame
                .syscalls
                .clone(&frame.caller(), SIGCHLD, 0, S32Ptr::NULL, 0, S32Ptr::NULL)
                .await?;
            Ok(())
        })
        .await;
    }
    if !import::instrumented(&env).await || in_vfork_child(&env).await? {
        return Ok(i32::from(Errno::NOTSUP.0));
    }
    // Save the vfork point under `pid_out`, an address unique to this call.
    import::save(&env, pid_out).await?;
    Ok(0)
}

// ---- spawn ----

/// v1 open flags of a `proc_spawn` open action, as Linux `open` flags.
fn open_flags(op: &ProcSpawnFdOp) -> OpenFlags {
    let read = op.fs_rights_base & Rights::FD_READ.0 != 0;
    let write = op.fs_rights_base & Rights::FD_WRITE.0 != 0;
    let mut flags = match (read, write) {
        (true, true) => OpenFlags::O_RDWR,
        (false, true) => OpenFlags::O_WRONLY,
        _ => OpenFlags::O_RDONLY,
    };
    let mut add = |on: bool, flag: OpenFlags| {
        if on {
            flags |= flag;
        }
    };
    let oflags = op.oflags;
    add(oflags & Oflags::CREAT.0 != 0, OpenFlags::O_CREAT);
    add(oflags & Oflags::DIRECTORY.0 != 0, OpenFlags::O_DIRECTORY);
    add(oflags & Oflags::EXCL.0 != 0, OpenFlags::O_EXCL);
    add(oflags & Oflags::TRUNC.0 != 0, OpenFlags::O_TRUNC);
    let fdflags = op.fdflags;
    add(fdflags & Fdflags::APPEND.0 != 0, OpenFlags::O_APPEND);
    add(fdflags & Fdflags::DSYNC.0 != 0, OpenFlags::O_DSYNC);
    add(fdflags & Fdflags::NONBLOCK.0 != 0, OpenFlags::O_NONBLOCK);
    // Linux's O_RSYNC is O_SYNC.
    add(
        fdflags & (Fdflags::RSYNC.0 | Fdflags::SYNC.0) != 0,
        OpenFlags::O_SYNC,
    );
    add(
        op.fdflagsext & Fdflagsext::CLOEXEC.0 != 0,
        OpenFlags::O_CLOEXEC,
    );
    add(
        op.dirflags & Lookupflags::SYMLINK_FOLLOW.0 == 0,
        OpenFlags::O_NOFOLLOW,
    );
    flags
}

/// Runs one file action as the spawned child, as musl's `posix_spawn` child does.
async fn file_action<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    op: &ProcSpawnFdOp,
) -> V1Result<()> {
    let caller = frame.caller();
    let fd = Fd(op.fd as i32);
    let path = async || -> V1Result<_> {
        let path = read_bytes(frame, op.path, op.path_len).await?;
        Ok(frame.mem.alloc_cstr(&path)?)
    };
    match op.cmd {
        // musl ignores a failed close.
        ProcSpawnFdOpName::CLOSE => {
            let _ = frame.syscalls.close(&caller, fd).await;
        }
        ProcSpawnFdOpName::DUP2 if op.src_fd == op.fd => {
            // dup2 onto itself keeps the descriptor across exec.
            let flags = frame
                .syscalls
                .fcntl64(&caller, fd, FcntlCmd::F_GETFD, 0)
                .await?;
            frame
                .syscalls
                .fcntl64(&caller, fd, FcntlCmd::F_SETFD, (flags & !1) as u32)
                .await?;
        }
        ProcSpawnFdOpName::DUP2 => {
            frame
                .syscalls
                .dup3(&caller, Fd(op.src_fd as i32), fd, OpenFlags(0))
                .await?;
        }
        ProcSpawnFdOpName::OPEN => {
            let path = path().await?;
            let at = Fd(AtFd::AT_FDCWD.0);
            let opened = frame
                .syscalls
                .openat(&caller, at, path, open_flags(op), 0o666)
                .await?;
            if opened != fd {
                frame
                    .syscalls
                    .dup3(&caller, opened, fd, OpenFlags(0))
                    .await?;
                frame.syscalls.close(&caller, opened).await?;
            }
        }
        ProcSpawnFdOpName::CHDIR => {
            let path = path().await?;
            frame.syscalls.chdir(&caller, path).await?;
        }
        ProcSpawnFdOpName::FCHDIR => {
            frame.syscalls.fchdir(&caller, fd).await?;
        }
        _ => return shim::fail(Errno::INVAL),
    }
    Ok(())
}

/// What `proc_spawn*` does in the child before `execve`.
struct Spawn {
    command: Command,
    file_actions: Vec<ProcSpawnFdOp>,
    dispositions: Vec<SignalDisposition>,
}

async fn read_spawn<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    command: Command,
    fd_ops: (i32, i32),
    dispositions: (i32, i32),
) -> V1Result<Spawn> {
    let mut file_actions = Vec::new();
    for i in 0..fd_ops.1 as u32 {
        let addr = (fd_ops.0 as u32).wrapping_add(i * ProcSpawnFdOp::SIZE as u32);
        file_actions.push(frame.read::<ProcSpawnFdOp>(addr).await?);
    }
    let mut signals = Vec::new();
    for i in 0..dispositions.1 as u32 {
        let addr = (dispositions.0 as u32).wrapping_add(i * SignalDisposition::SIZE as u32);
        signals.push(frame.read::<SignalDisposition>(addr).await?);
    }
    Ok(Spawn {
        command,
        file_actions,
        dispositions: signals,
    })
}

/// The child's part of a spawn: file actions in order, then the dispositions (later entries
/// win), then `execve`.
async fn spawn_child<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    spawn: &Spawn,
) -> V1Result<()> {
    for op in &spawn.file_actions {
        file_action(frame, op).await?;
    }
    for disposition in &spawn.dispositions {
        let Some(sig) = disposition.sig.to_linux() else {
            continue;
        };
        if disposition.sig == Signal::SIGKILL || disposition.sig == Signal::SIGSTOP {
            continue;
        }
        let handler = match disposition.disp {
            Disposition::IGNORE => SignalHandler::SIG_IGN,
            _ => SignalHandler::SIG_DFL,
        };
        signal::set_handler(frame, sig.0, handler.0 as u32, SaFlags(0)).await?;
    }
    exec(frame, &spawn.command).await
}

/// `posix_spawn` in one call (see the module docs): writes the child's pid to `pid_out`, or
/// fails with the errno of the step that failed in the child, which is reaped then.
async fn spawn<S: Syscalls + Control + 'static>(
    env: &Env<S>,
    read: impl AsyncFnOnce(&Frame<'_, S>) -> V1Result<Spawn>,
    pid_out: i32,
) -> Result<i32, RuntimeError> {
    let mut request = None;
    let mut read = Some(read);
    // Decode everything while still the parent.
    let done = shim::call(env, async |frame| {
        if let Some(read) = read.take() {
            request = Some(read(frame).await?);
        }
        Ok(())
    })
    .await?;
    if let Done::Err(errno) = done {
        return Ok(i32::from(errno.0));
    }
    let request = request.expect("decoded above");
    let parent = match vfork(env).await? {
        Ok(parent) => parent,
        Err(errno) => return Ok(errno),
    };
    let mut child = None;
    let done = shim::call(env, async |frame| {
        if child.is_none() {
            child = Some(start_vfork_child(frame, &parent, None));
        }
        spawn_child(frame, &request).await
    })
    .await?;
    let child = child.expect("set as the child");
    match done {
        // `execve` succeeded and the parent resumed.
        Done::Switched(_) => {
            shim::errno(env, async |frame| {
                frame.write(pid_out as u32, &child.0).await
            })
            .await
        }
        // Still the child: end it, which resumes the parent, and reap it.
        Done::Err(errno) => {
            let exited = shim::call(env, async |frame| {
                frame
                    .syscalls
                    .exit_group(&frame.caller(), EXEC_FAILED)
                    .await?;
                Ok(())
            })
            .await?;
            if !matches!(exited, Done::Switched(_)) {
                return Err(RuntimeError::new("a spawned child did not end"));
            }
            shim::call(env, async |frame| {
                let info = frame.mem.alloc_mut::<Siginfo>()?;
                frame
                    .syscalls
                    .waitid(
                        &frame.caller(),
                        Idtype::P_PID,
                        child.0 as i32,
                        info,
                        WaitOptions::WEXITED,
                        RusagePtr::NULL,
                    )
                    .await?;
                Ok(())
            })
            .await?;
            Ok(i32::from(errno.0))
        }
        Done::Ok(()) | Done::Unwinding => Err(RuntimeError::new(
            "a spawned child's execve returned success",
        )),
    }
}

/// `proc_spawn2(name, args, envs, fd_ops, signal_dispositions, search_path, path, pid_out)`,
/// with newline-separated arguments and environment.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proc_spawn2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
    envs: i32,
    envs_len: i32,
    fd_ops: i32,
    fd_ops_len: i32,
    signals: i32,
    signals_len: i32,
    search_path: i32,
    path: i32,
    path_len: i32,
    pid_out: i32,
) -> Result<i32, RuntimeError> {
    let read = async |frame: &Frame<'_, S>| {
        let command = command_from_lists(
            frame,
            (name, name_len),
            (args, args_len),
            (envs, envs_len),
            search_path as u8 == Bool::TRUE.0,
            (path, path_len),
        )
        .await?;
        read_spawn(frame, command, (fd_ops, fd_ops_len), (signals, signals_len)).await
    };
    spawn(&env, read, pid_out).await
}

/// `proc_spawn3`: `proc_spawn2` with argument and environment arrays.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proc_spawn3<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
    args: i32,
    args_len: i32,
    envs: i32,
    envs_len: i32,
    fd_ops: i32,
    fd_ops_len: i32,
    signals: i32,
    signals_len: i32,
    search_path: i32,
    path: i32,
    path_len: i32,
    pid_out: i32,
) -> Result<i32, RuntimeError> {
    let read = async |frame: &Frame<'_, S>| {
        let command = command_from_arrays(
            frame,
            (name, name_len),
            (args, args_len),
            (envs, envs_len),
            search_path as u8 == Bool::TRUE.0,
            (path, path_len),
        )
        .await?;
        read_spawn(frame, command, (fd_ops, fd_ops_len), (signals, signals_len)).await
    };
    spawn(&env, read, pid_out).await
}

// ---- join ----

/// The child and the v1 status `waitid` reported in `info`; pid 0 when nothing changed
/// (`WNOHANG`).
fn join_status(info: &Siginfo) -> (u32, JoinStatus) {
    let word = |at: usize| i32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    let (pid, status) = (word(0) as u32, word(8));
    let mut join = JoinStatus {
        tag: JoinStatusType::NOTHING,
        code: Errno(0),
        signal: Signal::NONE,
    };
    if pid == 0 {
        return (0, join);
    }
    match SiCode(info.si_code) {
        SiCode::CLD_EXITED => {
            join.tag = JoinStatusType::EXIT_NORMAL;
            join.code = Errno((status & 0xff) as u16);
        }
        SiCode::CLD_KILLED | SiCode::CLD_DUMPED => {
            join.tag = JoinStatusType::EXIT_SIGNAL;
            join.signal = Signal(status as u8);
        }
        SiCode::CLD_STOPPED | SiCode::CLD_TRAPPED => {
            // `stopped` holds the signal where the other variants hold the code.
            join.tag = JoinStatusType::STOPPED;
            join.code = Errno(u16::from(status as u8));
        }
        _ => {}
    }
    (pid, join)
}

/// `proc_join(pid, flags, status)`: `waitid` for the child `pid` names (any child for
/// `NONE`), which it then holds; with `NON_BLOCKING` and no change, pid 0 and `NOTHING`.
pub(crate) async fn proc_join<S: Syscalls + Control + 'static>(
    env: Env<S>,
    pid_ptr: i32,
    flags: i32,
    status_ptr: i32,
) -> Result<i32, RuntimeError> {
    let flags = JoinFlags(flags as u32);
    shim::errno(&env, async |frame| {
        let wanted: OptionPid = frame.read(pid_ptr as u32).await?;
        let (which, id) = match wanted.tag {
            OptionTag::NONE => (Idtype::P_ALL, 0),
            OptionTag::SOME => (Idtype::P_PID, wanted.pid as i32),
            _ => return shim::fail(Errno::INVAL),
        };
        let mut options = WaitOptions::WEXITED;
        if flags.0 & JoinFlags::NON_BLOCKING.0 != 0 {
            options = WaitOptions(options.0 | WaitOptions::WNOHANG.0);
        }
        if flags.0 & JoinFlags::WAKE_STOPPED.0 != 0 {
            options = WaitOptions(options.0 | WaitOptions::WSTOPPED.0);
        }
        let info = frame.mem.alloc_mut::<Siginfo>()?;
        frame
            .syscalls
            .waitid(&frame.caller(), which, id, info, options, RusagePtr::NULL)
            .await?;
        let (pid, status) = join_status(&frame.mem.take(info));
        let pid = OptionPid {
            tag: OptionTag::SOME,
            pid,
        };
        frame.write(pid_ptr as u32, &pid).await?;
        frame.write(status_ptr as u32, &status).await
    })
    .await
}

/// `proc_parent(pid, out)`: the caller's parent. Linux answers this for other processes only
/// through `/proc`, which the kernel does not have: `NOTSUP` for them.
pub(crate) async fn proc_parent<S: Syscalls + Control + 'static>(
    env: Env<S>,
    pid: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        if frame.syscalls.getpid(&caller).await? != pid {
            return shim::fail(Errno::NOTSUP);
        }
        let parent = frame.syscalls.getppid(&caller).await?;
        frame.write(out as u32, &(parent as u32)).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn argument_lists() {
        assert_eq!(split_args(b"ls\n-l\n"), strings(&["ls", "-l"]));
        assert_eq!(split_args(b"a\n\nb\r\n"), strings(&["a", "", "b"]));
        assert!(split_args(b"\n").is_empty());
        assert_eq!(
            split_env(b"A=1\n\nB=2\n").unwrap(),
            strings(&["A=1", "B=2"])
        );
        assert!(split_env(b"NOVALUE\n").is_err());
    }

    #[test]
    fn path_search_candidates() {
        let command = Command::new(
            b"ls".to_vec(),
            vec![],
            None,
            true,
            Some(b"/bin::/usr/bin".to_vec()),
        );
        assert_eq!(command.args, strings(&["ls"]));
        assert_eq!(
            command.candidates(),
            strings(&["/bin/ls", "ls", "/usr/bin/ls"])
        );
        let direct = Command::new(b"./ls".to_vec(), vec![], None, true, None);
        assert_eq!(direct.candidates(), strings(&["./ls"]));
        let default = Command::new(b"sh".to_vec(), vec![], None, true, None);
        assert_eq!(default.candidates().len(), 3);
        let plain = Command::new(b"sh".to_vec(), vec![], None, false, Some(b"/bin".to_vec()));
        assert_eq!(plain.candidates(), strings(&["sh"]));
    }

    fn sigchld(code: SiCode, pid: u32, status: i32) -> Siginfo {
        let mut info = Siginfo::ZERO;
        info.si_signo = 17;
        info.si_code = code.0;
        info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
        info.sifields[8..12].copy_from_slice(&status.to_le_bytes());
        info
    }

    #[test]
    fn join_statuses() {
        let (pid, status) = join_status(&sigchld(SiCode::CLD_EXITED, 7, 3));
        assert_eq!(
            (pid, status.tag, status.code),
            (7, JoinStatusType::EXIT_NORMAL, Errno(3))
        );
        let (_, status) = join_status(&sigchld(SiCode::CLD_KILLED, 7, 9));
        assert_eq!(
            (status.tag, status.signal),
            (JoinStatusType::EXIT_SIGNAL, Signal(9))
        );
        let (_, status) = join_status(&sigchld(SiCode::CLD_STOPPED, 7, 19));
        assert_eq!(
            (status.tag, status.code),
            (JoinStatusType::STOPPED, Errno(19))
        );
        let (pid, status) = join_status(&Siginfo::ZERO);
        assert_eq!((pid, status.tag), (0, JoinStatusType::NOTHING));
    }

    #[test]
    fn spawn_open_flags() {
        let mut op = ProcSpawnFdOp {
            cmd: ProcSpawnFdOpName::OPEN,
            fd: 1,
            src_fd: 0,
            path: 0,
            path_len: 0,
            dirflags: Lookupflags::SYMLINK_FOLLOW.0,
            oflags: Oflags::CREAT.0 | Oflags::TRUNC.0,
            fs_rights_base: Rights::FD_WRITE.0,
            fs_rights_inheriting: 0,
            fdflags: Fdflags::APPEND.0,
            fdflagsext: Fdflagsext::CLOEXEC.0,
        };
        let flags = open_flags(&op);
        let want = OpenFlags::O_WRONLY
            | OpenFlags::O_CREAT
            | OpenFlags::O_TRUNC
            | OpenFlags::O_APPEND
            | OpenFlags::O_CLOEXEC;
        assert_eq!(flags, want);
        op.fs_rights_base = Rights::FD_READ.0 | Rights::FD_WRITE.0;
        op.dirflags = 0;
        assert!(open_flags(&op).0 & OpenFlags::O_RDWR.0 != 0);
        assert!(open_flags(&op).0 & OpenFlags::O_NOFOLLOW.0 != 0);
    }
}
