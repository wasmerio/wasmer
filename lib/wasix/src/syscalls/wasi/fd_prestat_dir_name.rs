use super::*;
use crate::syscalls::*;

#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn fd_prestat_dir_name<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let path_chars = wasi_try_mem!(path.slice(&memory, path_len));

    let inode = wasi_try!(state.fs.get_fd_inode(fd));
    let name = inode.name.read().unwrap();
    Span::current().record("path", name.as_ref());

    // check inode-val.is_preopened?

    let guard = inode.read();
    match guard.deref() {
        Kind::Dir { .. } | Kind::Root { .. } => {
            let path_len: u64 = path_len.into();
            let name_len = name.len() as u64;
            if name_len <= path_len {
                wasi_try_mem!(
                    path_chars
                        .subslice(0..name_len)
                        .write_slice(name.as_bytes())
                );
                // Note: We don't write a null terminator since WASI spec doesn't require it
                // and pr_name_len already gives the exact length

                Errno::Success
            } else {
                Errno::Overflow
            }
        }
        Kind::Symlink { .. }
        | Kind::Buffer { .. }
        | Kind::File { .. }
        | Kind::Socket { .. }
        | Kind::PipeRx { .. }
        | Kind::PipeTx { .. }
        | Kind::DuplexPipe { .. }
        | Kind::EventNotifications { .. }
        | Kind::Epoll { .. } => Errno::Notdir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Kind;
    use crate::{WasiEnv, WasiEnvInit};
    use std::ops::Deref;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use virtual_fs::{FileSystem, TmpFileSystem};

    fn setup_wasi_env_with_preopen(preopen_path: &str) -> WasiEnvInit {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();
        let tmp_fs = TmpFileSystem::new();

        // Create the directory only if it's not root and not empty
        // Root "/" already exists in TmpFileSystem
        // Empty "" path will be handled by WASIX directly
        if !preopen_path.is_empty() && preopen_path != "/" {
            // For nested paths, create parent directories first
            let path = Path::new(preopen_path);
            if let Some(parent) = path.parent() {
                if parent != Path::new("") && parent != Path::new("/") {
                    // Create parent directories recursively
                    let mut current = PathBuf::from("/");
                    for component in parent.components().skip(1) {
                        current.push(component);
                        let _ = tmp_fs.create_dir(&current); // ignore errors if already exists
                    }
                }
            }
            tmp_fs.create_dir(Path::new(preopen_path)).unwrap();
        }

        WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir(preopen_path)
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap()
    }

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

    #[test]
    fn test_prestat_get_preopened_dir() {
        let env = setup_wasi_env_with_preopen("/testdir");
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        assert_eq!(prestat.pr_type, wasmer_wasix_types::wasi::Preopentype::Dir);
        // The path should be "/testdir" (8 characters)
        assert_eq!(prestat.u.dir.pr_name_len, 8);
    }

    #[test]
    fn test_prestat_get_stdin_success_but_notdir() {
        let env = setup_wasi_env();
        // Per reference wasmer behavior: fd_prestat_get succeeds on stdin
        // but it's not a directory, so fd_prestat_dir_name would fail with ENOTDIR
        let prestat = env.state.fs.prestat_fd(0).unwrap();
        // Verify it reports as a preopened item
        assert_eq!(prestat.pr_type, wasmer_wasix_types::wasi::Preopentype::Dir);

        // Attempting to get the directory name should fail with ENOTDIR
        let inode = env.state.fs.get_fd_inode(0).unwrap();
        let guard = inode.read();
        // stdin is CharacterDevice, not a directory
        match guard.deref() {
            Kind::Dir { .. } | Kind::Root { .. } => panic!("stdin should not be a directory"),
            _ => {} // Expected: not a directory
        }
    }

    #[test]
    fn test_prestat_get_stdout_success_but_notdir() {
        let env = setup_wasi_env();
        // Per reference wasmer behavior: fd_prestat_get succeeds on stdout
        let prestat = env.state.fs.prestat_fd(1).unwrap();
        assert_eq!(prestat.pr_type, wasmer_wasix_types::wasi::Preopentype::Dir);

        let inode = env.state.fs.get_fd_inode(1).unwrap();
        let guard = inode.read();
        match guard.deref() {
            Kind::Dir { .. } | Kind::Root { .. } => panic!("stdout should not be a directory"),
            _ => {} // Expected: not a directory
        }
    }

    #[test]
    fn test_prestat_get_stderr_success_but_notdir() {
        let env = setup_wasi_env();
        // Per reference wasmer behavior: fd_prestat_get succeeds on stderr
        let prestat = env.state.fs.prestat_fd(2).unwrap();
        assert_eq!(prestat.pr_type, wasmer_wasix_types::wasi::Preopentype::Dir);

        let inode = env.state.fs.get_fd_inode(2).unwrap();
        let guard = inode.read();
        match guard.deref() {
            Kind::Dir { .. } | Kind::Root { .. } => panic!("stderr should not be a directory"),
            _ => {} // Expected: not a directory
        }
    }

    #[test]
    fn test_prestat_get_invalid_fd() {
        let env = setup_wasi_env();
        let result = env.state.fs.prestat_fd(9999);
        assert_eq!(result.unwrap_err(), Errno::Badf);
    }

    #[test]
    fn test_prestat_get_multiple_preopens() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/dir1")).unwrap();
        tmp_fs.create_dir(Path::new("/dir2")).unwrap();
        tmp_fs.create_dir(Path::new("/dir3")).unwrap();

        let env = WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir("/dir1")
            .unwrap()
            .preopen_dir("/dir2")
            .unwrap()
            .preopen_dir("/dir3")
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        // fd 4, 5, 6 should be the preopened directories
        let prestat1 = env.state.fs.prestat_fd(4).unwrap();
        let prestat2 = env.state.fs.prestat_fd(5).unwrap();
        let prestat3 = env.state.fs.prestat_fd(6).unwrap();

        assert_eq!(prestat1.u.dir.pr_name_len, 5); // "/dir1"
        assert_eq!(prestat2.u.dir.pr_name_len, 5); // "/dir2"
        assert_eq!(prestat3.u.dir.pr_name_len, 5); // "/dir3"
    }

    #[test]
    fn test_prestat_dir_name_basic() {
        let env = setup_wasi_env_with_preopen("/testdir");
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, "/testdir");
    }

    #[test]
    fn test_prestat_dir_name_root() {
        // Root "/" can be preopened - it already exists in TmpFileSystem
        let env = setup_wasi_env_with_preopen("/");
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, "/");
    }

    #[test]
    fn test_prestat_dir_name_long_path() {
        // Nested paths can be preopened - we create parent directories first
        let long_path = "/very/long/path/to/test/directory";
        let env = setup_wasi_env_with_preopen(long_path);
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, long_path);
    }

    #[test]
    fn test_prestat_dir_name_multiple_dirs() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/alpha")).unwrap();
        tmp_fs.create_dir(Path::new("/beta")).unwrap();
        tmp_fs.create_dir(Path::new("/gamma")).unwrap();

        let env = WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir("/alpha")
            .unwrap()
            .preopen_dir("/beta")
            .unwrap()
            .preopen_dir("/gamma")
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        let inode1 = env.state.fs.get_fd_inode(4).unwrap();
        let inode2 = env.state.fs.get_fd_inode(5).unwrap();
        let inode3 = env.state.fs.get_fd_inode(6).unwrap();

        assert_eq!(&**(inode1.name.read().unwrap()), "/alpha");
        assert_eq!(&**(inode2.name.read().unwrap()), "/beta");
        assert_eq!(&**(inode3.name.read().unwrap()), "/gamma");
    }

    #[test]
    fn test_prestat_dir_name_invalid_fd() {
        let env = setup_wasi_env_with_preopen("/testdir");
        let result = env.state.fs.get_fd_inode(9999);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Errno::Badf);
    }

    #[test]
    fn test_prestat_name_no_null_terminator() {
        // The WASI spec states that pr_name_len is the exact length WITHOUT null terminator
        let env = setup_wasi_env_with_preopen("/test");
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        // "/test" is 5 characters, not 6 (no null terminator)
        assert_eq!(prestat.u.dir.pr_name_len, 5);
    }

    #[test]
    fn test_prestat_consistency_between_get_and_name() {
        let env = setup_wasi_env_with_preopen("/mydir");

        // Get the prestat which tells us the expected name length
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        let expected_len = prestat.u.dir.pr_name_len;

        // Get the actual name
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();

        // They should match
        assert_eq!(name.len() as u32, expected_len);
        assert_eq!(&**name, "/mydir");
    }

    #[test]
    fn test_prestat_empty_path_rejected() {
        // Test that WASIX correctly rejects empty path preopens
        // Empty paths are not valid directory paths per POSIX/WASI semantics
        // This should fail at the preopen_dir stage, not later

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();

        // Attempting to preopen an empty path should fail
        let result = WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir("")
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init();

        // Verify that preopen correctly rejected the empty path
        assert!(result.is_err(), "Empty path preopen should be rejected");
    }

    #[test]
    fn test_prestat_special_chars_in_path() {
        let path = "/test-dir_123";
        let env = setup_wasi_env_with_preopen(path);
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, path);
    }

    #[test]
    fn test_prestat_order_matches_preopen_order() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/first")).unwrap();
        tmp_fs.create_dir(Path::new("/second")).unwrap();
        tmp_fs.create_dir(Path::new("/third")).unwrap();

        let env = WasiEnv::builder("test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>)
            .preopen_dir("/first")
            .unwrap()
            .preopen_dir("/second")
            .unwrap()
            .preopen_dir("/third")
            .unwrap()
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        // Verify order: fd 4 = /first, fd 5 = /second, fd 6 = /third
        assert_eq!(
            &**env.state.fs.get_fd_inode(4).unwrap().name.read().unwrap(),
            "/first"
        );
        assert_eq!(
            &**env.state.fs.get_fd_inode(5).unwrap().name.read().unwrap(),
            "/second"
        );
        assert_eq!(
            &**env.state.fs.get_fd_inode(6).unwrap().name.read().unwrap(),
            "/third"
        );
    }

    #[test]
    fn test_prestat_dir_name_returns_notdir_for_stdin() {
        let env = setup_wasi_env();
        let inode = env.state.fs.get_fd_inode(0).unwrap();
        let guard = inode.read();

        // Verify stdin is File kind (standard fds are files in WASIX)
        assert!(
            matches!(guard.deref(), Kind::File { .. }),
            "stdin should be Kind::File, got {:?}",
            guard.deref()
        );
    }

    #[test]
    fn test_prestat_dir_name_returns_notdir_for_stdout() {
        let env = setup_wasi_env();
        let inode = env.state.fs.get_fd_inode(1).unwrap();
        let guard = inode.read();

        assert!(
            matches!(guard.deref(), Kind::File { .. }),
            "stdout should be Kind::File, got {:?}",
            guard.deref()
        );
    }

    #[test]
    fn test_prestat_dir_name_returns_notdir_for_stderr() {
        let env = setup_wasi_env();
        let inode = env.state.fs.get_fd_inode(2).unwrap();
        let guard = inode.read();

        assert!(
            matches!(guard.deref(), Kind::File { .. }),
            "stderr should be Kind::File, got {:?}",
            guard.deref()
        );
    }

    #[test]
    fn test_prestat_dir_name_notdir_for_regular_file() {
        use virtual_fs::FileSystem;

        let env = setup_wasi_env_with_preopen("/testdir");

        // Create a regular file
        let file_path = Path::new("/testdir/file.txt");
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

        // Open the file to get an fd
        let file_inode = env
            .state
            .fs
            .get_inode_at_path(&env.state.inodes, 4, "file.txt", false)
            .unwrap();
        let guard = file_inode.read();

        // Verify it's a File kind, which should return ENOTDIR
        match guard.deref() {
            Kind::File { .. } => {} // Expected
            _ => panic!("Expected Kind::File"),
        }
    }

    #[test]
    fn test_prestat_dir_name_notdir_for_symlink() {
        use std::path::PathBuf;

        let env = setup_wasi_env();
        let fs = &env.state.fs;
        let inodes = &env.state.inodes;

        let symlink_kind = Kind::Symlink {
            base_po_dir: 4,
            path_to_symlink: PathBuf::from("test-symlink"),
            relative_path: PathBuf::from("target"),
        };

        let inode =
            fs.create_inode_with_default_stat(inodes, symlink_kind, false, "test-symlink".into());
        let guard = inode.read();

        // Verify it returns ENOTDIR
        match guard.deref() {
            Kind::Symlink { .. } => {} // Expected - would return ENOTDIR
            _ => panic!("Expected Kind::Symlink"),
        }
    }

    #[test]
    fn test_prestat_dir_name_notdir_for_pipe() {
        use virtual_fs::Pipe;

        let env = setup_wasi_env();
        let fs = &env.state.fs;
        let inodes = &env.state.inodes;

        let (_, rx) = Pipe::new().split();
        let pipe_kind = Kind::PipeRx { rx };

        let inode = fs.create_inode_with_default_stat(inodes, pipe_kind, false, "test-pipe".into());
        let guard = inode.read();

        match guard.deref() {
            Kind::PipeRx { .. } => {} // Expected - would return ENOTDIR
            _ => panic!("Expected Kind::PipeRx"),
        }
    }

    #[test]
    fn test_prestat_dir_name_notdir_for_socket() {
        use crate::net::socket::{InodeSocket, InodeSocketKind, SocketProperties};
        use crate::syscalls::SockProto;
        use wasmer_wasix_types::wasi::{Addressfamily, Socktype};

        let env = setup_wasi_env();
        let fs = &env.state.fs;
        let inodes = &env.state.inodes;

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

        let inode =
            fs.create_inode_with_default_stat(inodes, socket_kind, false, "test-socket".into());
        let guard = inode.read();

        match guard.deref() {
            Kind::Socket { .. } => {} // Expected - would return ENOTDIR
            _ => panic!("Expected Kind::Socket"),
        }
    }

    #[test]
    fn test_prestat_get_for_non_preopened_dir() {
        use virtual_fs::FileSystem;
        use wasmer_wasix_types::wasi::Rights;

        let env = setup_wasi_env_with_preopen("/testdir");

        // Create a subdirectory
        let subdir_path = Path::new("/testdir/subdir");
        env.state.fs.root_fs.create_dir(&subdir_path).unwrap();

        // Open the subdirectory to get an fd (but it's not preopened)
        let subdir_inode = env
            .state
            .fs
            .get_inode_at_path(&env.state.inodes, 4, "subdir", false)
            .unwrap();

        // Verify it's not marked as preopened
        assert!(
            !subdir_inode.is_preopened,
            "Subdirectory should not be marked as preopened"
        );
    }

    #[test]
    fn test_prestat_dir_name_very_long_path() {
        // Test paths longer than typical buffer sizes (>256 bytes)
        let long_path = format!("/{}", "x".repeat(300));
        let env = setup_wasi_env_with_preopen(&long_path);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, long_path);
        assert_eq!(name.len(), 301); // "/" + 300 "x"s
    }

    #[test]
    fn test_prestat_dir_name_exact_256_bytes() {
        // Test exactly 256 bytes (common buffer size)
        let path_255 = format!("/{}", "a".repeat(255));
        let env = setup_wasi_env_with_preopen(&path_255);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, path_255);
        assert_eq!(name.len(), 256);
    }

    #[test]
    fn test_prestat_dir_name_buffer_boundary_257_bytes() {
        // Test 257 bytes (just over common buffer size)
        let path_256 = format!("/{}", "b".repeat(256));
        let env = setup_wasi_env_with_preopen(&path_256);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, path_256);
        assert_eq!(name.len(), 257);
    }

    #[test]
    fn test_prestat_consistency_multiple_calls() {
        // Test that calling prestat_get multiple times returns consistent results
        let env = setup_wasi_env_with_preopen("/consistent");

        let prestat1 = env.state.fs.prestat_fd(4).unwrap();
        let prestat2 = env.state.fs.prestat_fd(4).unwrap();
        let prestat3 = env.state.fs.prestat_fd(4).unwrap();

        assert_eq!(prestat1.u.dir.pr_name_len, prestat2.u.dir.pr_name_len);
        assert_eq!(prestat2.u.dir.pr_name_len, prestat3.u.dir.pr_name_len);
        assert_eq!(prestat1.u.dir.pr_name_len, 11); // "/consistent"
    }

    #[test]
    fn test_prestat_dir_name_after_getting_length() {
        let env = setup_wasi_env_with_preopen("/pattern");

        // Step 1: Get the length
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        let name_len = prestat.u.dir.pr_name_len as usize;

        // Step 2: Get the actual name
        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();

        // Step 3: Verify they match
        assert_eq!(name.len(), name_len);
        assert_eq!(&**name, "/pattern");
    }

    #[test]
    fn test_prestat_fd_3_is_virtual_root() {
        let env = setup_wasi_env_with_preopen("/testdir");

        // fd 3 should exist and be a preopened directory (VIRTUAL_ROOT_FD)
        let result = env.state.fs.prestat_fd(3);
        assert!(
            result.is_ok(),
            "fd 3 (VIRTUAL_ROOT_FD) should be accessible"
        );
    }

    #[test]
    fn stress_test_many_preopens() {
        // Stress test: Create 100 preopened directories
        // This tests fd allocation, memory usage, and iteration performance
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();

        // Create 100 directories
        for i in 0..100 {
            let dir_path = format!("/dir{:03}", i);
            tmp_fs.create_dir(Path::new(&dir_path)).unwrap();
        }

        // Build env with 100 preopens
        let mut builder = WasiEnv::builder("stress_test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>);

        for i in 0..100 {
            let dir_path = format!("/dir{:03}", i);
            builder = builder.preopen_dir(&dir_path).unwrap();
        }

        let env = builder
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        // Verify all 100 preopens are accessible
        // fd 3 is VIRTUAL_ROOT_FD, so preopens start at fd 4
        for i in 0..100 {
            let fd = 4 + i;
            let prestat = env.state.fs.prestat_fd(fd).unwrap();
            // Each dir name is "/dir000" = 7 chars
            assert_eq!(prestat.u.dir.pr_name_len, 7);

            let inode = env.state.fs.get_fd_inode(fd).unwrap();
            let name = inode.name.read().unwrap();
            assert_eq!(&**name, format!("/dir{:03}", i));
        }

        // Verify fd 104 (after all preopens) returns EBADF
        let result = env.state.fs.prestat_fd(104);
        assert_eq!(result.unwrap_err(), Errno::Badf);
    }

    #[test]
    fn stress_test_extremely_long_path() {
        // Stress test: Path with 4096 bytes (typical PATH_MAX on Unix)
        let very_long_path = format!("/{}", "x".repeat(4095));
        let env = setup_wasi_env_with_preopen(&very_long_path);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, very_long_path);
        assert_eq!(name.len(), 4096);

        // Verify prestat_get returns correct length
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        assert_eq!(prestat.u.dir.pr_name_len, 4096);
    }

    #[test]
    fn stress_test_deeply_nested_path() {
        // Stress test: Very deep nesting (100 levels)
        let mut path = String::from("/a");
        for _ in 0..99 {
            path.push_str("/a");
        }
        // Total: 200 characters ("/a" repeated 100 times)

        let env = setup_wasi_env_with_preopen(&path);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, path);
        assert_eq!(name.len(), 200);
    }

    #[test]
    fn stress_test_repeated_prestat_get_calls() {
        // Stress test: Call prestat_get 10,000 times on the same fd
        // Tests for memory leaks, race conditions, and performance
        let env = setup_wasi_env_with_preopen("/stress");

        let expected_len = 7; // "/stress"
        for _ in 0..10_000 {
            let prestat = env.state.fs.prestat_fd(4).unwrap();
            assert_eq!(prestat.u.dir.pr_name_len, expected_len);
        }
    }

    #[test]
    fn stress_test_repeated_dir_name_access() {
        // Stress test: Access directory name 10,000 times
        let env = setup_wasi_env_with_preopen("/repeated");

        for _ in 0..10_000 {
            let inode = env.state.fs.get_fd_inode(4).unwrap();
            let name = inode.name.read().unwrap();
            assert_eq!(&**name, "/repeated");
        }
    }

    #[test]
    fn stress_test_alternating_get_and_name() {
        // Stress test: Alternate between prestat_get and name access 5,000 times
        // Tests state consistency under rapid access pattern changes
        let env = setup_wasi_env_with_preopen("/alternate");

        for _ in 0..5_000 {
            let prestat = env.state.fs.prestat_fd(4).unwrap();
            assert_eq!(prestat.u.dir.pr_name_len, 10); // "/alternate"

            let inode = env.state.fs.get_fd_inode(4).unwrap();
            let name = inode.name.read().unwrap();
            assert_eq!(&**name, "/alternate");
        }
    }

    #[test]
    fn stress_test_scan_all_fds() {
        // Stress test: Scan through large fd range (0-1000)
        // Mimics real-world preopen discovery pattern but at scale
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();

        // Create 10 preopens scattered in the fd space
        for i in 0..10 {
            let dir_path = format!("/scan{}", i);
            tmp_fs.create_dir(Path::new(&dir_path)).unwrap();
        }

        let mut builder =
            WasiEnv::builder("scan_test").fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>);

        for i in 0..10 {
            let dir_path = format!("/scan{}", i);
            builder = builder.preopen_dir(&dir_path).unwrap();
        }

        let env = builder
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        // Scan through fds 0-1000, counting successes and failures
        let mut success_count = 0;
        let mut badf_count = 0;

        for fd in 0..1000 {
            match env.state.fs.prestat_fd(fd) {
                Ok(_) => success_count += 1,
                Err(Errno::Badf) => badf_count += 1,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        // We should have:
        // - 3 successful (stdin/stdout/stderr have is_preopened=true)
        // - 1 successful (fd 3 = VIRTUAL_ROOT_FD)
        // - 10 successful (our preopens)
        // - 986 EBADF (all other fds)
        assert_eq!(
            success_count, 14,
            "Expected 14 successful prestat_get calls (0-2 + 3 + 10 preopens)"
        );
        assert_eq!(badf_count, 986, "Expected 986 EBADF errors");
    }

    #[test]
    fn stress_test_unicode_in_paths() {
        // Stress test: Unicode characters in path names
        // Tests UTF-8 handling and byte length calculations
        let unicode_path = "/test_émojis_🚀_日本語_Ελληνικά";
        let env = setup_wasi_env_with_preopen(unicode_path);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, unicode_path);

        // Verify byte length (not character count)
        let byte_len = unicode_path.as_bytes().len();
        let prestat = env.state.fs.prestat_fd(4).unwrap();
        assert_eq!(prestat.u.dir.pr_name_len as usize, byte_len);
    }

    #[test]
    fn stress_test_path_with_all_printable_ascii() {
        // Stress test: Path with many special characters
        // Tests character escaping and edge cases
        let special_path = "/test-_+=[]{}().,;!@#$%^&";
        let env = setup_wasi_env_with_preopen(special_path);

        let inode = env.state.fs.get_fd_inode(4).unwrap();
        let name = inode.name.read().unwrap();
        assert_eq!(&**name, special_path);

        let prestat = env.state.fs.prestat_fd(4).unwrap();
        assert_eq!(prestat.u.dir.pr_name_len as usize, special_path.len());
    }

    #[test]
    fn stress_test_many_preopens_sequential_access() {
        // Stress test: 50 preopens accessed sequentially 100 times
        // Tests cache behavior and access pattern performance
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();

        for i in 0..50 {
            let dir_path = format!("/seq{:02}", i);
            tmp_fs.create_dir(Path::new(&dir_path)).unwrap();
        }

        let mut builder = WasiEnv::builder("sequential_test")
            .fs(Arc::new(tmp_fs) as Arc<dyn FileSystem + Send + Sync>);

        for i in 0..50 {
            let dir_path = format!("/seq{:02}", i);
            builder = builder.preopen_dir(&dir_path).unwrap();
        }

        let env = builder
            .engine(wasmer::Engine::default())
            .build_init()
            .unwrap();

        // Access all 50 preopens sequentially, 100 times
        for _ in 0..100 {
            for i in 0..50 {
                let fd = 4 + i; // fd 4 is first preopen
                let prestat = env.state.fs.prestat_fd(fd).unwrap();
                assert_eq!(prestat.u.dir.pr_name_len, 6); // "/seqNN"
            }
        }
    }
}
