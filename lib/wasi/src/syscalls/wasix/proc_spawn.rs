use wasmer_vfs::Pipe;

use super::*;
use crate::syscalls::*;

/// Spawns a new process within the context of this machine
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `chroot` - Indicates if the process will chroot or not
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
/// * `preopen` - List of the preopens for this process
///   (entries are separated by line feeds)
/// * `stdin` - How will stdin be handled
/// * `stdout` - How will stdout be handled
/// * `stderr` - How will stderr be handled
/// * `working_dir` - Working directory where this process should run
///   (passing '.' will use the current directory)
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn proc_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    chroot: Bool,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
    preopen: WasmPtr<u8, M>,
    preopen_len: M::Offset,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
    working_dir: WasmPtr<u8, M>,
    working_dir_len: M::Offset,
    ret_handles: WasmPtr<BusHandles, M>,
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let control_plane = env.process.control_plane();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus_ok!(&memory, name, name_len) };
    let args = unsafe { get_input_str_bus_ok!(&memory, args, args_len) };
    let preopen = unsafe { get_input_str_bus_ok!(&memory, preopen, preopen_len) };
    let working_dir = unsafe { get_input_str_bus_ok!(&memory, working_dir, working_dir_len) };
    debug!(
        "wasi[{}:{}]::process_spawn (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    if chroot == Bool::True {
        warn!(
            "wasi[{}:{}]::chroot is not currently supported",
            ctx.data().pid(),
            ctx.data().tid()
        );
        return Ok(BusErrno::Unsupported);
    }

    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let preopen: Vec<_> = preopen
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let (handles, ctx) = match proc_spawn_internal(
        ctx,
        name,
        Some(args),
        Some(preopen),
        Some(working_dir),
        stdin,
        stdout,
        stderr,
    )? {
        Ok(a) => a,
        Err(err) => {
            return Ok(err);
        }
    };

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus_ok!(ret_handles.write(&memory, handles));
    Ok(BusErrno::Success)
}

pub fn proc_spawn_internal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    args: Option<Vec<String>>,
    preopen: Option<Vec<String>>,
    working_dir: Option<String>,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
) -> Result<Result<(BusHandles, FunctionEnvMut<'_, WasiEnv>), BusErrno>, WasiError> {
    let env = ctx.data();

    // Build a new store that will be passed to the thread
    let new_store = ctx.data().runtime.new_store();

    // Fork the current environment and set the new arguments
    let (mut child_env, handle) = match ctx.data().fork() {
        Ok(x) => x,
        Err(err) => {
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Err(BusErrno::Denied));
        }
    };
    if let Some(args) = args {
        let mut child_state = env.state.fork();
        child_state.args = args;
        child_env.state = Arc::new(child_state);
    }

    // Take ownership of this child
    ctx.data_mut().owned_handles.push(handle);
    let env = ctx.data();

    // Preopen
    if let Some(preopen) = preopen {
        if !preopen.is_empty() {
            for preopen in preopen {
                warn!(
                    "wasi[{}:{}]::preopens are not yet supported for spawned processes [{}]",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    preopen
                );
            }
            return Ok(Err(BusErrno::Unsupported));
        }
    }

    // Change the current directory
    if let Some(working_dir) = working_dir {
        child_env.state.fs.set_current_dir(working_dir.as_str());
    }

    // Replace the STDIO
    let (stdin, stdout, stderr) = {
        let (_, child_state, child_inodes) =
            child_env.get_memory_and_wasi_state_and_inodes(&new_store, 0);
        let mut conv_stdio_mode = |mode: WasiStdioMode, fd: WasiFd| -> Result<OptionFd, BusErrno> {
            match mode {
                WasiStdioMode::Piped => {
                    let (pipe1, pipe2) = Pipe::channel();
                    let inode1 = child_state.fs.create_inode_with_default_stat(
                        child_inodes,
                        Kind::Pipe { pipe: pipe1 },
                        false,
                        "pipe".into(),
                    );
                    let inode2 = child_state.fs.create_inode_with_default_stat(
                        child_inodes,
                        Kind::Pipe { pipe: pipe2 },
                        false,
                        "pipe".into(),
                    );

                    let rights = crate::net::socket::all_socket_rights();
                    let pipe = ctx
                        .data()
                        .state
                        .fs
                        .create_fd(rights, rights, Fdflags::empty(), 0, inode1)
                        .map_err(|_| BusErrno::Internal)?;
                    child_state
                        .fs
                        .create_fd_ext(rights, rights, Fdflags::empty(), 0, inode2, fd)
                        .map_err(|_| BusErrno::Internal)?;

                    trace!(
                        "wasi[{}:{}]::fd_pipe (fd1={}, fd2={})",
                        ctx.data().pid(),
                        ctx.data().tid(),
                        pipe,
                        fd
                    );
                    Ok(OptionFd {
                        tag: OptionTag::Some,
                        fd: pipe,
                    })
                }
                WasiStdioMode::Inherit => Ok(OptionFd {
                    tag: OptionTag::None,
                    fd: u32::MAX,
                }),
                _ => {
                    child_state.fs.close_fd(fd);
                    Ok(OptionFd {
                        tag: OptionTag::None,
                        fd: u32::MAX,
                    })
                }
            }
        };
        let stdin = match conv_stdio_mode(stdin, 0) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        let stdout = match conv_stdio_mode(stdout, 1) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        let stderr = match conv_stdio_mode(stderr, 2) {
            Ok(a) => a,
            Err(err) => return Ok(Err(err)),
        };
        (stdin, stdout, stderr)
    };

    // Create the new process
    let bin_factory = Box::new(ctx.data().bin_factory.clone());
    let child_pid = child_env.pid();

    let mut new_store = Some(new_store);
    let mut builder = Some(child_env);

    // First we try the built in commands
    let mut process =
        match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut new_store, &mut builder) {
            Ok(a) => a,
            Err(err) => {
                if err != VirtualBusError::NotFound {
                    error!(
                        "wasi[{}:{}]::proc_spawn - builtin failed - {}",
                        ctx.data().pid(),
                        ctx.data().tid(),
                        err
                    );
                }
                // Now we actually spawn the process
                let child_work =
                    bin_factory.spawn(name, new_store.take().unwrap(), builder.take().unwrap());

                match __asyncify(&mut ctx, None, async move {
                    Ok(child_work.await.map_err(vbus_error_into_bus_errno))
                })?
                .map_err(|err| BusErrno::Unknown)
                {
                    Ok(Ok(a)) => a,
                    Ok(Err(err)) => return Ok(Err(err)),
                    Err(err) => return Ok(Err(err)),
                }
            }
        };

    // Add the process to the environment state
    {
        let mut children = ctx.data().process.children.write().unwrap();
        children.push(child_pid);
    }
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    {
        let mut guard = env.process.write();
        guard.bus_processes.insert(child_pid, process);
    };

    let handles = BusHandles {
        bid: child_pid.raw(),
        stdin,
        stdout,
        stderr,
    };
    Ok(Ok((handles, ctx)))
}
