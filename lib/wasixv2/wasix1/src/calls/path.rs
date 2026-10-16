//! Path calls (`path_*`), `getcwd` and `chdir`.
//!
//! v1 paths are `(pointer, length)` pairs relative to a directory descriptor; the shims copy
//! them into scratch as C strings and call the `*at` syscalls, so the kernel resolves them with
//! Linux's rules (absolute paths ignore the descriptor, `..` stops at the root).

use wasixv2_abi::{AtFlags, Fd, OpenFlags, RenameFlags};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno,
        fs::{Fdflags, Fdflagsext, Fstflags, Lookupflags, Oflags, Rights},
    },
    calls::fs::{
        FILESTAT_MASK, filestat, filetype_of_mode, open_flags, path_arg, stat_at, utimes,
        write_filestat,
    },
    errno::V1Result,
    shim::{self, Env},
};

/// Mode bits for new files and directories; the umask applies.
const FILE_MODE: u32 = 0o666;
const DIR_MODE: u32 = 0o777;

/// `AT_SYMLINK_NOFOLLOW` unless `lookup` follows symlinks.
fn nofollow(lookup: Lookupflags) -> AtFlags {
    match lookup.contains(Lookupflags::SYMLINK_FOLLOW) {
        true => AtFlags(0),
        false => AtFlags::AT_SYMLINK_NOFOLLOW,
    }
}

/// The `openat` flags for a v1 `path_open`.
///
/// The access mode comes from the requested rights (`FD_READ`/`FD_READDIR`, `FD_WRITE`), as
/// wasix-libc computes them from `O_*`; a directory (`DIRECTORY`) is always opened read-only.
/// Without `SYMLINK_FOLLOW` the last component must not be a symlink (`O_NOFOLLOW`).
pub(crate) fn open_how(
    lookup: Lookupflags,
    oflags: Oflags,
    rights: Rights,
    fdflags: Fdflags,
    fdflagsext: Fdflagsext,
) -> V1Result<OpenFlags> {
    if (oflags & !Oflags::ALL).0 != 0 {
        return shim::fail(Errno::INVAL);
    }
    let directory = oflags.contains(Oflags::DIRECTORY);
    let write = !directory && rights.contains(Rights::FD_WRITE);
    let read = rights.intersects(Rights::FD_READ | Rights::FD_READDIR);
    let mut flags = match (read, write) {
        (true, true) => OpenFlags::O_RDWR,
        (false, true) => OpenFlags::O_WRONLY,
        (_, false) => OpenFlags::O_RDONLY,
    };
    for (v1, linux) in [
        (Oflags::CREAT, OpenFlags::O_CREAT),
        (Oflags::DIRECTORY, OpenFlags::O_DIRECTORY),
        (Oflags::EXCL, OpenFlags::O_EXCL),
        (Oflags::TRUNC, OpenFlags::O_TRUNC),
    ] {
        if oflags.contains(v1) {
            flags |= linux;
        }
    }
    flags |= open_flags(fdflags);
    if !lookup.contains(Lookupflags::SYMLINK_FOLLOW) {
        flags |= OpenFlags::O_NOFOLLOW;
    }
    if fdflagsext.contains(Fdflagsext::CLOEXEC) {
        flags |= OpenFlags::O_CLOEXEC;
    }
    Ok(flags)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn path_open<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    dirflags: i32,
    path: i32,
    path_len: i32,
    oflags: i32,
    rights_base: i64,
    rights_inheriting: i64,
    fdflags: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    path_open2(
        env,
        fd,
        dirflags,
        path,
        path_len,
        oflags,
        rights_base,
        rights_inheriting,
        fdflags,
        0,
        out,
    )
    .await
}

/// `path_open` with descriptor flags (`CLOEXEC`), which wasix-libc's `openat` uses.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn path_open2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    dirflags: i32,
    path: i32,
    path_len: i32,
    oflags: i32,
    rights_base: i64,
    _rights_inheriting: i64,
    fdflags: i32,
    fdflagsext: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let flags = open_how(
            Lookupflags(dirflags as u32),
            Oflags(oflags as u16),
            Rights(rights_base as u64),
            Fdflags(fdflags as u16),
            Fdflagsext(fdflagsext as u16),
        )?;
        let path = path_arg(frame, path, path_len).await?;
        let new = frame
            .syscalls
            .openat(&frame.caller(), Fd(fd), path, flags, FILE_MODE)
            .await?;
        frame.write(out as u32, &(new.0 as u32)).await
    })
    .await
}

