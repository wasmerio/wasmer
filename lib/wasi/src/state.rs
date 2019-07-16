// use wasmer_runtime_abi::vfs::{
//     vfs::Vfs,
//     file_like::{FileLike, Metadata};
// };
use crate::syscalls::types::*;
use generational_arena::{Arena, Index as Inode};
use hashbrown::hash_map::{Entry, HashMap};
use std::{
    cell::Cell,
    fs,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use wasmer_runtime_core::debug;

pub const MAX_SYMLINKS: usize = 100;

#[derive(Debug)]
pub enum WasiFile {
    HostFile(fs::File),
}

impl Write for WasiFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            WasiFile::HostFile(hf) => hf.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            WasiFile::HostFile(hf) => hf.flush(),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            WasiFile::HostFile(hf) => hf.write_all(buf),
        }
    }

    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        match self {
            WasiFile::HostFile(hf) => hf.write_fmt(fmt),
        }
    }
}

impl Read for WasiFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            WasiFile::HostFile(hf) => hf.read(buf),
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        match self {
            WasiFile::HostFile(hf) => hf.read_to_end(buf),
        }
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        match self {
            WasiFile::HostFile(hf) => hf.read_to_string(buf),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            WasiFile::HostFile(hf) => hf.read_exact(buf),
        }
    }
}

impl Seek for WasiFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match self {
            WasiFile::HostFile(hf) => hf.seek(pos),
        }
    }
}

/// A file that Wasi knows about that may or may not be open
#[derive(Debug)]
pub struct InodeVal {
    pub stat: __wasi_filestat_t,
    pub is_preopened: bool,
    pub name: String,
    pub kind: Kind,
}

