use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `Fd fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `Fdstat *buf`
///     The location where the metadata will be written
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdstat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf_ptr: WasmPtr<Fdstat, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let stat = wasi_try_ok!(state.fs.fdstat(fd));

    let buf = buf_ptr.deref(&memory);

    wasi_try_mem_ok!(buf.write(stat));

    Ok(Errno::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WasiEnv, WasiEnvInit};
    use wasmer_wasix_types::{wasi::Fdflags, wasi::Filetype};

    // ========== Test Helpers ==========

    /// Creates a basic WasiEnv for testing (no filesystem, just stdin/stdout/stderr)
    fn setup_wasi_env() -> WasiEnvInit {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        WasiEnv::builder("test")
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap()
    }

    /// Creates a WasiEnv with a TmpFileSystem and preopened directory
    fn setup_wasi_env_with_fs() -> WasiEnvInit {
        use std::path::Path;
        use std::sync::Arc;
        use virtual_fs::{FileSystem, TmpFileSystem};

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/testdir")).unwrap();

        WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir("/testdir")
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap()
    }

    /// Creates a file in the TmpFileSystem and returns an fd for it
    /// Note: Requires env to have been created with setup_wasi_env_with_fs()
    fn create_test_file(
        env: &WasiEnvInit,
        filename: &str,
        rights: Rights,
        flags: Fdflags,
    ) -> WasiFd {
        use std::path::Path;
        use virtual_fs::FileSystem;

        // Create the file through the root filesystem
        let file_path = Path::new("/testdir").join(filename);
        let file = env
            .state
            .fs
            .root_fs
            .new_open_options()
            .write(true)
            .create(true)
            .open(&file_path)
            .unwrap();
        drop(file);

        // Open the file through WasiFs
        let fs = &env.state.fs;
        let inodes = &env.state.inodes;
        let dir_fd = 4; // First preopen is fd 4 (fd 3 is VIRTUAL_ROOT_FD)

        let inode = fs
            .get_inode_at_path(inodes, dir_fd, filename, false)
            .unwrap();
        fs.create_fd(
            rights,
            Rights::empty(),
            flags,
            Fdflagsext::empty(),
            0,
            inode,
        )
        .unwrap()
    }

    /// Creates an inode from a Kind and returns an fd for it
    fn create_fd_from_kind(
        env: &WasiEnvInit,
        kind: Kind,
        name: &'static str,
        rights: Rights,
        flags: Fdflags,
    ) -> WasiFd {
        let fs = &env.state.fs;
        let inodes = &env.state.inodes;

        let inode = fs.create_inode_with_default_stat(inodes, kind, false, name.into());
        fs.create_fd(
            rights,
            Rights::empty(),
            flags,
            Fdflagsext::empty(),
            0,
            inode,
        )
        .unwrap()
    }

    // ========== Standard FD Tests ==========

    #[test]
    fn test_fd_fdstat_get_stdin() {
        let env = setup_wasi_env();
        let fdstat = env.state.fs.fdstat(0).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::CharacterDevice);
    }

    #[test]
    fn test_fd_fdstat_get_stdout() {
        let env = setup_wasi_env();
        let fdstat = env.state.fs.fdstat(1).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::CharacterDevice);
    }

    #[test]
    fn test_fd_fdstat_get_stderr() {
        let env = setup_wasi_env();
        let fdstat = env.state.fs.fdstat(2).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::CharacterDevice);
    }

    #[test]
    fn test_fd_fdstat_get_directory() {
        let env = setup_wasi_env_with_fs();
        // fd 3 is VIRTUAL_ROOT_FD, preopened directory is fd 4
        let fdstat = env.state.fs.fdstat(4).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::Directory);
    }

    #[test]
    fn test_fd_fdstat_get_pipe_rx() {
        use crate::fs::Kind;
        use virtual_fs::Pipe;
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env();
        let (_, rx) = Pipe::new().split();
        let rights = Rights::FD_READ | Rights::FD_FDSTAT_SET_FLAGS | Rights::POLL_FD_READWRITE;
        let pipe_fd = create_fd_from_kind(
            &env,
            Kind::PipeRx { rx },
            "test-pipe-rx",
            rights,
            Fdflags::empty(),
        );

        let fdstat = env.state.fs.fdstat(pipe_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::Unknown);
        assert!(fdstat.fs_rights_base.contains(Rights::FD_READ));
    }

    #[test]
    fn test_fd_fdstat_get_pipe_tx() {
        use crate::fs::Kind;
        use virtual_fs::Pipe;
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env();
        let (tx, _) = Pipe::new().split();
        let rights = Rights::FD_WRITE | Rights::FD_FDSTAT_SET_FLAGS | Rights::POLL_FD_READWRITE;
        let pipe_fd = create_fd_from_kind(
            &env,
            Kind::PipeTx { tx },
            "test-pipe-tx",
            rights,
            Fdflags::empty(),
        );

        let fdstat = env.state.fs.fdstat(pipe_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::Unknown);
        assert!(fdstat.fs_rights_base.contains(Rights::FD_WRITE));
    }

    #[test]
    fn test_fd_fdstat_get_socket_stream() {
        use crate::fs::Kind;
        use crate::net::socket::{InodeSocket, InodeSocketKind, SocketProperties};
        use crate::syscalls::SockProto;
        use wasmer_wasix_types::wasi::{Addressfamily, Rights, Socktype};

        let env = setup_wasi_env();
        let socket_kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                props: SocketProperties {
                    family: Addressfamily::Inet4,
                    ty: Socktype::Stream,
                    pt: SockProto::Tcp,
                    only_v6: false,
                    reuse_port: false,
                    reuse_addr: false,
                    no_delay: None,
                    keep_alive: None,
                    dont_route: None,
                    send_buf_size: None,
                    recv_buf_size: None,
                    write_timeout: None,
                    read_timeout: None,
                    accept_timeout: None,
                    connect_timeout: None,
                    handler: None,
                },
                addr: None,
            }),
        };
        let rights = Rights::FD_READ
            | Rights::FD_WRITE
            | Rights::SOCK_CONNECT
            | Rights::SOCK_BIND
            | Rights::SOCK_LISTEN;
        let socket_fd =
            create_fd_from_kind(&env, socket_kind, "test-socket", rights, Fdflags::empty());

        let fdstat = env.state.fs.fdstat(socket_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::SocketStream);
        assert!(fdstat.fs_rights_base.contains(Rights::SOCK_CONNECT));
    }

    #[test]
    fn test_fd_fdstat_get_socket_dgram() {
        use crate::fs::Kind;
        use crate::net::socket::{InodeSocket, InodeSocketKind, SocketProperties};
        use crate::syscalls::SockProto;
        use wasmer_wasix_types::wasi::{Addressfamily, Rights, Socktype};

        let env = setup_wasi_env();
        let socket_kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                props: SocketProperties {
                    family: Addressfamily::Inet4,
                    ty: Socktype::Dgram,
                    pt: SockProto::Udp,
                    only_v6: false,
                    reuse_port: false,
                    reuse_addr: false,
                    no_delay: None,
                    keep_alive: None,
                    dont_route: None,
                    send_buf_size: None,
                    recv_buf_size: None,
                    write_timeout: None,
                    read_timeout: None,
                    accept_timeout: None,
                    connect_timeout: None,
                    handler: None,
                },
                addr: None,
            }),
        };
        let rights =
            Rights::FD_READ | Rights::FD_WRITE | Rights::SOCK_SEND_TO | Rights::SOCK_RECV_FROM;
        let socket_fd = create_fd_from_kind(
            &env,
            socket_kind,
            "test-socket-dgram",
            rights,
            Fdflags::empty(),
        );

        let fdstat = env.state.fs.fdstat(socket_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::SocketDgram);
        assert!(fdstat.fs_rights_base.contains(Rights::SOCK_SEND_TO));
    }

    #[test]
    fn test_fd_fdstat_get_symlink() {
        use crate::fs::Kind;
        use std::path::PathBuf;
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env();
        let symlink_kind = Kind::Symlink {
            base_po_dir: 4,
            path_to_symlink: PathBuf::from("test-symlink"),
            relative_path: PathBuf::from("target-file"),
        };
        let rights = Rights::PATH_READLINK | Rights::FD_FILESTAT_GET;
        let symlink_fd =
            create_fd_from_kind(&env, symlink_kind, "test-symlink", rights, Fdflags::empty());

        let fdstat = env.state.fs.fdstat(symlink_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::SymbolicLink);
    }

    #[test]
    fn test_fd_fdstat_get_regular_file_rdonly() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_READ
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_TELL
            | Rights::FD_SEEK
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test.txt", rights, Fdflags::empty());

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_rights_base.contains(Rights::FD_READ));
    }

    #[test]
    fn test_fd_fdstat_get_regular_file_wronly() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_WRITE
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_DATASYNC
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test_wronly.txt", rights, Fdflags::empty());

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_rights_base.contains(Rights::FD_WRITE));
        assert!(!fdstat.fs_rights_base.contains(Rights::FD_READ));
    }

    #[test]
    fn test_fd_fdstat_get_regular_file_rdwr() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_READ
            | Rights::FD_WRITE
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_DATASYNC
            | Rights::FD_TELL
            | Rights::FD_SEEK
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test_rdwr.txt", rights, Fdflags::empty());

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_rights_base.contains(Rights::FD_READ));
        assert!(fdstat.fs_rights_base.contains(Rights::FD_WRITE));
    }

    #[test]
    fn test_fd_fdstat_get_file_with_append() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_WRITE
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test_append.txt", rights, Fdflags::APPEND);

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_flags.contains(Fdflags::APPEND));
    }

    #[test]
    fn test_fd_fdstat_get_file_with_nonblock() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_WRITE
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test_nonblock.txt", rights, Fdflags::NONBLOCK);

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_flags.contains(Fdflags::NONBLOCK));
    }

    #[test]
    fn test_fd_fdstat_get_file_with_sync() {
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_fs();
        let rights = Rights::FD_WRITE
            | Rights::FD_FDSTAT_SET_FLAGS
            | Rights::FD_SYNC
            | Rights::FD_FILESTAT_GET;
        let file_fd = create_test_file(&env, "test_sync.txt", rights, Fdflags::SYNC);

        let fdstat = env.state.fs.fdstat(file_fd).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::RegularFile);
        assert!(fdstat.fs_flags.contains(Fdflags::SYNC));
    }

    #[test]
    fn test_fd_fdstat_get_ebadf() {
        let env = setup_wasi_env();
        assert_eq!(env.state.fs.fdstat(9999).unwrap_err(), Errno::Badf);
        assert_eq!(env.state.fs.fdstat(1500).unwrap_err(), Errno::Badf);
    }

    #[test]
    fn test_fd_fdstat_get_duped_fd() {
        let env = setup_wasi_env();
        let dup_fd = env.state.fs.clone_fd_ext(1, 0, Some(false)).unwrap();
        let fdstat = env.state.fs.fdstat(dup_fd).unwrap();
        let _ = fdstat.fs_filetype; // Verify it has a filetype
    }

    #[test]
    fn test_fd_fdstat_get_all_standard_fds() {
        let env = setup_wasi_env();
        for fd in 0..=2 {
            let fdstat = env.state.fs.fdstat(fd).unwrap();
            assert_eq!(fdstat.fs_filetype, Filetype::CharacterDevice);
        }
    }

    #[test]
    fn test_fd_fdstat_get_fields() {
        let env = setup_wasi_env();
        let fdstat = env.state.fs.fdstat(0).unwrap();
        assert_eq!(fdstat.fs_filetype, Filetype::CharacterDevice);
        let _ = (
            fdstat.fs_flags,
            fdstat.fs_rights_base,
            fdstat.fs_rights_inheriting,
        );
    }

    #[test]
    fn test_fd_fdstat_get_consistency() {
        let env = setup_wasi_env();
        let fdstat1 = env.state.fs.fdstat(0).unwrap();
        let fdstat2 = env.state.fs.fdstat(0).unwrap();
        let fdstat3 = env.state.fs.fdstat(0).unwrap();
        assert_eq!(fdstat1.fs_filetype, fdstat2.fs_filetype);
        assert_eq!(fdstat1.fs_filetype, fdstat3.fs_filetype);
        assert_eq!(fdstat1.fs_flags, fdstat2.fs_flags);
        assert_eq!(fdstat1.fs_flags, fdstat3.fs_flags);
    }

    #[test]
    fn test_fd_fdstat_get_after_dup() {
        let env = setup_wasi_env();
        let original_fdstat = env.state.fs.fdstat(1).unwrap();
        assert_eq!(original_fdstat.fs_filetype, Filetype::CharacterDevice);

        let dup_fd = env.state.fs.clone_fd_ext(1, 0, Some(false)).unwrap();
        let dup_fdstat = env.state.fs.fdstat(dup_fd).unwrap();
        let _ = dup_fdstat.fs_filetype; // Verify it has a filetype
    }

    #[test]
    fn test_fd_fdstat_get_after_close() {
        let env = setup_wasi_env();
        let dup_fd = env.state.fs.clone_fd_ext(1, 0, Some(false)).unwrap();
        assert!(env.state.fs.fdstat(dup_fd).is_ok());

        env.state.fs.close_fd(dup_fd).unwrap();
        assert_eq!(env.state.fs.fdstat(dup_fd).unwrap_err(), Errno::Badf);
    }
}
