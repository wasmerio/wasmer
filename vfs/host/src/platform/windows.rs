use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use vfs_core::provider::FsProviderCapabilities;
use vfs_core::{BackendInodeId, VfsFileType, VfsTimespec};

use super::{DirEntryInfo, Stat};

#[derive(Clone, Debug)]
pub struct DirHandle {
    path: PathBuf,
}

pub fn provider_capabilities() -> FsProviderCapabilities {
    FsProviderCapabilities::SEEK | FsProviderCapabilities::CASE_PRESERVING
}

pub fn open_root_dir(path: &Path) -> io::Result<DirHandle> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "root not directory",
        ));
    }
    Ok(DirHandle {
        path: path.to_path_buf(),
    })
}

pub fn open_dir_at(parent: &DirHandle, name: &vfs_core::VfsName) -> io::Result<DirHandle> {
    let name = name_to_osstring(name.as_bytes())?;
    let path = parent.path.join(name);
    let meta = std::fs::metadata(&path)?;
    if !meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "not dir"));
    }
    Ok(DirHandle { path })
}

pub fn stat_root(dir: &DirHandle) -> io::Result<Stat> {
    stat_path(&dir.path, true)
}

pub fn stat_dir(dir: &DirHandle) -> io::Result<Stat> {
    stat_path(&dir.path, true)
}

pub fn stat_at(parent: &DirHandle, name: &[u8], _nofollow: bool) -> io::Result<Stat> {
    let name = name_to_osstring(name)?;
    let path = parent.path.join(name);
    stat_path(&path, false)
}

pub fn stat_file(file: &std::fs::File) -> io::Result<Stat> {
    let meta = file.metadata()?;
    Ok(stat_from_metadata(meta))
}

pub fn open_file_at(
    parent: &DirHandle,
    name: &[u8],
    flags: vfs_core::flags::OpenFlags,
    _mode: Option<u32>,
) -> io::Result<std::fs::File> {
    let name = name_to_osstring(name)?;
    let path = parent.path.join(name);
    let mut opts = std::fs::OpenOptions::new();
    opts.read(flags.contains(vfs_core::flags::OpenFlags::READ));
    opts.write(flags.contains(vfs_core::flags::OpenFlags::WRITE));
    opts.append(flags.contains(vfs_core::flags::OpenFlags::APPEND));
    if flags.contains(vfs_core::flags::OpenFlags::CREATE)
        && flags.contains(vfs_core::flags::OpenFlags::EXCL)
    {
        opts.create_new(true);
    } else if flags.contains(vfs_core::flags::OpenFlags::CREATE) {
        opts.create(true);
    }
    if flags.contains(vfs_core::flags::OpenFlags::TRUNC) {
        opts.truncate(true);
    }
    opts.open(&path)
}

pub fn mkdir_at(parent: &DirHandle, name: &[u8], _mode: Option<u32>) -> io::Result<()> {
    let name = name_to_osstring(name)?;
    let path = parent.path.join(name);
    std::fs::create_dir(path)
}

pub fn unlink_at(parent: &DirHandle, name: &[u8]) -> io::Result<()> {
    let name = name_to_osstring(name)?;
    let path = parent.path.join(name);
    std::fs::remove_file(path)
}

pub fn rmdir_at(parent: &DirHandle, name: &[u8]) -> io::Result<()> {
    let name = name_to_osstring(name)?;
    let path = parent.path.join(name);
    std::fs::remove_dir(path)
}

pub fn rename_at(
    old_parent: &DirHandle,
    old_name: &[u8],
    new_parent: &DirHandle,
    new_name: &[u8],
) -> io::Result<()> {
    let old = old_parent.path.join(name_to_osstring(old_name)?);
    let new = new_parent.path.join(name_to_osstring(new_name)?);
    std::fs::rename(old, new)
}

pub fn link_at(
    existing_parent: &DirHandle,
    existing_name: &[u8],
    new_parent: &DirHandle,
    new_name: &[u8],
) -> io::Result<()> {
    let old = existing_parent.path.join(name_to_osstring(existing_name)?);
    let new = new_parent.path.join(name_to_osstring(new_name)?);
    std::fs::hard_link(old, new)
}

pub fn symlink_at(_parent: &DirHandle, _new_name: &[u8], _target: &[u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink not supported",
    ))
}

pub fn readlink_at(_parent: &DirHandle, _name: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "readlink not supported",
    ))
}

pub fn chmod_at(_parent: &DirHandle, _name: &[u8], _mode: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "chmod not supported",
    ))
}

pub fn chown_at(
    _parent: &DirHandle,
    _name: &[u8],
    _uid: Option<u32>,
    _gid: Option<u32>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "chown not supported",
    ))
}

pub fn utimens_at(
    _parent: &DirHandle,
    _name: &[u8],
    _atime: Option<VfsTimespec>,
    _mtime: Option<VfsTimespec>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "utimens not supported",
    ))
}

pub fn read_dir(dir: &DirHandle) -> io::Result<Vec<DirEntryInfo>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir.path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().as_bytes().to_vec();
        let stat = stat_path(&entry.path(), false)?;
        entries.push(DirEntryInfo {
            name,
            inode: stat.inode,
            file_type: stat.file_type,
        });
    }
    Ok(entries)
}

fn stat_path(path: &Path, follow: bool) -> io::Result<Stat> {
    let meta = if follow {
        std::fs::metadata(path)?
    } else {
        std::fs::symlink_metadata(path)?
    };
    Ok(stat_from_metadata(meta))
}

fn stat_from_metadata(meta: std::fs::Metadata) -> Stat {
    use std::os::windows::fs::MetadataExt;
    let file_type = if meta.file_type().is_dir() {
        VfsFileType::Directory
    } else if meta.file_type().is_symlink() {
        VfsFileType::Symlink
    } else {
        VfsFileType::RegularFile
    };
    let inode = make_backend_inode(meta.volume_serial_number() as u64, meta.file_index());
    let (atime, mtime, ctime) = (
        system_time_to_vfs(meta.accessed().ok()),
        system_time_to_vfs(meta.modified().ok()),
        system_time_to_vfs(meta.created().ok()),
    );
    Stat {
        inode,
        file_type,
        mode: 0,
        uid: 0,
        gid: 0,
        nlink: meta.number_of_links() as u64,
        size: meta.len(),
        atime,
        mtime,
        ctime,
        rdev_major: 0,
        rdev_minor: 0,
        dir_handle: None,
    }
}

fn make_backend_inode(dev: u64, ino: u64) -> BackendInodeId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    dev.hash(&mut hasher);
    ino.hash(&mut hasher);
    let mut raw = hasher.finish();
    if raw == 0 {
        raw = 1;
    }
    BackendInodeId::new(raw).expect("non-zero inode")
}

fn system_time_to_vfs(st: Option<SystemTime>) -> VfsTimespec {
    let Some(time) = st else {
        return VfsTimespec { secs: 0, nanos: 0 };
    };
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    VfsTimespec {
        secs: duration.as_secs() as i64,
        nanos: duration.subsec_nanos(),
    }
}

fn name_to_osstring(name: &[u8]) -> io::Result<OsString> {
    let s = std::str::from_utf8(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name not utf-8"))?;
    Ok(OsString::from(s))
}
