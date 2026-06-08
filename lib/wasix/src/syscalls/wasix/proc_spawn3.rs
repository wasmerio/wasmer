use virtual_mio::block_on;
use wasmer::FromToNativeWasmType;
use wasmer_wasix_types::wasi::ProcSpawnFdOpName;

use super::*;
use crate::{
    VIRTUAL_ROOT_FD, WasiFs,
    os::task::{OwnedTaskStatus, TaskStatus},
    syscalls::*,
};

/// Spawns a new sub-process (posix-spawn style) with proper `WasmPtr<WasmPtr<u8>>` string lists.
///
/// Successor to `proc_spawn2`. `args` and `envs` are pointer arrays of null-terminated
/// strings with `args_len` / `envs_len` as element counts. A null `envs` pointer inherits
/// the current environment.
#[instrument(
    level = "trace",
    skip_all,
    fields(name = field::Empty, full_path = field::Empty, pid = field::Empty, tid = field::Empty, %args_len),
    ret)]
pub fn proc_spawn3<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<WasmPtr<u8, M>, M>,
    args_len: M::Offset,
    envs: WasmPtr<WasmPtr<u8, M>, M>,
    envs_len: M::Offset,
    fd_ops: WasmPtr<ProcSpawnFdOp<M>, M>,
    fd_ops_len: M::Offset,
    signal_actions: WasmPtr<SignalDisposition, M>,
    signal_actions_len: M::Offset,
    search_path: Bool,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    ret: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let memory = unsafe { ctx.data().memory_view(&ctx) };
    let mut name = unsafe { get_input_str_ok!(&memory, name, name_len) };
    Span::current().record("name", name.as_str());
    let args = wasi_try_ok!(read_string_array(&memory, args, args_len));

    let envs = if !envs.is_null() {
        let envs = wasi_try_ok!(read_string_array(&memory, envs, envs_len));
        Some(wasi_try_ok!(parse_env_entries(envs)))
    } else {
        None
    };

    let signals = if !signal_actions.is_null() {
        let signal_actions = wasi_try_mem_ok!(signal_actions.slice(&memory, signal_actions_len));
        let mut vec = Vec::with_capacity(signal_actions.len() as usize);
        for s in wasi_try_mem_ok!(signal_actions.access()).iter() {
            vec.push(*s);
        }
        Some(vec)
    } else {
        None
    };

    let fd_ops = if !fd_ops.is_null() {
        let fd_ops = wasi_try_mem_ok!(fd_ops.slice(&memory, fd_ops_len));
        let mut vec = Vec::with_capacity(fd_ops.len() as usize);
        for s in wasi_try_mem_ok!(fd_ops.access()).iter() {
            vec.push(*s);
        }
        vec
    } else {
        vec![]
    };

    let path = if path.is_null() {
        None
    } else {
        Some(unsafe { get_input_str_ok!(&memory, path, path_len) })
    };

    proc_spawn3_impl(
        ctx,
        &mut name,
        args,
        envs,
        fd_ops,
        signals,
        search_path,
        path.as_deref(),
        ret,
    )
}