impl InodeVal {
    // TODO: clean this up
    pub fn from_file_metadata(
        metadata: &std::fs::Metadata,
        name: String,
        is_preopened: bool,
        kind: Kind,
    ) -> Self {
        InodeVal {
            stat: __wasi_filestat_t {
                st_filetype: if metadata.is_dir() {
                    __WASI_FILETYPE_DIRECTORY
                } else {
                    __WASI_FILETYPE_REGULAR_FILE
                },
                st_size: metadata.len(),
                st_atim: metadata
                    .accessed()
                    .ok()
                    .and_then(|sys_time| sys_time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos() as u64)
                    .unwrap_or(0),
                st_ctim: metadata
                    .created()
                    .ok()
                    .and_then(|sys_time| sys_time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos() as u64)
                    .unwrap_or(0),
                st_mtim: metadata
                    .modified()
                    .ok()
                    .and_then(|sys_time| sys_time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos() as u64)
                    .unwrap_or(0),
                ..__wasi_filestat_t::default()
            },
            is_preopened,
            name,
            kind,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum Kind {
    File {
        /// the open file, if it's open
        handle: Option<WasiFile>,
        /// the path to the file
        path: PathBuf,
    },
    Dir {
        /// Parent directory
        parent: Option<Inode>,
        /// The path on the host system where the directory is located
        // TODO: wrap it like WasiFile
        path: PathBuf,
        /// The entries of a directory are lazily filled.
        entries: HashMap<String, Inode>,
    },
    Symlink {
        forwarded: Option<Inode>,
        /// This is required because, at the very least, symlinks can be deleted and we'll need to check that
        path: PathBuf,
    },
    Buffer {
        buffer: Vec<u8>,
    },
}

#[derive(Clone, Debug)]
pub struct Fd {
    pub rights: __wasi_rights_t,
    pub rights_inheriting: __wasi_rights_t,
    pub flags: __wasi_fdflags_t,
    pub offset: u64,
    pub inode: Inode,
}

#[derive(Debug)]
pub struct WasiFs {
    //pub repo: Repo,
    pub name_map: HashMap<String, Inode>,
    pub inodes: Arena<InodeVal>,
    pub fd_map: HashMap<u32, Fd>,
    pub next_fd: Cell<u32>,
    pub inode_counter: Cell<u64>,
}

impl WasiFs {
    pub fn new(
        preopened_dirs: &[String],
        mapped_dirs: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        debug!("wasi::fs::inodes");
        let inodes = Arena::new();
        let mut wasi_fs = Self {
            name_map: HashMap::new(),
            inodes: inodes,
            fd_map: HashMap::new(),
            next_fd: Cell::new(3),
            inode_counter: Cell::new(1000),
        };
        debug!("wasi::fs::preopen_dirs");
        for dir in preopened_dirs {
            debug!("Attempting to preopen {}", &dir);
            // TODO: think about this
            let default_rights = 0x1FFFFFFF; // all rights
            let cur_dir = PathBuf::from(dir);
            let cur_dir_metadata = cur_dir.metadata().expect("Could not find directory");
            let kind = if cur_dir_metadata.is_dir() {
                Kind::Dir {
                    parent: None,
                    path: cur_dir.clone(),
                    entries: Default::default(),
                }
            } else {
                return Err(format!(
                    "WASI only supports pre-opened directories right now; found \"{}\"",
                    &dir
                ));
            };
            // TODO: handle nested pats in `file`
            let inode_val =
                InodeVal::from_file_metadata(&cur_dir_metadata, dir.clone(), true, kind);

            let inode = wasi_fs.inodes.insert(inode_val);
            wasi_fs.inodes[inode].stat.st_ino = wasi_fs.inode_counter.get();
            wasi_fs
                .create_fd(default_rights, default_rights, 0, inode)
                .expect("Could not open fd");
        }
        debug!("wasi::fs::mapped_dirs");
        for (alias, real_dir) in mapped_dirs {
            debug!("Attempting to open {:?} at {}", real_dir, alias);
            // TODO: think about this
            let default_rights = 0x1FFFFFFF; // all rights
            let cur_dir_metadata = real_dir
                .metadata()
                .expect("mapped dir not at previously verified location");
            let kind = if cur_dir_metadata.is_dir() {
                Kind::Dir {
                    parent: None,
                    path: real_dir.clone(),
                    entries: Default::default(),
                }
            } else {
                return Err(format!(
                    "WASI only supports pre-opened directories right now; found \"{:?}\"",
                    &real_dir,
                ));
            };
            // TODO: handle nested pats in `file`
            let inode_val =
                InodeVal::from_file_metadata(&cur_dir_metadata, alias.clone(), true, kind);

            let inode = wasi_fs.inodes.insert(inode_val);
            wasi_fs.inodes[inode].stat.st_ino = wasi_fs.inode_counter.get();
            wasi_fs
                .create_fd(default_rights, default_rights, 0, inode)
                .expect("Could not open fd");
        }
        debug!("wasi::fs::end");
        Ok(wasi_fs)
    }

    #[allow(dead_code)]
    fn get_inode(&mut self, path: &str) -> Option<Inode> {
        Some(match self.name_map.entry(path.to_string()) {
            Entry::Occupied(o) => *o.get(),
            Entry::Vacant(_v) => {
                return None;
                // let file = if let Ok(file) = OpenOptions::new()
                //     .read(true)
                //     .write(true)
                //     .create(false)
                //     .open(&mut self.repo, path)
                // {
                //     file
                // } else {
                //     return None;
                // };

                // let metadata = file.metadata().unwrap();
                // let inode_index = {
                //     let index = self.inode_counter.get();
                //     self.inode_counter.replace(index + 1)
                // };

                // let systime_to_nanos = |systime: SystemTime| {
                //     let duration = systime
                //         .duration_since(SystemTime::UNIX_EPOCH)
                //         .expect("should always be after unix epoch");
                //     duration.as_nanos() as u64
                // };

                // let inode = self.inodes.insert(InodeVal {
                //     stat: __wasi_filestat_t {
                //         st_dev: 0,
                //         st_ino: inode_index,
                //         st_filetype: match metadata.file_type() {
                //             FileType::File => __WASI_FILETYPE_REGULAR_FILE,
                //             FileType::Dir => __WASI_FILETYPE_DIRECTORY,
                //         },
                //         st_nlink: 0,
                //         st_size: metadata.content_len() as u64,
                //         st_atim: systime_to_nanos(SystemTime::now()),
                //         st_mtim: systime_to_nanos(metadata.modified_at()),
                //         st_ctim: systime_to_nanos(metadata.created_at()),
                //     },
                //     is_preopened: false,
                //     name: path.to_string(),
                //     kind: match metadata.file_type() {
                //         FileType::File => Kind::File { handle: file },
                //         FileType::Dir => Kind::Dir {
                //             handle: file,
                //             entries: HashMap::new(),
                //         },
                //     },
                // });
                // v.insert(inode);
                // inode
            }
        })
    }

    /*
    #[allow(dead_code)]
    fn filestat_inode(
        &self,
        inode: Inode,
        flags: __wasi_lookupflags_t,
    ) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let inode_val = &self.inodes[inode];
        if let (
            true,
            Kind::Symlink {
                mut forwarded,
                path,
            },
        ) = (flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0, &inode_val.kind)
        {
            // Time to follow the symlink.
            let mut counter = 0;

            while counter <= MAX_SYMLINKS {
                let inode_val = &self.inodes[forwarded];
                if let &Kind::Symlink {
                    forwarded: new_forwarded,
                } = &inode_val.kind
                {
                    counter += 1;
                    forwarded = new_forwarded;
                } else {
                    return Ok(inode_val.stat);
                }
            }

            Err(__WASI_EMLINK)
        } else {
            Ok(inode_val.stat)
        }
    }

    #[allow(dead_code)]
    pub fn filestat_path(
        &mut self,
        preopened_fd: __wasi_fd_t,
        flags: __wasi_lookupflags_t,
        path: &str,
    ) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        warn!("Should use preopned_fd: {}", preopened_fd);
        let inode = self.get_inode(path).ok_or(__WASI_EINVAL)?;

        self.filestat_inode(inode, flags)
    }
    */

    /// gets a host file from a base directory and a path
    /// this function ensures the fs remains sandboxed
    pub fn get_inode_at_path(
        &mut self,
        base: __wasi_fd_t,
        path: &str,
    ) -> Result<Inode, __wasi_errno_t> {
        let base_dir = self.get_fd(base)?;
        let path: &Path = Path::new(path);

        let mut symlinks_followed = 0;
        let mut cur_inode = base_dir.inode;
        // TODO: rights checks
        'path_iter: for component in path.components() {
            // for each component traverse file structure
            // loading inodes as necessary
            'symlink_resolution: loop {
                match &mut self.inodes[cur_inode].kind {
                    Kind::Buffer { .. } => unimplemented!("state::get_inode_at_path for buffers"),
                    Kind::Dir {
                        ref mut entries,
                        ref path,
                        ref parent,
                        ..
                    } => {
                        if component.as_os_str().to_string_lossy() == ".." {
                            if let Some(p) = parent {
                                cur_inode = *p;
                                continue 'path_iter;
                            } else {
                                // TODO: be smart here with multiple preopened directories
                                return Err(__WASI_EACCES);
                            }
                        }
                        if let Some(entry) =
                            entries.get(component.as_os_str().to_string_lossy().as_ref())
                        {
                            cur_inode = *entry;
                        } else {
                            let file = {
                                let mut cd = path.clone();
                                cd.push(component);
                                cd
                            };
                            // TODO: verify this returns successfully when given a non-symlink
                            let metadata = file.symlink_metadata().ok().ok_or(__WASI_EEXIST)?;
                            let file_type = metadata.file_type();

                            let kind = if file_type.is_dir() {
                                // load DIR
                                Kind::Dir {
                                    parent: Some(cur_inode),
                                    path: file.clone(),
                                    entries: Default::default(),
                                }
                            } else if file_type.is_file() {
                                // load file
                                Kind::File {
                                    handle: None,
                                    path: file.clone(),
                                }
                            } else if file_type.is_symlink() {
                                // use a stack and load symlinks?
                                // load symlink
                                Kind::Symlink {
                                    forwarded: None,
                                    path: file.clone(),
                                }
                            } else {
                                unimplemented!("state::get_inode_at_path unknown file type: not file, directory, or symlink");
                            };

                            let new_inode = self.inodes.insert(InodeVal {
                                stat: get_stat_for_kind(&kind).ok_or(__WASI_EIO)?,
                                is_preopened: false,
                                name: file.to_string_lossy().to_string(),
                                kind,
                            });
                            *self.inode_counter.get_mut() += 1;

                            cur_inode = new_inode;
                        }
                    }
                    Kind::File { .. } => {
                        return Err(__WASI_ENOTDIR);
                    }
                    Kind::Symlink { forwarded, path } => {
                        if symlinks_followed > MAX_SYMLINKS {
                            return Err(__WASI_EMLINK);
                        }
                        if let Some(fwd) = forwarded {
                            cur_inode = *fwd;
                        } else {
                            // load the symlink
                            let _link = path.read_link().ok().ok_or(__WASI_EIO)?;
                        }
                        symlinks_followed += 1;
                        continue 'symlink_resolution;
                    }
                }
                break 'symlink_resolution;
            }
        }

        Ok(cur_inode)
    }

    pub fn get_fd(&self, fd: __wasi_fd_t) -> Result<&Fd, __wasi_errno_t> {
        self.fd_map.get(&fd).ok_or(__WASI_EBADF)
    }

    pub fn filestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

        Ok(self.inodes[fd.inode].stat)
    }