pub(crate) async fn path_create_directory<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let path = path_arg(frame, path, path_len).await?;
        frame
            .syscalls
            .mkdirat(&frame.caller(), Fd(fd), path, DIR_MODE)
            .await?;
        Ok(())
    })
    .await
}

pub(crate) async fn path_remove_directory<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    unlink(env, fd, path, path_len, AtFlags::AT_REMOVEDIR).await
}

pub(crate) async fn path_unlink_file<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    unlink(env, fd, path, path_len, AtFlags(0)).await
}

async fn unlink<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
    flags: AtFlags,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let path = path_arg(frame, path, path_len).await?;
        frame
            .syscalls
            .unlinkat(&frame.caller(), Fd(fd), path, flags)
            .await?;
        Ok(())
    })
    .await
}

/// `path_filestat_get` of `wasi_snapshot_preview1` and `wasix_32v1`.
pub(crate) async fn path_filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    flags: i32,
    path: i32,
    path_len: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    filestat_get(env, fd, flags, path, path_len, out, false).await
}

/// `path_filestat_get` of `wasi_unstable`.
pub(crate) async fn snapshot0_path_filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    flags: i32,
    path: i32,
    path_len: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    filestat_get(env, fd, flags, path, path_len, out, true).await
}

/// A path cannot be asked for its socket type, so sockets in the filesystem are
/// `SOCKET_STREAM`, as v1 reported them.
async fn filestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    lookup: i32,
    path: i32,
    path_len: i32,
    out: i32,
    snapshot0: bool,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let path = path_arg(frame, path, path_len).await?;
        let flags = nofollow(Lookupflags(lookup as u32));
        let st = stat_at(frame, Fd(fd), path, flags, FILESTAT_MASK).await?;
        let stat = filestat(&st, filetype_of_mode(u32::from(st.stx_mode)));
        write_filestat(frame, out, stat, snapshot0).await
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn path_filestat_set_times<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    lookup: i32,
    path: i32,
    path_len: i32,
    atim: i64,
    mtim: i64,
    fst_flags: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let times = utimes(atim as u64, mtim as u64, Fstflags(fst_flags as u16))?;
        let path = path_arg(frame, path, path_len).await?;
        let times = frame.mem.alloc(&times)?;
        frame
            .syscalls
            .utimensat_time64(
                &frame.caller(),
                Fd(fd),
                path,
                wasixv2_abi::GuestPtr::new(times.addr()),
                nofollow(Lookupflags(lookup as u32)),
            )
            .await?;
        Ok(())
    })
    .await
}

/// `linkat`; `SYMLINK_FOLLOW` links the target of a symlink rather than the symlink.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn path_link<S: Syscalls + Control + 'static>(
    env: Env<S>,
    old_fd: i32,
    old_lookup: i32,
    old_path: i32,
    old_len: i32,
    new_fd: i32,
    new_path: i32,
    new_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let old = path_arg(frame, old_path, old_len).await?;
        let new = path_arg(frame, new_path, new_len).await?;
        let flags = match Lookupflags(old_lookup as u32).contains(Lookupflags::SYMLINK_FOLLOW) {
            true => AtFlags::AT_SYMLINK_FOLLOW,
            false => AtFlags(0),
        };
        frame
            .syscalls
            .linkat(&frame.caller(), Fd(old_fd), old, Fd(new_fd), new, flags)
            .await?;
        Ok(())
    })
    .await
}

pub(crate) async fn path_rename<S: Syscalls + Control + 'static>(
    env: Env<S>,
    old_fd: i32,
    old_path: i32,
    old_len: i32,
    new_fd: i32,
    new_path: i32,
    new_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let old = path_arg(frame, old_path, old_len).await?;
        let new = path_arg(frame, new_path, new_len).await?;
        frame
            .syscalls
            .renameat2(
                &frame.caller(),
                Fd(old_fd),
                old,
                Fd(new_fd),
                new,
                RenameFlags(0),
            )
            .await?;
        Ok(())
    })
    .await
}

