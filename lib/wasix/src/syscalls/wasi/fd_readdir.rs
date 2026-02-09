use super::*;
use crate::fs::Inode;
use std::collections::HashSet;
use crate::syscalls::*;

/// ### `fd_readdir()`
/// Read data from directory specified by file descriptor
/// Inputs:
/// - `Fd fd`
///     File descriptor from which directory data will be read
/// - `void *buf`
///     Buffer where directory entries are stored
/// - `u32 buf_len`
///     Length of data in `buf`
/// - `Dircookie cookie`
///     Where the directory reading should start from
/// Output:
/// - `u32 *bufused`
///     The Number of bytes stored in `buf`; if less than `buf_len` then entire
///     directory has been read
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_readdir<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    cookie: Dircookie,
    bufused: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    // TODO: figure out how this is supposed to work;
    // is it supposed to pack the buffer full every time until it can't? or do one at a time?

    let buf_arr = wasi_try_mem_ok!(buf.slice(&memory, buf_len));
    let bufused_ref = bufused.deref(&memory);
    let working_dir = wasi_try_ok!(state.fs.get_fd(fd));
    let mut cur_cookie = cookie;
    let mut buf_idx = 0usize;

    let entries: Vec<(String, Filetype, u64)> = {
        let guard = working_dir.inode.read();
        match guard.deref() {
            Kind::Dir {
                path,
                entries,
                parent,
                ..
            } => {
                trace!("reading dir {:?}", path);
                let dot_ino = working_dir.inode.stat.read().unwrap().st_ino;
                let dotdot_ino = parent
                    .upgrade()
                    .map(|inode| inode.stat.read().unwrap().st_ino)
                    .unwrap_or(dot_ino);
                // TODO: refactor this code
                // we need to support multiple calls,
                // simple and obviously correct implementation for now:
                // maintain consistent order via lexacographic sorting
                let fs_info = wasi_try_ok!(
                    wasi_try_ok!(state.fs_read_dir(path))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(fs_error_into_wasi_err)
                );
                let mut entry_vec = wasi_try_ok!(
                    fs_info
                        .into_iter()
                        .map(|entry| {
                            let filename = entry.file_name().to_string_lossy().to_string();
                            trace!("getting file: {:?}", filename);
                            let filetype = virtual_file_type_to_wasi_file_type(
                                entry.file_type().map_err(fs_error_into_wasi_err)?,
                            );
                            let ino = entries
                                .get(&filename)
                                .map(|inode| inode.stat.read().unwrap().st_ino)
                                .unwrap_or_else(|| {
                                    Inode::from_path(entry.path().to_string_lossy().as_ref())
                                        .as_u64()
                                });
                            Ok((filename, filetype, ino))
                        })
                        .collect::<Result<Vec<(String, Filetype, u64)>, _>>()
                );
                let mut seen_names: HashSet<String> =
                    entry_vec.iter().map(|(name, _, _)| name.clone()).collect();
                entry_vec.extend(entries.iter().filter_map(|(name, inode)| {
                    if seen_names.contains(name) {
                        return None;
                    }
                    seen_names.insert(name.clone());
                    let stat = inode.stat.read().unwrap();
                    Some((name.clone(), stat.st_filetype, stat.st_ino))
                }));
                // adding . and .. special folders
                // TODO: inode
                entry_vec.push((".".to_string(), Filetype::Directory, dot_ino));
                entry_vec.push(("..".to_string(), Filetype::Directory, dotdot_ino));
                entry_vec.sort_by(|a, b| a.0.cmp(&b.0));
                entry_vec
            }
            Kind::Root { entries } => {
                trace!("reading root");
                let sorted_entries = {
                    let mut entry_vec: Vec<(String, InodeGuard)> = entries
                        .iter()
                        .map(|(a, b)| (a.clone(), b.clone()))
                        .collect();
                    entry_vec.sort_by(|a, b| a.0.cmp(&b.0));
                    entry_vec
                };
                sorted_entries
                    .into_iter()
                    .map(|(name, inode)| {
                        let stat = inode.stat.read().unwrap();
                        (
                            format!("/{}", inode.name.read().unwrap().as_ref()),
                            stat.st_filetype,
                            stat.st_ino,
                        )
                    })
                    .collect()
            }
            Kind::File { .. }
            | Kind::Symlink { .. }
            | Kind::Buffer { .. }
            | Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Ok(Errno::Notdir),
        }
    };

    let buf_len_u64: u64 = buf_len.into();
    if buf_len_u64 < std::mem::size_of::<Dirent>() as u64 {
        let zero = wasi_try_ok!(to_offset::<M>(0));
        wasi_try_mem_ok!(bufused_ref.write(zero));
        return Ok(Errno::Inval);
    }

    for (entry_path_str, wasi_file_type, ino) in entries.iter().skip(cookie as usize) {
        cur_cookie += 1;
        let namlen = entry_path_str.len();
        trace!("returning dirent for {}", entry_path_str);
        let dirent = Dirent {
            d_next: cur_cookie,
            d_ino: *ino,
            d_namlen: namlen as u32,
            d_type: *wasi_file_type,
        };
        let dirent_bytes = dirent_to_le_bytes(&dirent);
        let upper_limit = std::cmp::min(
            (buf_len_u64 - buf_idx as u64) as usize,
            std::mem::size_of::<Dirent>(),
        );
        for (i, b) in dirent_bytes.iter().enumerate().take(upper_limit) {
            wasi_try_mem_ok!(buf_arr.index((i + buf_idx) as u64).write(*b));
        }
        buf_idx += upper_limit;
        if upper_limit != std::mem::size_of::<Dirent>() {
            break;
        }
        let upper_limit = std::cmp::min((buf_len_u64 - buf_idx as u64) as usize, namlen);
        for (i, b) in entry_path_str.bytes().take(upper_limit).enumerate() {
            wasi_try_mem_ok!(buf_arr.index((i + buf_idx) as u64).write(b));
        }
        buf_idx += upper_limit;
        if upper_limit != namlen {
            break;
        }
    }

    let buf_idx: M::Offset = wasi_try_ok!(buf_idx.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(bufused_ref.write(buf_idx));
    Ok(Errno::Success)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wasmer::{imports, Instance, Module, Store};
    use wasmer::FromToNativeWasmType;
    use virtual_fs::TmpFileSystem;

    fn setup_env_with_tmpfs() -> (Store, WasiFunctionEnv, WasiFd) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let tmp_fs = TmpFileSystem::new();
        tmp_fs.create_dir(Path::new("/sandbox")).unwrap();
        let mut store = Store::default();
        let mut func_env = WasiEnv::builder("test")
            .engine(wasmer::Engine::default())
            .fs(Arc::new(tmp_fs) as Arc<dyn virtual_fs::FileSystem + Send + Sync>)
            .preopen_dir("/sandbox")
            .unwrap()
            .finalize(&mut store)
            .unwrap();

        let wat = r#"(module (memory (export "memory") 1))"#;
        let module = Module::new(&store, wat).unwrap();
        let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
        func_env.initialize(&mut store, instance).unwrap();

        let env = func_env.data(&store);
        let mut preopen_fd = None;
        for fd in env.state.fs.preopen_fds.read().unwrap().iter().copied() {
            if let Ok(entry) = env.state.fs.get_fd(fd) {
                if !entry.inner.rights.contains(Rights::PATH_CREATE_DIRECTORY) {
                    continue;
                }
                let is_root = matches!(*entry.inode.read(), Kind::Root { .. });
                if is_root {
                    continue;
                }
                preopen_fd = Some(fd);
                break;
            }
        }
        let preopen_fd = preopen_fd.expect("no non-root preopen with PATH_CREATE_DIRECTORY rights");
        (store, func_env, preopen_fd)
    }

    fn open_path(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        root_fd: WasiFd,
        path: &str,
        oflags: Oflags,
    ) -> Result<WasiFd, Errno> {
        path_open_internal(
            ctx.data(),
            root_fd,
            0,
            path,
            oflags,
            Rights::all(),
            Rights::all(),
            Fdflags::empty(),
            Fdflagsext::empty(),
            None,
        )
        .unwrap()
    }

    fn read_dir_entries(
        store: &mut Store,
        func_env: &WasiFunctionEnv,
        fd: WasiFd,
    ) -> Vec<(String, Filetype)> {
        let buf_ptr: WasmPtr<u8, Memory32> = WasmPtr::new(0);
        let bufused_ptr: WasmPtr<u32, Memory32> = WasmPtr::new(8192);
        let buf_len: u32 = 4096;

        let ctx = func_env.env.clone().into_mut(store);
        let err = fd_readdir::<Memory32>(ctx, fd, buf_ptr, buf_len, 0, bufused_ptr).unwrap();
        assert_eq!(err, Errno::Success);

        let env = func_env.data(store);
        let memory = unsafe { env.memory_view(store) };
        let used = bufused_ptr.read(&memory).unwrap() as usize;
        let buf = buf_ptr.slice(&memory, used as u32).unwrap();

        let mut bytes = vec![0u8; used];
        for i in 0..used {
            bytes[i] = buf.index(i as u64).read().unwrap();
        }

        let dirent_size = std::mem::size_of::<Dirent>();
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos + dirent_size <= bytes.len() {
            let d_next = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            let _d_ino = u64::from_le_bytes(bytes[pos + 8..pos + 16].try_into().unwrap());
            let d_namlen = u32::from_le_bytes(bytes[pos + 16..pos + 20].try_into().unwrap()) as usize;
            let d_type = bytes[pos + 20];
            pos += dirent_size;
            if pos + d_namlen > bytes.len() {
                break;
            }
            let name = String::from_utf8(bytes[pos..pos + d_namlen].to_vec()).unwrap();
            pos += d_namlen;
            let ftype = <Filetype as FromToNativeWasmType>::from_native(d_type as i32);
            out.push((name, ftype));
            if d_next == 0 {
                break;
            }
        }
        out
    }

    #[test]
    fn test_readdir_empty() {
        let (mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let entries = read_dir_entries(&mut store, &func_env, root_fd);
        let mut names: Vec<_> = entries.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec![".", ".."]); 
    }

    #[test]
    fn test_readdir_with_files_and_dirs() {
        let (mut store, func_env, root_fd) = setup_env_with_tmpfs();
        {
            let mut ctx = func_env.env.clone().into_mut(&mut store);
            path_create_directory_internal(&mut ctx, root_fd, "testdir").unwrap();
            let fd1 = open_path(&mut ctx, root_fd, "testfile1", Oflags::CREATE | Oflags::TRUNC).unwrap();
            ctx.data().state.fs.close_fd(fd1).unwrap();
            let fd2 = open_path(&mut ctx, root_fd, "testfile2", Oflags::CREATE | Oflags::TRUNC).unwrap();
            ctx.data().state.fs.close_fd(fd2).unwrap();
        }

        let entries = read_dir_entries(&mut store, &func_env, root_fd);
        let mut names: Vec<_> = entries.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec![".", "..", "testdir", "testfile1", "testfile2"]);

        for (name, ftype) in entries.iter() {
            match name.as_str() {
                "." | ".." | "testdir" => assert_eq!(*ftype, Filetype::Directory),
                "testfile1" | "testfile2" => assert_eq!(*ftype, Filetype::RegularFile),
                other => panic!("unexpected entry {other}"),
            }
        }

        let dir_fd = {
            let mut ctx = func_env.env.clone().into_mut(&mut store);
            open_path(&mut ctx, root_fd, "testdir", Oflags::DIRECTORY).unwrap()
        };
        let sub_entries = read_dir_entries(&mut store, &func_env, dir_fd);
        let mut sub_names: Vec<_> = sub_entries.iter().map(|(n, _)| n.as_str()).collect();
        sub_names.sort_unstable();
        assert_eq!(sub_names, vec![".", ".."]); 
        let mut ctx = func_env.env.clone().into_mut(&mut store);
        ctx.data().state.fs.close_fd(dir_fd).unwrap();
    }

    #[test]
    fn test_readdir_on_file_not_directory() {
        let (mut store, func_env, root_fd) = setup_env_with_tmpfs();
        let file_fd = {
            let mut ctx = func_env.env.clone().into_mut(&mut store);
            let fd = open_path(&mut ctx, root_fd, "testfile", Oflags::CREATE | Oflags::TRUNC).unwrap();
            fd
        };
        let ctx2 = func_env.env.clone().into_mut(&mut store);
        let buf_ptr: WasmPtr<u8, Memory32> = WasmPtr::new(0);
        let bufused_ptr: WasmPtr<u32, Memory32> = WasmPtr::new(8192);
        let err = fd_readdir::<Memory32>(ctx2, file_fd, buf_ptr, 256, 0, bufused_ptr).unwrap();
        assert_eq!(err, Errno::Notdir);
        let mut ctx = func_env.env.clone().into_mut(&mut store);
        ctx.data().state.fs.close_fd(file_fd).unwrap();
    }
}
