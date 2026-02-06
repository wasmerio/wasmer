use std::sync::atomic::AtomicUsize;

use virtual_fs::Pipe;

use super::*;
use crate::syscalls::*;

// Used to make pipe end names unique. This is necessary since we use
// a hash of the name to calculate inode numbers. The actual number
// has no other meaning.
static PIPE_NUMBER: AtomicUsize = AtomicUsize::new(0);

/// ### `fd_pipe()`
/// Creates ta pipe that feeds data between two file handles
/// Output:
/// - `Fd`
///     First file handle that represents the read end of the pipe
/// - `Fd`
///     Second file handle that represents the write end of the pipe
#[instrument(level = "trace", skip_all, fields(read_fd = field::Empty, write_fd = field::Empty), ret)]
pub fn fd_pipe<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ro_read_fd: WasmPtr<WasiFd, M>,
    ro_write_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (read_fd, write_fd) = wasi_try_ok!(fd_pipe_internal(&mut ctx, None, None));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_pipe(&mut ctx, read_fd, write_fd).map_err(|err| {
            tracing::error!("failed to save create pipe event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    Span::current()
        .record("read_fd", read_fd)
        .record("write_fd", write_fd);

    wasi_try_mem_ok!(ro_read_fd.write(&memory, read_fd));
    wasi_try_mem_ok!(ro_write_fd.write(&memory, write_fd));

    Ok(Errno::Success)
}

pub fn fd_pipe_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    with_read_fd: Option<WasiFd>,
    with_write_fd: Option<WasiFd>,
) -> Result<(WasiFd, WasiFd), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let (tx, rx) = Pipe::new().split();

    // FIXME: since a hash of the inode name is used to calculate the inode number, this may
    // or may not break journals that include pipes and are compacted.
    let pipe_no = PIPE_NUMBER.fetch_add(1, Ordering::SeqCst);

    let rx_inode = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::PipeRx { rx },
        false,
        format!("pipe{pipe_no}-rx").into(),
    );
    let tx_inode = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::PipeTx { tx },
        false,
        format!("pipe{pipe_no}-tx").into(),
    );

    let rights = Rights::FD_SYNC
        | Rights::FD_DATASYNC
        | Rights::POLL_FD_READWRITE
        | Rights::SOCK_SEND
        | Rights::FD_FDSTAT_SET_FLAGS
        | Rights::FD_FILESTAT_GET;

    let read_rights = rights | Rights::FD_READ;
    let write_rights = rights | Rights::FD_WRITE;

    let read_fd = if let Some(fd) = with_read_fd {
        state
            .fs
            .with_fd(
                read_rights,
                read_rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                rx_inode,
                fd,
            )
            .map(|()| fd)?
    } else {
        state.fs.create_fd(
            read_rights,
            read_rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            rx_inode,
        )?
    };

    let write_fd = if let Some(fd) = with_write_fd {
        state
            .fs
            .with_fd(
                write_rights,
                write_rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                tx_inode,
                fd,
            )
            .map(|()| fd)?
    } else {
        state.fs.create_fd(
            write_rights,
            write_rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            tx_inode,
        )?
    };

    Ok((read_fd, write_fd))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Kind;
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::thread;
    use wasmer::{imports, Instance, Module, Store};

    fn setup_env_with_memory() -> (Store, WasiFunctionEnv) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let mut store = Store::default();
        let mut func_env = WasiEnv::builder("test")
            .engine(wasmer::Engine::default())
            .finalize(&mut store)
            .unwrap();

        // Minimal module exporting memory for syscall memory access.
        let wat = r#"(module (memory (export "memory") 1))"#;
        let module = Module::new(&store, wat).unwrap();
        let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
        func_env.initialize(&mut store, instance).unwrap();

        (store, func_env)
    }

    fn get_pipe_ends(
        store: &Store,
        func_env: &WasiFunctionEnv,
        read_fd: WasiFd,
        write_fd: WasiFd,
    ) -> (virtual_fs::PipeRx, virtual_fs::PipeTx) {
        let env = func_env.data(store);
        let fs = &env.state.fs;

        let read_entry = fs.get_fd(read_fd).unwrap();
        let write_entry = fs.get_fd(write_fd).unwrap();

        let rx = match &*read_entry.inode.read() {
            Kind::PipeRx { rx } => rx.clone(),
            other => panic!("expected PipeRx, got {other:?}"),
        };
        let tx = match &*write_entry.inode.read() {
            Kind::PipeTx { tx } => tx.clone(),
            other => panic!("expected PipeTx, got {other:?}"),
        };

        (rx, tx)
    }

    #[test]
    fn test_pipe_blocking_channel() {
        let (mut store, func_env) = setup_env_with_memory();
        let mut ctx = func_env.env.clone().into_mut(&mut store);
        let (read_fd, write_fd) = fd_pipe_internal(&mut ctx, None, None).unwrap();

        let (mut rx, mut tx) = get_pipe_ends(&store, &func_env, read_fd, write_fd);

        let writer = thread::spawn(move || {
            let data = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
            let mut written = 0;
            while written < data.len() {
                let n = std::io::Write::write(&mut tx, &data[written..]).unwrap();
                written += n;
            }
            tx.close();
            assert_eq!(written, data.len());
        });

        let mut buf = [0u8; 10];
        let mut read = 0;
        while read < buf.len() {
            let n = rx.read(&mut buf[read..]).unwrap();
            assert!(n > 0, "unexpected EOF before full read");
            read += n;
        }
        writer.join().unwrap();
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_pipe_nonblocking_channel() {
        let (mut store, func_env) = setup_env_with_memory();
        let mut ctx = func_env.env.clone().into_mut(&mut store);
        let (read_fd, write_fd) = fd_pipe_internal(&mut ctx, None, None).unwrap();

        let (mut rx, tx) = get_pipe_ends(&store, &func_env, read_fd, write_fd);

        // Fill the pipe until it blocks.
        let mut bytes_written = 0usize;
        loop {
            match tx.try_write_nonblocking(&[0u8; 1024]) {
                Ok(n) => bytes_written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("unexpected write error: {e}")
            }
        }
        assert!(bytes_written > 0);

        // Drain some bytes and ensure we can write again.
        let mut read_buf = [0u8; 2048];
        let mut drained = 0usize;
        loop {
            match rx.try_read(&mut read_buf) {
                Some(n) if n > 0 => {
                    drained += n;
                    break;
                }
                Some(0) => break,
                Some(_) => continue,
                None => continue,
            }
        }
        assert!(drained > 0);

        let n = tx.try_write_nonblocking(&[1u8; 16]).unwrap();
        assert_eq!(n, 16);
    }
}
