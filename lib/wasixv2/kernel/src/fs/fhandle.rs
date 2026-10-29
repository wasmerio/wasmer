//! `name_to_handle_at` and `open_by_handle_at` (`fs/fhandle.c`): argument decoding and the
//! permission model; the VFS encodes and decodes (`wasixv2_vfs::Vfs::encode_fh`).

use wasixv2_abi::{
    AtFd, AtFlags, AtHandleFlags, ConstFileHandlePtr, Cstr, FileHandle, FileHandleMisc,
    FileHandlePtr, GuestPtr, OpenFlags, S32Ptr,
};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, read_value, write_value};
use wasixv2_vfs::{Cap, FileId, HandleKind, MAX_HANDLE_SZ, Path};

use super::{Env, build_open_flags, build_open_how, read_path};
use crate::Kernel;

/// `FILEID_USER_FLAGS_MASK`: the bits of a handle type that are the VFS's, not the
/// filesystem's.
const FILEID_USER_FLAGS_MASK: i32 = !0xffff;
/// `FILEID_VALID_USER_FLAGS`.
const FILEID_VALID_USER_FLAGS: i32 =
    FileHandleMisc::FILEID_IS_CONNECTABLE.0 | FileHandleMisc::FILEID_IS_DIR.0;
/// Where the handle body starts in `struct file_handle`.
const HANDLE_BODY: u32 = 8;

/// fs/fhandle.c's HANDLE_CHECK_PERMS and HANDLE_CHECK_SUBTREE models.
enum DecodePermission {
    Global,
    Filesystem,
    Subtree,
}

impl Kernel {
    /// `sys_name_to_handle_at`.
    pub(crate) async fn sys_name_to_handle_at(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        name: Cstr,
        handle: FileHandlePtr,
        mnt_id: S32Ptr,
        flag: i32,
    ) -> SysResult<i32> {
        let flag = flag as u32;
        let connectable = AtHandleFlags::AT_HANDLE_CONNECTABLE.0;
        let fid = AtHandleFlags::AT_HANDLE_FID.0;
        let unique = AtHandleFlags::AT_HANDLE_MNT_ID_UNIQUE.0;
        let known = AtFlags::AT_SYMLINK_FOLLOW.0 | AtFlags::AT_EMPTY_PATH.0 | fid | unique;
        if flag & !(known | connectable) != 0 {
            return Err(Errno::EINVAL.into());
        }
        // A connectable handle needs a parent, which neither a mere identifier nor an
        // `AT_EMPTY_PATH` object (possibly disconnected) gives.
        if flag & connectable != 0 && flag & (fid | AtFlags::AT_EMPTY_PATH.0) != 0 {
            return Err(Errno::EINVAL.into());
        }
        let kind = if flag & fid != 0 {
            HandleKind::Fid
        } else if flag & connectable != 0 {
            HandleKind::Connectable
        } else {
            HandleKind::Decodable
        };
        let path = read_path(caller.memory(), name).await?;
        let env = self.env(caller);
        let lookup = AtFlags(flag & AtFlags::AT_EMPTY_PATH.0);
        let follow = flag & AtFlags::AT_SYMLINK_FOLLOW.0 != 0;
        let target = self
            .resolve_at(caller, &env, dfd, &path, lookup, follow)
            .await?;
        self.do_name_to_handle(caller, &target, handle, mnt_id, flag & unique != 0, kind)
            .await
    }

    /// `do_sys_name_to_handle`.
    async fn do_name_to_handle(
        &self,
        caller: &Caller<'_>,
        path: &Path,
        ufh: FileHandlePtr,
        mnt_id: S32Ptr,
        unique: bool,
        kind: HandleKind,
    ) -> SysResult<i32> {
        let fid = self.vfs.encode_fh(path, kind)?;
        let memory = caller.memory();
        let header: FileHandle = read_value(memory, GuestPtr::new(ufh.addr())).await?;
        if header.handle_bytes as usize > MAX_HANDLE_SZ {
            return Err(Errno::EINVAL.into());
        }
        // Too small: the size it needs and `FILEID_INVALID`, without a body.
        let fits = fid.data.len() <= header.handle_bytes as usize;
        let mut handle_type = match fits {
            true => fid.kind,
            false => FileHandleMisc::FILEID_INVALID.0,
        };
        if fits && kind == HandleKind::Connectable {
            handle_type |= FileHandleMisc::FILEID_IS_CONNECTABLE.0;
            if path.dentry.is_dir() {
                handle_type |= FileHandleMisc::FILEID_IS_DIR.0;
            }
        }
        if unique {
            let id = path.mnt.unique_id().to_le_bytes();
            memory
                .write(mnt_id.addr(), &id)
                .await
                .map_err(|_| Errno::EFAULT)?;
        } else {
            write_value(memory, mnt_id, &(path.mnt.id as i32)).await?;
        }
        let out = FileHandle {
            handle_bytes: fid.data.len() as u32,
            handle_type,
        };
        write_value(memory, ufh, &out).await?;
        if !fits {
            return Err(Errno::EOVERFLOW.into());
        }
        memory
            .write(ufh.addr() + HANDLE_BODY, &fid.data)
            .await
            .map_err(|_| Errno::EFAULT)?;
        Ok(0)
    }