/// `symlinkat(target, fd, path)`: the target is stored as given, never resolved.
pub(crate) async fn path_symlink<S: Syscalls + Control + 'static>(
    env: Env<S>,
    target: i32,
    target_len: i32,
    fd: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let target = path_arg(frame, target, target_len).await?;
        let path = path_arg(frame, path, path_len).await?;
        frame
            .syscalls
            .symlinkat(&frame.caller(), target, Fd(fd), path)
            .await?;
        Ok(())
    })
    .await
}

/// `readlinkat` straight into the guest's buffer. As on Linux, a target longer than the
/// buffer is truncated (v1 failed with `OVERFLOW` instead), and no NUL is appended.
pub(crate) async fn path_readlink<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
    buf: i32,
    buf_len: i32,
    used: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let path = path_arg(frame, path, path_len).await?;
        let n = frame
            .syscalls
            .readlinkat(
                &frame.caller(),
                Fd(fd),
                path,
                wasixv2_abi::GuestPtrMut::new(buf as u32),
                buf_len.max(0),
            )
            .await?;
        frame.write(used as u32, &(n as u32)).await
    })
    .await
}

/// The working directory without a NUL; `len` is the buffer's size in and the path's length
/// out. A buffer without room for the path and its NUL is `RANGE`, with the size needed in
/// `len`, so wasix-libc's `getcwd(NULL, 0)` allocates enough and gets a terminated string.
pub(crate) async fn getcwd<S: Syscalls + Control + 'static>(
    env: Env<S>,
    buf: i32,
    len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let size: u32 = frame.read(len as u32).await?;
        let scratch = frame.mem.alloc_zeroed(4096)?;
        let n = frame.syscalls.getcwd(&frame.caller(), scratch).await?;
        // The kernel counts the NUL.
        let cwd = frame.mem.take_bytes(scratch, n.saturating_sub(1) as usize);
        let needed = cwd.len() as u32 + 1;
        if size < needed {
            frame.write(len as u32, &needed).await?;
            return shim::fail(Errno::RANGE);
        }
        let mut terminated = cwd;
        terminated.push(0);
        frame.write_bytes(buf as u32, &terminated).await?;
        frame.write(len as u32, &(needed - 1)).await
    })
    .await
}

pub(crate) async fn chdir<S: Syscalls + Control + 'static>(
    env: Env<S>,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let path = path_arg(frame, path, path_len).await?;
        frame.syscalls.chdir(&frame.caller(), path).await?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn how(oflags: Oflags, rights: Rights) -> OpenFlags {
        open_how(
            Lookupflags::SYMLINK_FOLLOW,
            oflags,
            rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
        )
        .unwrap()
    }

    #[test]
    fn access_mode_from_rights() {
        assert_eq!(how(Oflags::empty(), Rights::FD_READ), OpenFlags::O_RDONLY);
        assert_eq!(how(Oflags::empty(), Rights::FD_WRITE), OpenFlags::O_WRONLY);
        assert_eq!(
            how(Oflags::empty(), Rights::FD_READ | Rights::FD_WRITE),
            OpenFlags::O_RDWR
        );
        assert_eq!(how(Oflags::empty(), Rights::empty()), OpenFlags::O_RDONLY);
        // Directories are opened read-only whatever rights the caller asks for.
        assert_eq!(
            how(Oflags::DIRECTORY, Rights::ALL),
            OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY
        );
    }

    #[test]
    fn open_flags_map() {
        let flags = open_how(
            Lookupflags::empty(),
            Oflags::CREAT | Oflags::EXCL | Oflags::TRUNC,
            Rights::FD_WRITE,
            Fdflags::APPEND | Fdflags::NONBLOCK,
            Fdflagsext::CLOEXEC,
        )
        .unwrap();
        assert_eq!(
            flags,
            OpenFlags::O_WRONLY
                | OpenFlags::O_CREAT
                | OpenFlags::O_EXCL
                | OpenFlags::O_TRUNC
                | OpenFlags::O_APPEND
                | OpenFlags::O_NONBLOCK
                | OpenFlags::O_NOFOLLOW
                | OpenFlags::O_CLOEXEC
        );
        assert!(
            open_how(
                Lookupflags::empty(),
                Oflags(0x10),
                Rights::empty(),
                Fdflags::empty(),
                Fdflagsext::empty()
            )
            .is_err()
        );
    }
}