pub(crate) fn proc_spawn3_impl<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: &mut String,
    args: Vec<String>,
    envs: Option<Vec<(String, String)>>,
    fd_ops: Vec<ProcSpawnFdOp<M>>,
    signals: Option<Vec<SignalDisposition>>,
    search_path: Bool,
    path: Option<&str>,
    ret: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    let memory = unsafe { ctx.data().memory_view(&ctx) };

    // Convert relative paths into absolute paths
    if search_path == Bool::True && !name.contains('/') {
        let path = if let Some(path) = path {
            path.split(':').collect::<Vec<_>>()
        } else {
            vec!["/usr/local/bin", "/bin", "/usr/bin"]
        };
        let (_, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match find_executable_in_path(&state.fs, inodes, path.iter().map(AsRef::as_ref), name) {
            FindExecutableResult::Found(p) => *name = p,
            FindExecutableResult::AccessError => return Ok(Errno::Access),
            FindExecutableResult::NotFound => return Ok(Errno::Noexec),
        }
    } else if name.starts_with("./") {
        *name = ctx.data().state.fs.relative_path_to_absolute(name.clone());
    }

    Span::current().record("full_path", name.as_str());

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = match ctx.data().fork() {
        Ok(p) => p,
        Err(err) => {
            debug!("could not fork process: {err}");
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };

    {
        let mut inner = ctx.data().process.lock();
        inner.children.push(child_env.process.clone());
    }

    // Setup some properties in the child environment
    let pid = child_env.pid();
    let tid = child_env.tid();
    wasi_try_mem_ok!(ret.write(&memory, pid.raw()));
    Span::current()
        .record("pid", pid.raw())
        .record("tid", tid.raw());

    _prepare_wasi(&mut child_env, Some(args), envs, signals);

    for fd_op in fd_ops {
        wasi_try_ok!(apply_fd_op(&mut child_env, &memory, &fd_op));
    }

    // Create the process and drop the context
    let bin_factory = Box::new(child_env.bin_factory.clone());

    let mut builder = Some(child_env);

    let process = match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut builder) {
        Ok(a) => Ok(a),
        Err(err) => {
            if !err.is_not_found() {
                error!("builtin failed - {}", err);
            }

            let env = builder.take().unwrap();

            // Spawn a new process with this current execution environment
            block_on(bin_factory.spawn(name.clone(), env))
        }
    };

    match process {
        Ok(_) => {
            ctx.data_mut().owned_handles.push(child_handle);
            trace!(child_pid = %pid, "spawned sub-process");
            Ok(Errno::Success)
        }
        Err(err) => {
            let err_exit_code = conv_spawn_err_to_exit_code(&err);

            debug!(child_pid = %pid, "process failed with (err={})", err_exit_code);

            Ok(Errno::Noexec)
        }
    }
}

pub(crate) fn apply_fd_op<M: MemorySize>(
    env: &mut WasiEnv,
    memory: &MemoryView,
    op: &ProcSpawnFdOp<M>,
) -> Result<(), Errno> {
    match op.cmd {
        ProcSpawnFdOpName::Close => {
            if let Ok(fd) = env.state.fs.get_fd(op.fd)
                && !fd.is_stdio
                && fd.inode.is_preopened
            {
                trace!("Skipping close FD action for pre-opened FD ({})", op.fd);
                return Ok(());
            }
            env.state.fs.close_fd(op.fd)
        }
        ProcSpawnFdOpName::Dup2 => {
            let flush_target = env.state.fs.dup2_at(op.src_fd, op.fd)?;
            if let Some(file) = flush_target {
                block_on(WasiFs::flush_file_best_effort(file));
            }
            Ok(())
        }
        ProcSpawnFdOpName::Open => {
            let mut name = unsafe {
                WasmPtr::<u8, M>::new(op.name)
                    .read_utf8_string(memory, op.name_len)
                    .map_err(mem_error_to_wasi)?
            };
            name = env.state.fs.relative_path_to_absolute(name.to_owned());
            match path_open_internal(
                env,
                VIRTUAL_ROOT_FD,
                op.dirflags,
                &name,
                op.oflags,
                op.fs_rights_base,
                op.fs_rights_inheriting,
                op.fdflags,
                op.fdflagsext,
                Some(op.fd),
            ) {
                Err(e) => {
                    tracing::warn!("Failed to open file for posix_spawn: {:?}", e);
                    Err(Errno::Io)
                }
                Ok(Err(e)) => Err(e),
                Ok(Ok(_)) => Ok(()),
            }
        }
        ProcSpawnFdOpName::Chdir => {
            let mut path = unsafe {
                WasmPtr::<u8, M>::new(op.name)
                    .read_utf8_string(memory, op.name_len)
                    .map_err(mem_error_to_wasi)?
            };
            path = env.state.fs.relative_path_to_absolute(path.to_owned());
            chdir_internal(env, &path)
        }
        ProcSpawnFdOpName::Fchdir => {
            let fd = env.state.fs.get_fd(op.fd)?;
            let inode_kind = fd.inode.read();
            match inode_kind.deref() {
                Kind::Dir { path, .. } => {
                    let path = path.to_str().ok_or(Errno::Notsup)?;
                    env.state.fs.set_current_dir(path);
                    Ok(())
                }
                _ => Err(Errno::Notdir),
            }
        }
        _ => Err(Errno::Inval),
    }
}
