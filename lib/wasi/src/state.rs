// use wasmer_runtime_abi::vfs::{
//     vfs::Vfs,
//     file_like::{FileLike, Metadata};
// };
use crate::syscalls::types::*;
use generational_arena::Arena;
pub use generational_arena::Index as Inode;
use hashbrown::hash_map::{Entry, HashMap};
use std::{
    borrow::Borrow,
    cell::Cell,
    fs,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use wasmer_runtime_core::debug;

/// A completely aribtrary "big enough" number used as the upper limit for
/// the number of symlinks that can be traversed when resolving a path
pub const MAX_SYMLINKS: u32 = 128;

#[derive(Debug)]
pub enum WasiFile {
    HostFile(fs::File),
}

impl WasiFile {
    pub fn close(self) {}
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
    /// The same as Dir but without the irrelevant bits
    /// The root is immutable after creation; generally the Kind::Root
    /// branch of whatever code you're writing will be a simpler version of
    /// your Kind::Dir logic
    Root {
        entries: HashMap<String, Inode>,
    },
    /// The first two fields are data _about_ the symlink
    /// the last field is the data _inside_ the symlink
    ///
    /// `base_po_dir` should never be the root because:
    /// - Right now symlinks are not allowed in the immutable root
    /// - There is always a closer pre-opened dir to the symlink file (by definition of the root being a collection of preopened dirs)
    Symlink {
        /// The preopened dir that this symlink file is relative to (via `path_to_symlink`)
        base_po_dir: __wasi_fd_t,
        /// The path to the symlink from the `base_po_dir`
        path_to_symlink: PathBuf,
        /// the value of the symlink as a relative path
        relative_path: PathBuf,
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
    pub preopen_fds: Vec<u32>,
    pub name_map: HashMap<String, Inode>,
    pub inodes: Arena<InodeVal>,
    pub fd_map: HashMap<u32, Fd>,
    pub next_fd: Cell<u32>,
    inode_counter: Cell<u64>,
}

impl WasiFs {
    pub fn new(
        preopened_dirs: &[String],
        mapped_dirs: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        debug!("wasi::fs::inodes");
        let inodes = Arena::new();
        let mut wasi_fs = Self {
            preopen_fds: vec![],
            name_map: HashMap::new(),
            inodes,
            fd_map: HashMap::new(),
            next_fd: Cell::new(3),
            inode_counter: Cell::new(1024),
        };
        // create virtual root
        let root_inode = {
            let all_rights = 0x1FFFFFFF;
            // TODO: make this a list of positive rigths instead of negative ones
            // root gets all right for now
            let root_rights = all_rights
                /*& (!__WASI_RIGHT_FD_WRITE)
                & (!__WASI_RIGHT_FD_ALLOCATE)
                & (!__WASI_RIGHT_PATH_CREATE_DIRECTORY)
                & (!__WASI_RIGHT_PATH_CREATE_FILE)
                & (!__WASI_RIGHT_PATH_LINK_SOURCE)
                & (!__WASI_RIGHT_PATH_RENAME_SOURCE)
                & (!__WASI_RIGHT_PATH_RENAME_TARGET)
                & (!__WASI_RIGHT_PATH_FILESTAT_SET_SIZE)
                & (!__WASI_RIGHT_PATH_FILESTAT_SET_TIMES)
                & (!__WASI_RIGHT_FD_FILESTAT_SET_SIZE)
                & (!__WASI_RIGHT_FD_FILESTAT_SET_TIMES)
                & (!__WASI_RIGHT_PATH_SYMLINK)
                & (!__WASI_RIGHT_PATH_UNLINK_FILE)
                & (!__WASI_RIGHT_PATH_REMOVE_DIRECTORY)*/;
            let inode = wasi_fs.create_virtual_root();
            let fd = wasi_fs
                .create_fd(root_rights, root_rights, 0, inode)
                .expect("Could not create root fd");
            wasi_fs.preopen_fds.push(fd);
            inode
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
                    parent: Some(root_inode),
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
            let inode = wasi_fs
                .create_inode(kind, true, dir.to_string())
                .map_err(|e| {
                    format!(
                        "Failed to create inode for preopened dir: WASI error code: {}",
                        e
                    )
                })?;
            let fd = wasi_fs
                .create_fd(default_rights, default_rights, 0, inode)
                .expect("Could not open fd");
            if let Kind::Root { entries } = &mut wasi_fs.inodes[root_inode].kind {
                // todo handle collisions
                assert!(entries.insert(dir.to_string(), inode).is_none())
            }
            wasi_fs.preopen_fds.push(fd);
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
                    parent: Some(root_inode),
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
            let inode = wasi_fs
                .create_inode(kind, true, alias.clone())
                .map_err(|e| {
                    format!(
                        "Failed to create inode for preopened dir: WASI error code: {}",
                        e
                    )
                })?;
            let fd = wasi_fs
                .create_fd(default_rights, default_rights, 0, inode)
                .expect("Could not open fd");
            if let Kind::Root { entries } = &mut wasi_fs.inodes[root_inode].kind {
                // todo handle collisions
                assert!(entries.insert(alias.clone(), inode).is_none());
            }
            wasi_fs.preopen_fds.push(fd);
        }

        debug!("wasi::fs::end");
        Ok(wasi_fs)
    }

    fn get_next_inode_index(&mut self) -> u64 {
        let next = self.inode_counter.get();
        self.inode_counter.set(next + 1);
        next
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

    fn get_inode_at_path_inner(
        &mut self,
        base: __wasi_fd_t,
        path: &str,
        mut symlink_count: u32,
        follow_symlinks: bool,
    ) -> Result<Inode, __wasi_errno_t> {
        if symlink_count > MAX_SYMLINKS {
            return Err(__WASI_EMLINK);
        }

        let base_dir = self.get_fd(base)?;
        let path: &Path = Path::new(path);

        let mut cur_inode = base_dir.inode;
        // TODO: rights checks
        'path_iter: for component in path.components() {
            // for each component traverse file structure
            // loading inodes as necessary
            'symlink_resolution: while symlink_count < MAX_SYMLINKS {
                match &mut self.inodes[cur_inode].kind {
                    Kind::Buffer { .. } => unimplemented!("state::get_inode_at_path for buffers"),
                    Kind::Dir {
                        ref mut entries,
                        ref path,
                        ref parent,
                        ..
                    } => {
                        match component.as_os_str().to_string_lossy().borrow() {
                            ".." => {
                                if let Some(p) = parent {
                                    cur_inode = *p;
                                    continue 'path_iter;
                                } else {
                                    return Err(__WASI_EACCES);
                                }
                            }
                            "." => continue 'path_iter,
                            _ => (),
                        }
                        // used for full resolution of symlinks
                        let mut loop_for_symlink = false;
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
                            let metadata = file.symlink_metadata().ok().ok_or(__WASI_EINVAL)?;
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
                                let link_value = file.read_link().ok().ok_or(__WASI_EIO)?;
                                debug!("attempting to decompose path {:?}", link_value);

                                let (pre_open_dir_fd, relative_path) = if link_value.is_relative() {
                                    self.path_into_pre_open_and_relative_path(&file)?
                                } else {
                                    unimplemented!("Absolute symlinks are not yet supported");
                                };
                                loop_for_symlink = true;
                                symlink_count += 1;
                                Kind::Symlink {
                                    base_po_dir: pre_open_dir_fd,
                                    path_to_symlink: relative_path,
                                    relative_path: link_value,
                                }
                            } else {
                                unimplemented!("state::get_inode_at_path unknown file type: not file, directory, or symlink");
                            };

                            cur_inode =
                                self.create_inode(kind, false, file.to_string_lossy().to_string())?;
                            if loop_for_symlink && follow_symlinks {
                                continue 'symlink_resolution;
                            }
                        }
                    }
                    Kind::Root { entries } => {
                        match component.as_os_str().to_string_lossy().borrow() {
                            // the root's parent is the root
                            ".." => continue 'path_iter,
                            // the root's current directory is the root
                            "." => continue 'path_iter,
                            _ => (),
                        }

                        if let Some(entry) =
                            entries.get(component.as_os_str().to_string_lossy().as_ref())
                        {
                            cur_inode = *entry;
                        } else {
                            return Err(__WASI_EINVAL);
                        }
                    }
                    Kind::File { .. } => {
                        return Err(__WASI_ENOTDIR);
                    }
                    Kind::Symlink {
                        base_po_dir,
                        path_to_symlink,
                        relative_path,
                    } => {
                        let new_base_dir = *base_po_dir;
                        // allocate to reborrow mutabily to recur
                        let new_path = {
                            /*if let Kind::Root { .. } = self.inodes[base_po_dir].kind {
                                assert!(false, "symlinks should never be relative to the root");
                            }*/
                            let mut base = path_to_symlink.clone();
                            // remove the symlink file itself from the path, leaving just the path from the base
                            // to the dir containing the symlink
                            base.pop();
                            base.push(relative_path);
                            base.to_string_lossy().to_string()
                        };
                        let symlink_inode = self.get_inode_at_path_inner(
                            new_base_dir,
                            &new_path,
                            symlink_count + 1,
                            follow_symlinks,
                        )?;
                        cur_inode = symlink_inode;
                        //continue 'symlink_resolution;
                    }
                }
                break 'symlink_resolution;
            }
        }

        Ok(cur_inode)
    }

    fn path_into_pre_open_and_relative_path(
        &self,
        path: &Path,
    ) -> Result<(__wasi_fd_t, PathBuf), __wasi_errno_t> {
        // for each preopened directory
        for po_fd in &self.preopen_fds {
            let po_inode = self.fd_map[po_fd].inode;
            let po_path = match &self.inodes[po_inode].kind {
                Kind::Dir { path, .. } => &**path,
                Kind::Root { .. } => Path::new("/"),
                _ => unreachable!("Preopened FD that's not a directory or the root"),
            };
            // stem path based on it
            if let Ok(rest) = path.strip_prefix(po_path) {
                // if any path meets this criteria
                // (verify that all remaining components are not symlinks except for maybe last? (or do the more complex logic of resolving intermediary symlinks))
                // return preopened dir and the rest of the path

                return Ok((*po_fd, rest.to_owned()));
            }
        }
        Err(__WASI_EINVAL) // this may not make sense
    }

    /// gets a host file from a base directory and a path
    /// this function ensures the fs remains sandboxed
    // NOTE: follow symlinks is super weird right now
    // even if it's false, it still follows symlinks, just not the last
    // symlink so
    // This will be resolved when we have tests asserting the correct behavior
    pub fn get_inode_at_path(
        &mut self,
        base: __wasi_fd_t,
        path: &str,
        follow_symlinks: bool,
    ) -> Result<Inode, __wasi_errno_t> {
        self.get_inode_at_path_inner(base, path, 0, follow_symlinks)
    }

    /// Returns the parent Dir or Root that the file at a given path is in and the file name
    /// stripped off
    pub fn get_parent_inode_at_path(
        &mut self,
        base: __wasi_fd_t,
        path: &Path,
        follow_symlinks: bool,
    ) -> Result<(Inode, String), __wasi_errno_t> {
        let mut parent_dir = std::path::PathBuf::new();
        let mut components = path.components().rev();
        let new_entity_name = components
            .next()
            .ok_or(__WASI_EINVAL)?
            .as_os_str()
            .to_string_lossy()
            .to_string();
        for comp in components.rev() {
            parent_dir.push(comp);
        }
        self.get_inode_at_path(base, &parent_dir.to_string_lossy(), follow_symlinks)
            .map(|v| (v, new_entity_name))
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

    /// Creates an inode and inserts it given a Kind and some extra data
    pub fn create_inode(
        &mut self,
        kind: Kind,
        is_preopened: bool,
        name: String,
    ) -> Result<Inode, __wasi_errno_t> {
        let mut stat = self.get_stat_for_kind(&kind).ok_or(__WASI_EIO)?;
        stat.st_ino = self.get_next_inode_index();

        Ok(self.inodes.insert(InodeVal {
            stat: stat,
            is_preopened,
            name,
            kind,
        }))
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

    /// This function is unsafe because it's the caller's responsibility to ensure that
    /// all refences to the given inode have been removed from the filesystem
    ///
    /// returns true if the inode existed and was removed
    pub unsafe fn remove_inode(&mut self, inode: Inode) -> bool {
        self.inodes.remove(inode).is_some()
    }

    fn create_virtual_root(&mut self) -> Inode {
        let stat = __wasi_filestat_t {
            st_filetype: __WASI_FILETYPE_DIRECTORY,
            st_ino: self.get_next_inode_index(),
            ..__wasi_filestat_t::default()
        };
        let root_kind = Kind::Root {
            entries: HashMap::new(),
        };

        self.inodes.insert(InodeVal {
            stat: stat,
            is_preopened: true,
            name: "/".to_string(),
            kind: root_kind,
        })
    }

    pub fn get_stat_for_kind(&self, kind: &Kind) -> Option<__wasi_filestat_t> {
        let md = match kind {
            Kind::File { handle, path } => match handle {
                Some(WasiFile::HostFile(hf)) => hf.metadata().ok()?,
                None => path.metadata().ok()?,
            },
            Kind::Dir { path, .. } => path.metadata().ok()?,
            Kind::Symlink {
                base_po_dir,
                path_to_symlink,
                ..
            } => {
                let base_po_inode = &self.fd_map[base_po_dir].inode;
                let base_po_inode_v = &self.inodes[*base_po_inode];
                match &base_po_inode_v.kind {
                    Kind::Root { .. } => {
                        path_to_symlink.clone().symlink_metadata().ok()?
                    }
                    Kind::Dir { path, .. } => {
                        let mut real_path = path.clone();
                        // PHASE 1: ignore all possible symlinks in `relative_path`
                        // TODO: walk the segments of `relative_path` via the entries of the Dir
                        //       use helper function to avoid duplicating this logic (walking this will require
                        //       &self to be &mut sel
                        // TODO: adjust size of symlink, too
                        //      for all paths adjusted think about this
                        real_path.push(path_to_symlink);
                        real_path.symlink_metadata().ok()?
                    }
                    // if this triggers, there's a bug in the symlink code
                    _ => unreachable!("Symlink pointing to something that's not a directory as its base preopened directory"),
                }
            }
            __ => return None,
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