    /// `sys_open_by_handle_at`: `handle_to_path`, then the open of the object itself
    /// (`file_open_root` with an empty name).
    pub(crate) async fn sys_open_by_handle_at(
        &self,
        caller: &Caller<'_>,
        mountdirfd: Fd,
        ufh: ConstFileHandlePtr,
        flags: OpenFlags,
    ) -> SysResult<i32> {
        let memory = caller.memory();
        let header: FileHandle = read_value(memory, ufh).await?;
        let size = header.handle_bytes as usize;
        if size > MAX_HANDLE_SZ || size == 0 {
            return Err(Errno::EINVAL.into());
        }
        let user_flags = header.handle_type & FILEID_USER_FLAGS_MASK;
        if header.handle_type < 0 || user_flags & !FILEID_VALID_USER_FLAGS != 0 {
            return Err(Errno::EINVAL.into());
        }
        let env = self.env(caller);
        let root = self.path_anchor(caller, &env, mountdirfd)?;
        let check = self.may_decode_fh(caller, &env, &root, flags)?;
        let mut data = vec![0; size];
        memory
            .read(ufh.addr() + HANDLE_BODY, &mut data)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let fid = FileId {
            kind: header.handle_type & !FILEID_USER_FLAGS_MASK,
            data,
        };
        let dir_only = !matches!(check, DecodePermission::Global)
            || user_flags & FileHandleMisc::FILEID_IS_DIR.0 != 0;
        let cred = (!matches!(check, DecodePermission::Global)).then_some(&env.cred);
        let path = self
            .vfs
            .decode_fh(
                &root,
                &fid,
                dir_only,
                cred,
                matches!(check, DecodePermission::Subtree),
            )
            .await?;
        let how = build_open_how(flags, 0);
        let (flags, lookup) = build_open_flags(&how)?;
        let lookup = wasixv2_vfs::LookupFlags {
            empty: true,
            ..lookup
        };
        let fd = self
            .open_from(caller, &env, &path, b"", flags, 0, lookup)
            .await?;
        Ok(fd.0)
    }

    /// `get_path_anchor`: the mount a handle is decoded on.
    fn path_anchor(&self, caller: &Caller<'_>, env: &Env, fd: Fd) -> Result<Path, Errno> {
        if fd.0 >= 0 {
            return self.fd_path(caller, fd);
        }
        if fd.0 == AtFd::AT_FDCWD.0 {
            return Ok(env.fs.cwd.clone());
        }
        Err(Errno::EBADF)
    }

    /// `may_decode_fh`: `CAP_DAC_READ_SEARCH` decodes anything; otherwise only a directory
    /// (`O_DIRECTORY`), for a caller that could mount the filesystem (`CAP_SYS_ADMIN` over its
    /// user namespace) and override read and search permissions, and only along objects its
    /// user namespace maps (`HANDLE_CHECK_PERMS`). A caller who owns only the mount
    /// namespace also needs an unobstructed subtree, and may decode only below root
    /// (`HANDLE_CHECK_SUBTREE`), matching fs/fhandle.c may_decode_fh.
    fn may_decode_fh(
        &self,
        caller: &Caller<'_>,
        env: &Env,
        root: &Path,
        flags: OpenFlags,
    ) -> Result<DecodePermission, Errno> {
        let cred = self.process(caller.task()).cred();
        if cred.capable_init(Cap::DacReadSearch) {
            return Ok(DecodePermission::Global);
        }
        if !flags.contains(OpenFlags::O_DIRECTORY) {
            return Err(Errno::EPERM);
        }
        let check = if cred.ns_capable(&self.sb_owner(root.dentry.sb()), Cap::SysAdmin) {
            DecodePermission::Filesystem
        } else if self.vfs.subtree_unobstructed(root, |owner| {
            let owner = crate::userns::kernel_user_ns(owner, &self.init_user_ns);
            cred.ns_capable(&owner, Cap::SysAdmin)
        }) {
            DecodePermission::Subtree
        } else {
            return Err(Errno::EPERM);
        };
        if !env.cred.capable(Cap::DacReadSearch) {
            return Err(Errno::EPERM);
        }
        Ok(check)
    }
}