    pub fn fdstat(&self, fd: __wasi_fd_t) -> Result<__wasi_fdstat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

        debug!("fdstat: {:?}", fd);

        Ok(__wasi_fdstat_t {
            fs_filetype: match self.inodes[fd.inode].kind {
                Kind::File { .. } => __WASI_FILETYPE_REGULAR_FILE,
                Kind::Dir { .. } => __WASI_FILETYPE_DIRECTORY,
                Kind::Symlink { .. } => __WASI_FILETYPE_SYMBOLIC_LINK,
                _ => __WASI_FILETYPE_UNKNOWN,
            },
            fs_flags: fd.flags,
            fs_rights_base: fd.rights,
            fs_rights_inheriting: fd.rights_inheriting, // TODO(lachlan): Is this right?
        })
    }

    pub fn prestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_prestat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

        debug!("in prestat_fd {:?}", fd);
        let inode_val = &self.inodes[fd.inode];

        if inode_val.is_preopened {
            Ok(__wasi_prestat_t {
                pr_type: __WASI_PREOPENTYPE_DIR,
                u: PrestatEnum::Dir {
                    // REVIEW:
                    pr_name_len: inode_val.name.len() as u32 + 1,
                }
                .untagged(),
            })
        } else {
            Err(__WASI_EBADF)
        }
    }

    pub fn flush(&mut self, fd: __wasi_fd_t) -> Result<(), __wasi_errno_t> {
        match fd {
            0 => (),
            1 => io::stdout().flush().map_err(|_| __WASI_EIO)?,
            2 => io::stderr().flush().map_err(|_| __WASI_EIO)?,
            _ => {
                let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;
                if fd.rights & __WASI_RIGHT_FD_DATASYNC == 0 {
                    return Err(__WASI_EACCES);
                }

                let inode = &mut self.inodes[fd.inode];

                match &mut inode.kind {
                    Kind::File {
                        handle: Some(handle),
                        ..
                    } => handle.flush().map_err(|_| __WASI_EIO)?,
                    // TODO: verify this behavior
                    Kind::Dir { .. } => return Err(__WASI_EISDIR),
                    Kind::Symlink { .. } => unimplemented!(),
                    Kind::Buffer { .. } => (),
                    _ => return Err(__WASI_EIO),
                }
            }
        }
        Ok(())
    }

    pub fn create_fd(
        &mut self,
        rights: __wasi_rights_t,
        rights_inheriting: __wasi_rights_t,
        flags: __wasi_fdflags_t,
        inode: Inode,
    ) -> Result<u32, __wasi_errno_t> {
        let idx = self.next_fd.get();
        self.next_fd.set(idx + 1);
        self.fd_map.insert(
            idx,
            Fd {
                rights,
                rights_inheriting,
                flags,
                offset: 0,
                inode,
            },
        );
        Ok(idx)
    }

    pub fn get_base_path_for_directory(&self, directory: Inode) -> Option<String> {
        if let Kind::Dir { path, .. } = &self.inodes[directory].kind {
            return Some(path.to_string_lossy().to_string());
        }
        None
    }
}

