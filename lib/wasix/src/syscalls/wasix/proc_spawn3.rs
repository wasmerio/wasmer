use virtual_mio::block_on;
use wasmer_wasix_types::wasi::ProcSpawnFdOpName;

use super::*;
use crate::{VIRTUAL_ROOT_FD, WasiFs, syscalls::*};

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
    wasi_try_mem_ok!(ret.access(&memory));

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, child_handle) = match ctx.data().fork() {
        Ok(p) => p,
        Err(err) => {
            debug!("could not fork process: {err}");
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };

    // Setup some properties in the child environment
    let pid = child_env.pid();
    let tid = child_env.tid();
    let child_process = child_env.process.clone();
    let child_finished = child_env.process.finished.clone();
    let tasks = child_env.tasks().clone();
    Span::current()
        .record("pid", pid.raw())
        .record("tid", tid.raw());

    _prepare_wasi(&mut child_env, Some(args), envs, signals);

    for fd_op in fd_ops {
        wasi_try_ok!(apply_fd_op(&mut child_env, &memory, &fd_op));
    }

    *name = wasi_try_ok!(resolve_spawn_executable(
        &child_env,
        name,
        search_path,
        path
    ));
    Span::current().record("full_path", name.as_str());
    wasi_try_mem_ok!(ret.write(&memory, pid.raw()));

    // Create the process and drop the context
    let bin_factory = Box::new(child_env.bin_factory.clone());

    let mut builder = Some(child_env);

    let process = match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut builder) {
        Ok(task) => {
            if let Err(err) = propagate_virtual_task_completion(&tasks, task, child_finished) {
                return Ok(err.into());
            }
            Ok(())
        }
        Err(err) => {
            if !err.is_not_found() {
                error!("builtin failed - {}", err);
            }

            let env = builder.take().unwrap();

            // Spawn a new process with this current execution environment
            block_on(bin_factory.spawn(name.clone(), env)).map(|_| ())
        }
    };

    match process {
        Ok(_) => {
            {
                let mut inner = ctx.data().process.lock();
                inner.children.push(child_process);
            }
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

fn resolve_spawn_executable(
    env: &WasiEnv,
    name: &str,
    search_path: Bool,
    path: Option<&str>,
) -> Result<String, Errno> {
    if search_path == Bool::True && !name.contains('/') {
        let path = resolve_spawn_search_path(env, path);
        return match find_executable_in_path(
            &env.state.fs,
            &env.state.inodes,
            path.iter().map(AsRef::as_ref),
            name,
        ) {
            FindExecutableResult::Found(path) => Ok(path),
            FindExecutableResult::AccessError => Err(Errno::Access),
            FindExecutableResult::NotFound => Err(Errno::Noent),
        };
    }

    if name.starts_with('/') {
        Ok(name.to_string())
    } else {
        Ok(env.state.fs.relative_path_to_absolute(name.to_string()))
    }
}

fn resolve_spawn_search_path(env: &WasiEnv, path: Option<&str>) -> Vec<String> {
    path.map(|path| path.split(':').collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["/usr/local/bin", "/bin", "/usr/bin"])
        .into_iter()
        .map(|entry| env.state.fs.relative_path_to_absolute(entry.to_string()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer::Engine;

    #[tokio::test]
    async fn spawn_resolution_uses_prepared_child_cwd() {
        let env = WasiEnv::builder("test")
            .engine(Engine::default())
            .build()
            .unwrap();
        env.state.fs.set_current_dir("/child");

        assert_eq!(
            resolve_spawn_executable(&env, "tool", Bool::False, None),
            Ok("/child/tool".to_string())
        );
        assert_eq!(
            resolve_spawn_executable(&env, "./tool", Bool::False, None),
            Ok("/child/./tool".to_string())
        );
        assert_eq!(
            resolve_spawn_executable(&env, "sub/tool", Bool::False, None),
            Ok("/child/sub/tool".to_string())
        );
        assert_eq!(
            resolve_spawn_search_path(&env, Some("bin::/absolute")),
            vec![
                "/child/bin".to_string(),
                "/child/".to_string(),
                "/absolute".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn uncommitted_child_is_finished_without_parent_publication() {
        let parent = WasiEnv::builder("test")
            .engine(Engine::default())
            .build()
            .unwrap();
        let owned_handles = parent.owned_handles.len();
        let children = parent.process.lock().children.len();
        let (child, child_handle) = parent.fork().unwrap();
        let child_process = child.process.clone();

        drop(child);
        drop(child_handle);

        assert_eq!(parent.owned_handles.len(), owned_handles);
        assert_eq!(parent.process.lock().children.len(), children);
        assert!(
            matches!(child_process.try_join(), Some(Ok(code)) if code == Errno::Success.into())
        );
    }
}
