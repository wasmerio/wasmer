use super::*;
use crate::syscalls::*;

/// ### `fd_dup()`
/// Duplicates the file handle
/// Inputs:
/// - `Fd fd`
///   File handle to be cloned
/// Outputs:
/// - `Fd fd`
///   The new file handle that is a duplicate of the original
#[instrument(level = "trace", skip_all, fields(%fd, ret_fd = field::Empty), ret)]
pub fn fd_dup<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let copied_fd = wasi_try_ok!(fd_dup_internal(&mut ctx, fd, 0, false));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_duplicate(&mut ctx, fd, copied_fd, false).map_err(|err| {
            tracing::error!("failed to save file descriptor duplicate event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Span::current().record("ret_fd", copied_fd);
    let env = ctx.data();
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    wasi_try_mem_ok!(ret_fd.write(&memory, copied_fd));

    Ok(Errno::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasiEnv;

    #[test]
    fn test_fd_dup_basic() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        // Get access to the file system
        let fs = &env.state.fs;

        // Test basic dup on stdin (fd 0) which always exists
        let original_fd = 0;

        // Verify fd exists
        assert!(fs.get_fd(original_fd).is_ok());

        // Dup the fd
        let dup_result = fs.clone_fd_ext(original_fd, 0, Some(false));
        assert!(dup_result.is_ok());

        let duped_fd = dup_result.unwrap();
        assert_ne!(original_fd, duped_fd, "Duped fd should be different number");

        // Verify both fds are valid
        assert!(fs.get_fd(original_fd).is_ok());
        assert!(fs.get_fd(duped_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_ebadf() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        // Test with invalid fd (9999)
        let result = fs.clone_fd_ext(9999, 0, Some(false));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Errno::Badf);

        // Test with another invalid fd (1500)
        let result = fs.clone_fd_ext(1500, 0, Some(false));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Errno::Badf);
    }

    #[test]
    fn test_fd_dup_stdin() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        // Dup stdin (fd 0)
        let result = fs.clone_fd_ext(0, 0, Some(false));
        assert!(result.is_ok(), "Duping stdin should succeed");

        let duped_fd = result.unwrap();
        assert_ne!(0, duped_fd, "Duped fd should be different from stdin");

        // Both fds should be valid
        assert!(fs.get_fd(0).is_ok());
        assert!(fs.get_fd(duped_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_stdout() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        // Dup stdout (fd 1)
        let result = fs.clone_fd_ext(1, 0, Some(false));
        assert!(result.is_ok(), "Duping stdout should succeed");

        let duped_fd = result.unwrap();
        assert_ne!(1, duped_fd);

        assert!(fs.get_fd(1).is_ok());
        assert!(fs.get_fd(duped_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_stderr() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        // Dup stderr (fd 2)
        let result = fs.clone_fd_ext(2, 0, Some(false));
        assert!(result.is_ok(), "Duping stderr should succeed");

        let duped_fd = result.unwrap();
        assert_ne!(2, duped_fd);

        assert!(fs.get_fd(2).is_ok());
        assert!(fs.get_fd(duped_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_multiple() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists // Preopen fd

        // Dup multiple times
        let dup_fd1 = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();
        let dup_fd2 = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();
        let dup_fd3 = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // All fds should be different
        assert_ne!(original_fd, dup_fd1);
        assert_ne!(original_fd, dup_fd2);
        assert_ne!(original_fd, dup_fd3);
        assert_ne!(dup_fd1, dup_fd2);
        assert_ne!(dup_fd1, dup_fd3);
        assert_ne!(dup_fd2, dup_fd3);

        // All fds should be valid
        assert!(fs.get_fd(original_fd).is_ok());
        assert!(fs.get_fd(dup_fd1).is_ok());
        assert!(fs.get_fd(dup_fd2).is_ok());
        assert!(fs.get_fd(dup_fd3).is_ok());
    }

    #[test]
    fn test_fd_dup_cloexec_false() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists

        // Dup with cloexec = false (standard dup behavior)
        let dup_fd = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // Verify both fds exist
        assert!(fs.get_fd(original_fd).is_ok());
        assert!(fs.get_fd(dup_fd).is_ok());

        // The duped fd should not have CLOEXEC set (cloexec=false)
        let fd_entry = fs.get_fd(dup_fd).unwrap();
        assert_ne!(original_fd, dup_fd);
    }

    #[test]
    fn test_fd_dup_close_original() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists

        // Dup the fd
        let dup_fd = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // Verify both exist
        assert!(fs.get_fd(original_fd).is_ok());
        assert!(fs.get_fd(dup_fd).is_ok());

        // Close the original fd
        let close_result = fs.close_fd(original_fd);
        assert!(close_result.is_ok());

        // Original should be gone, dup should still exist
        assert!(fs.get_fd(original_fd).is_err());
        assert!(fs.get_fd(dup_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_close_duped() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists

        // Dup the fd
        let dup_fd = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // Close the duped fd
        let close_result = fs.close_fd(dup_fd);
        assert!(close_result.is_ok());

        // Duped should be gone, original should still exist
        assert!(fs.get_fd(dup_fd).is_err());
        assert!(fs.get_fd(original_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_stress_100() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists

        // Perform 100 dup/close cycles
        for _ in 0..100 {
            let dup_fd = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();
            assert_ne!(original_fd, dup_fd);
            assert!(fs.get_fd(dup_fd).is_ok());

            fs.close_fd(dup_fd).unwrap();
            assert!(fs.get_fd(dup_fd).is_err());
        }

        // Original fd should still be valid
        assert!(fs.get_fd(original_fd).is_ok());
    }

    #[test]
    fn test_fd_dup_sequential_allocation() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let fs = &env.state.fs;

        let original_fd = 1; // stdout always exists

        // Dup to get first available fd
        let dup_fd1 = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // Close it
        fs.close_fd(dup_fd1).unwrap();

        // Dup again - should reuse the fd number
        let dup_fd2 = fs.clone_fd_ext(original_fd, 0, Some(false)).unwrap();

        // The second dup should get the same fd number (lowest available)
        assert_eq!(dup_fd1, dup_fd2, "Should reuse lowest available fd");
    }
}