#[derive(Debug)]
pub struct WasiState<'a> {
    pub fs: WasiFs,
    pub args: &'a [Vec<u8>],
    pub envs: &'a [Vec<u8>],
}

pub fn host_file_type_to_wasi_file_type(file_type: fs::FileType) -> __wasi_filetype_t {
    // TODO: handle other file types
    if file_type.is_dir() {
        __WASI_FILETYPE_DIRECTORY
    } else if file_type.is_file() {
        __WASI_FILETYPE_REGULAR_FILE
    } else if file_type.is_symlink() {
        __WASI_FILETYPE_SYMBOLIC_LINK
    } else {
        __WASI_FILETYPE_UNKNOWN
    }
}

pub fn get_stat_for_kind(kind: &Kind) -> Option<__wasi_filestat_t> {
    match kind {
        Kind::File { handle, path } => {
            let md = match handle {
                Some(WasiFile::HostFile(hf)) => hf.metadata().ok()?,
                None => path.metadata().ok()?,
            };

            Some(__wasi_filestat_t {
                st_filetype: host_file_type_to_wasi_file_type(md.file_type()),
                st_size: md.len(),
                st_atim: md
                    .accessed()
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos() as u64,
                st_mtim: md
                    .modified()
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos() as u64,
                st_ctim: md
                    .created()
                    .ok()
                    .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|ct| ct.as_nanos() as u64)
                    .unwrap_or(0),
                ..__wasi_filestat_t::default()
            })
        }
        Kind::Dir { path, .. } => {
            let md = path.metadata().ok()?;
            Some(__wasi_filestat_t {
                st_filetype: host_file_type_to_wasi_file_type(md.file_type()),
                st_size: md.len(),
                st_atim: md
                    .accessed()
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos() as u64,
                st_mtim: md
                    .modified()
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos() as u64,
                st_ctim: md
                    .created()
                    .ok()
                    .and_then(|ct| ct.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|ct| ct.as_nanos() as u64)
                    .unwrap_or(0),
                ..__wasi_filestat_t::default()
            })
        }
        _ => None,
    }
}
