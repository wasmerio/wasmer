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
