//! WARNING: the API exposed here is unstable and very experimental.  Certain things are not ready
//! yet and may be broken in patch releases.  If you're using this and have any specific needs,
//! please let us know here https://github.com/wasmerio/wasmer/issues/583 or by filing an issue.
//!
//! Wasmer always has a virtual root directory located at `/` at which all pre-opened directories can
//! be found.  It's possible to traverse between preopened directories this way as well (for example
//! `preopen-dir1/../preopen-dir2`).
//!
//! A preopened directory is a directory or directory + name combination passed into the
//! `generate_import_object` function.  These are directories that the caller has given
//! the WASI module permission to access.
//!
//! You can implement `WasiFile` for your own types to get custom behavior and extend WASI, see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs).

mod builder;
mod types;

pub use self::builder::*;
pub use self::types::*;
use crate::syscalls::types::*;
use generational_arena::Arena;
pub use generational_arena::Index as Inode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{
    borrow::Borrow,
    cell::Cell,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};
use wasmer_runtime_core::{debug, vm::Ctx};

/// the fd value of the virtual root
pub const VIRTUAL_ROOT_FD: __wasi_fd_t = 3;
/// all the rights enabled
pub const ALL_RIGHTS: __wasi_rights_t = 0x1FFFFFFF;
const STDIN_DEFAULT_RIGHTS: __wasi_rights_t = __WASI_RIGHT_FD_DATASYNC
    | __WASI_RIGHT_FD_READ
    | __WASI_RIGHT_FD_SYNC
    | __WASI_RIGHT_FD_ADVISE
    | __WASI_RIGHT_FD_FILESTAT_GET
    | __WASI_RIGHT_POLL_FD_READWRITE;
const STDOUT_DEFAULT_RIGHTS: __wasi_rights_t = __WASI_RIGHT_FD_DATASYNC
    | __WASI_RIGHT_FD_WRITE
    | __WASI_RIGHT_FD_SYNC
    | __WASI_RIGHT_FD_ADVISE
    | __WASI_RIGHT_FD_FILESTAT_GET
    | __WASI_RIGHT_POLL_FD_READWRITE;
const STDERR_DEFAULT_RIGHTS: __wasi_rights_t = STDOUT_DEFAULT_RIGHTS;

/// Get WasiState from a Ctx
/// This function is unsafe because it must be called on a WASI Ctx
pub unsafe fn get_wasi_state(ctx: &mut Ctx) -> &mut WasiState {
    &mut *(ctx.data as *mut WasiState)
}

/// A completely aribtrary "big enough" number used as the upper limit for
/// the number of symlinks that can be traversed when resolving a path
pub const MAX_SYMLINKS: u32 = 128;

/// A file that Wasi knows about that may or may not be open
#[derive(Debug, Serialize, Deserialize)]
pub struct InodeVal {
    pub stat: __wasi_filestat_t,
    pub is_preopened: bool,
    pub name: String,
    pub kind: Kind,
}

/// The core of the filesystem abstraction.  Includes directories,
/// files, and symlinks.
#[derive(Debug, Serialize, Deserialize)]
pub enum Kind {
    File {
        /// the open file, if it's open
        handle: Option<Box<dyn WasiFile>>,
        /// The path on the host system where the file is located
        /// This is deprecated and will be removed soon
        path: PathBuf,
        /// Marks the file as a special file that only one `fd` can exist for
        /// This is useful when dealing with host-provided special files that
        /// should be looked up by path
        /// TOOD: clarify here?
        fd: Option<u32>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Fd {
    pub rights: __wasi_rights_t,
    pub rights_inheriting: __wasi_rights_t,
    pub flags: __wasi_fdflags_t,
    pub offset: u64,
    /// Flags that determine how the [`Fd`] can be used.
    ///
    /// Used when reopening a [`HostFile`] during [`WasiState`] deserialization.
    pub open_flags: u16,
    pub inode: Inode,
}

impl Fd {
    /// This [`Fd`] can be used with read system calls.
    pub const READ: u16 = 1;
    /// This [`Fd`] can be used with write system calls.
    pub const WRITE: u16 = 2;
    /// This [`Fd`] can append in write system calls. Note that the append
    /// permission implies the write permission.
    pub const APPEND: u16 = 4;
    /// This [`Fd`] will delete everything before writing. Note that truncate
    /// permissions require the write permission.
    ///
    /// This permission is currently unused when deserializing [`WasiState`].
    pub const TRUNCATE: u16 = 8;
    /// This [`Fd`] may create a file before writing to it. Note that create
    /// permissions require write permissions.
    ///
    /// This permission is currently unused when deserializing [`WasiState`].
    pub const CREATE: u16 = 16;
}

#[derive(Debug, Serialize, Deserialize)]
/// Warning, modifying these fields directly may cause invariants to break and
/// should be considered unsafe.  These fields may be made private in a future release
pub struct WasiFs {
    //pub repo: Repo,
    pub preopen_fds: Vec<u32>,
    pub name_map: HashMap<String, Inode>,
    pub inodes: Arena<InodeVal>,
    pub fd_map: HashMap<u32, Fd>,
    pub next_fd: Cell<u32>,
    inode_counter: Cell<u64>,
    /// for fds still open after the file has been deleted
    pub orphan_fds: HashMap<Inode, InodeVal>,
}

impl WasiFs {
    pub fn new(
        preopened_dirs: &[PathBuf],
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
            orphan_fds: HashMap::new(),
        };
        wasi_fs.create_stdin();
        wasi_fs.create_stdout();
        wasi_fs.create_stderr();

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
                .create_fd(root_rights, root_rights, 0, Fd::READ, inode)
                .map_err(|e| format!("Could not create root fd: {}", e))?;
            wasi_fs.preopen_fds.push(fd);
            inode
        };

        debug!("wasi::fs::preopen_dirs");
        for dir in preopened_dirs {
            debug!("Attempting to preopen {}", &dir.to_string_lossy());
            // TODO: think about this
            let default_rights = 0x1FFFFFFF; // all rights
            let cur_dir_metadata = dir.metadata().map_err(|e| {
                format!(
                    "Could not get metadata for file {:?}: {}",
                    dir,
                    e.to_string()
                )
            })?;
            let kind = if cur_dir_metadata.is_dir() {
                Kind::Dir {
                    parent: Some(root_inode),
                    path: dir.clone(),
                    entries: Default::default(),
                }
            } else {
                return Err(format!(
                    "WASI only supports pre-opened directories right now; found \"{}\"",
                    &dir.to_string_lossy()
                ));
            };
            // TODO: handle nested pats in `file`
            let inode = wasi_fs
                .create_inode(kind, true, dir.to_string_lossy().into_owned())
                .map_err(|e| {
                    format!(
                        "Failed to create inode for preopened dir: WASI error code: {}",
                        e
                    )
                })?;
            let fd = wasi_fs
                .create_fd(
                    default_rights,
                    default_rights,
                    0,
                    Fd::READ | Fd::WRITE,
                    inode,
                )
                .map_err(|e| format!("Could not open fd for file {:?}: {}", dir, e))?;
            if let Kind::Root { entries } = &mut wasi_fs.inodes[root_inode].kind {
                // todo handle collisions
                assert!(entries
                    .insert(dir.to_string_lossy().into_owned(), inode)
                    .is_none())
            }
            wasi_fs.preopen_fds.push(fd);
        }
        debug!("wasi::fs::mapped_dirs");
        for (alias, real_dir) in mapped_dirs {
            debug!("Attempting to open {:?} at {}", real_dir, alias);
            // TODO: think about this
            let default_rights = 0x1FFFFFFF; // all rights
            let cur_dir_metadata = real_dir.metadata().map_err(|e| {
                format!(
                    "Could not get metadata for file {:?}: {}",
                    &real_dir,
                    e.to_string()
                )
            })?;
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
                .create_fd(
                    default_rights,
                    default_rights,
                    0,
                    Fd::READ | Fd::WRITE,
                    inode,
                )
                .map_err(|e| format!("Could not open fd for file {:?}: {}", &real_dir, e))?;
            if let Kind::Root { entries } = &mut wasi_fs.inodes[root_inode].kind {
                // todo handle collisions
                assert!(entries.insert(alias.clone(), inode).is_none());
            }
            wasi_fs.preopen_fds.push(fd);
        }

        debug!("wasi::fs::end");
        Ok(wasi_fs)
    }

    /// Get the `WasiFile` object at stdout
    pub fn stdout(&self) -> Result<&Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get(__WASI_STDOUT_FILENO)
    }
    /// Get the `WasiFile` object at stdout mutably
    pub fn stdout_mut(&mut self) -> Result<&mut Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get_mut(__WASI_STDOUT_FILENO)
    }

    /// Get the `WasiFile` object at stderr
    pub fn stderr(&self) -> Result<&Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get(__WASI_STDERR_FILENO)
    }
    /// Get the `WasiFile` object at stderr mutably
    pub fn stderr_mut(&mut self) -> Result<&mut Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get_mut(__WASI_STDERR_FILENO)
    }

    /// Get the `WasiFile` object at stdin
    pub fn stdin(&self) -> Result<&Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get(__WASI_STDIN_FILENO)
    }
    /// Get the `WasiFile` object at stdin mutably
    pub fn stdin_mut(&mut self) -> Result<&mut Option<Box<dyn WasiFile>>, WasiFsError> {
        self.std_dev_get_mut(__WASI_STDIN_FILENO)
    }

    /// Internal helper function to get a standard device handle.
    /// Expects one of `__WASI_STDIN_FILENO`, `__WASI_STDOUT_FILENO`, `__WASI_STDERR_FILENO`.
    fn std_dev_get(&self, fd: __wasi_fd_t) -> Result<&Option<Box<dyn WasiFile>>, WasiFsError> {
        if let Some(fd) = self.fd_map.get(&fd) {
            if let Kind::File { ref handle, .. } = self.inodes[fd.inode].kind {
                Ok(handle)
            } else {
                // Our public API should ensure that this is not possible
                unreachable!("Non-file found in standard device location")
            }
        } else {
            // this should only trigger if we made a mistake in this crate
            Err(WasiFsError::NoDevice)
        }
    }
    /// Internal helper function to mutably get a standard device handle.
    /// Expects one of `__WASI_STDIN_FILENO`, `__WASI_STDOUT_FILENO`, `__WASI_STDERR_FILENO`.
    fn std_dev_get_mut(
        &mut self,
        fd: __wasi_fd_t,
    ) -> Result<&mut Option<Box<dyn WasiFile>>, WasiFsError> {
        if let Some(fd) = self.fd_map.get_mut(&fd) {
            if let Kind::File { ref mut handle, .. } = self.inodes[fd.inode].kind {
                Ok(handle)
            } else {
                // Our public API should ensure that this is not possible
                unreachable!("Non-file found in standard device location")
            }
        } else {
            // this should only trigger if we made a mistake in this crate
            Err(WasiFsError::NoDevice)
        }
    }

    /// Returns the next available inode index for creating a new inode.
    fn get_next_inode_index(&mut self) -> u64 {
        let next = self.inode_counter.get();
        self.inode_counter.set(next + 1);
        next
    }

    /// like create dir all, but it also opens it
    /// Function is unsafe because it may break invariants and hasn't been tested.
    /// This is an experimental function and may be removed
    // dead code because this is an API for external use
    #[allow(dead_code)]
    pub unsafe fn open_dir_all(
        &mut self,
        base: __wasi_fd_t,
        name: String,
        rights: __wasi_rights_t,
        rights_inheriting: __wasi_rights_t,
        flags: __wasi_fdflags_t,
    ) -> Result<__wasi_fd_t, WasiFsError> {
        let base_fd = self.get_fd(base).map_err(WasiFsError::from_wasi_err)?;
        // TODO: check permissions here? probably not, but this should be
        // an explicit choice, so justify it in a comment when we remove this one
        let mut cur_inode = base_fd.inode;

        let path: &Path = Path::new(&name);
        //let n_components = path.components().count();
        for c in path.components() {
            let segment_name = c.as_os_str().to_string_lossy().to_string();
            match &self.inodes[cur_inode].kind {
                Kind::Dir { ref entries, .. } | Kind::Root { ref entries } => {
                    if let Some(_entry) = entries.get(&segment_name) {
                        // TODO: this should be fixed
                        return Err(WasiFsError::AlreadyExists);
                    }

                    let kind = Kind::Dir {
                        parent: Some(cur_inode),
                        path: PathBuf::from(""),
                        entries: HashMap::new(),
                    };

                    let inode =
                        self.create_inode_with_default_stat(kind, false, segment_name.clone());
                    // reborrow to insert
                    match &mut self.inodes[cur_inode].kind {
                        Kind::Dir {
                            ref mut entries, ..
                        }
                        | Kind::Root { ref mut entries } => {
                            entries.insert(segment_name, inode);
                        }
                        _ => unreachable!("Dir or Root became not Dir or Root"),
                    }

                    cur_inode = inode;
                }
                _ => return Err(WasiFsError::BaseNotDirectory),
            }
        }

        // TODO: review open flags (read, write); they were added without consideration
        self.create_fd(
            rights,
            rights_inheriting,
            flags,
            Fd::READ | Fd::WRITE,
            cur_inode,
        )
        .map_err(WasiFsError::from_wasi_err)
    }

    /// Opens a user-supplied file in the directory specified with the
    /// name and flags given
    // dead code because this is an API for external use
    #[allow(dead_code)]
    pub fn open_file_at(
        &mut self,
        base: __wasi_fd_t,
        file: Box<dyn WasiFile>,
        open_flags: u16,
        name: String,
        rights: __wasi_rights_t,
        rights_inheriting: __wasi_rights_t,
        flags: __wasi_fdflags_t,
    ) -> Result<__wasi_fd_t, WasiFsError> {
        let base_fd = self.get_fd(base).map_err(WasiFsError::from_wasi_err)?;
        // TODO: check permissions here? probably not, but this should be
        // an explicit choice, so justify it in a comment when we remove this one
        let base_inode = base_fd.inode;

        match &self.inodes[base_inode].kind {
            Kind::Dir { ref entries, .. } | Kind::Root { ref entries } => {
                if let Some(_entry) = entries.get(&name) {
                    // TODO: eventually change the logic here to allow overwrites
                    return Err(WasiFsError::AlreadyExists);
                }

                let kind = Kind::File {
                    handle: Some(file),
                    path: PathBuf::from(""),
                    fd: Some(self.next_fd.get()),
                };

                let inode = self
                    .create_inode(kind, false, name.clone())
                    .map_err(|_| WasiFsError::IOError)?;
                // reborrow to insert
                match &mut self.inodes[base_inode].kind {
                    Kind::Dir {
                        ref mut entries, ..
                    }
                    | Kind::Root { ref mut entries } => {
                        entries.insert(name, inode);
                    }
                    _ => unreachable!("Dir or Root became not Dir or Root"),
                }

                self.create_fd(rights, rights_inheriting, flags, open_flags, inode)
                    .map_err(WasiFsError::from_wasi_err)
            }
            _ => Err(WasiFsError::BaseNotDirectory),
        }
    }

    /// Change the backing of a given file descriptor
    /// Returns the old backing
    /// TODO: add examples
    #[allow(dead_code)]
    pub fn swap_file(
        &mut self,
        fd: __wasi_fd_t,
        file: Box<dyn WasiFile>,
    ) -> Result<Option<Box<dyn WasiFile>>, WasiFsError> {
        let mut ret = Some(file);
        match fd {
            __WASI_STDIN_FILENO => {
                std::mem::swap(self.stdin_mut()?, &mut ret);
            }
            __WASI_STDOUT_FILENO => {
                std::mem::swap(self.stdout_mut()?, &mut ret);
            }
            __WASI_STDERR_FILENO => {
                std::mem::swap(self.stderr_mut()?, &mut ret);
            }
            _ => {
                let base_fd = self.get_fd(fd).map_err(WasiFsError::from_wasi_err)?;
                let base_inode = base_fd.inode;

                match &mut self.inodes[base_inode].kind {
                    Kind::File { ref mut handle, .. } => {
                        std::mem::swap(handle, &mut ret);
                    }
                    _ => return Err(WasiFsError::NotAFile),
                }
            }
        }

        Ok(ret)
    }

    /// refresh size from filesystem
    pub(crate) fn filestat_resync_size(
        &mut self,
        fd: __wasi_fd_t,
    ) -> Result<__wasi_filesize_t, __wasi_errno_t> {
        let fd = self.fd_map.get_mut(&fd).ok_or(__WASI_EBADF)?;
        match &mut self.inodes[fd.inode].kind {
            Kind::File { handle, .. } => {
                if let Some(h) = handle {
                    let new_size = h.size();
                    self.inodes[fd.inode].stat.st_size = new_size;
                    Ok(new_size as __wasi_filesize_t)
                } else {
                    Err(__WASI_EBADF)
                }
            }
            Kind::Dir { .. } | Kind::Root { .. } => Err(__WASI_EISDIR),
            _ => Err(__WASI_EINVAL),
        }
    }

    /// Internal part of the core path resolution function which implements path
    /// traversal logic such as resolving relative path segments (such as
    /// `.` and `..`) and resolving symlinks (while preventing infinite
    /// loops/stack overflows).
    ///
    /// TODO: expand upon exactly what the state of the returned value is,
    /// explaining lazy-loading from the real file system and synchronizing
    /// between them.
    ///
    /// This is where a lot of the magic happens, be very careful when editing
    /// this code.
    ///
    /// TODO: write more tests for this code
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
        let n_components = path.components().count();
        // TODO: rights checks
        'path_iter: for (i, component) in path.components().enumerate() {
            // used to terminate symlink resolution properly
            let last_component = i + 1 == n_components;
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
                            let metadata = file.symlink_metadata().ok().ok_or(__WASI_EINVAL)?;
                            let file_type = metadata.file_type();
                            // we want to insert newly opened dirs and files, but not transient symlinks
                            // TODO: explain why (think about this deeply when well rested)
                            let mut should_insert = false;

                            let kind = if file_type.is_dir() {
                                should_insert = true;
                                // load DIR
                                Kind::Dir {
                                    parent: Some(cur_inode),
                                    path: file.clone(),
                                    entries: Default::default(),
                                }
                            } else if file_type.is_file() {
                                should_insert = true;
                                // load file
                                Kind::File {
                                    handle: None,
                                    path: file.clone(),
                                    fd: None,
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

                            let new_inode =
                                self.create_inode(kind, false, file.to_string_lossy().to_string())?;
                            if should_insert {
                                if let Kind::Dir {
                                    ref mut entries, ..
                                } = &mut self.inodes[cur_inode].kind
                                {
                                    entries.insert(
                                        component.as_os_str().to_string_lossy().to_string(),
                                        new_inode,
                                    );
                                }
                            }
                            cur_inode = new_inode;

                            if loop_for_symlink && follow_symlinks {
                                debug!("Following symlink to {:?}", cur_inode);
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
                        debug!("Following symlink recursively");
                        let symlink_inode = self.get_inode_at_path_inner(
                            new_base_dir,
                            &new_path,
                            symlink_count + 1,
                            follow_symlinks,
                        )?;
                        cur_inode = symlink_inode;
                        // if we're at the very end and we found a file, then we're done
                        // TODO: figure out if this should also happen for directories?
                        if let Kind::File { .. } = &self.inodes[cur_inode].kind {
                            // check if on last step
                            if last_component {
                                break 'symlink_resolution;
                            }
                        }
                        continue 'symlink_resolution;
                    }
                }
                break 'symlink_resolution;
            }
        }

        Ok(cur_inode)
    }

    /// Splits a path into the first preopened directory that is a parent of it,
    /// if such a preopened directory exists, and the rest of the path.
    ///
    /// NOTE: this behavior seems to be not the same as what libpreopen is
    /// doing in WASI.
    ///
    /// TODO: evaluate users of this function and explain why this behavior is
    /// not the same as libpreopen or update its behavior to be the same.
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

    // if this is still dead code and the year is 2020 or later, please delete this function
    #[allow(dead_code)]
    pub(crate) fn path_relative_to_fd(
        &self,
        fd: __wasi_fd_t,
        inode: Inode,
    ) -> Result<PathBuf, __wasi_errno_t> {
        let mut stack = vec![];
        let base_fd = self.get_fd(fd)?;
        let base_inode = base_fd.inode;
        let mut cur_inode = inode;

        while cur_inode != base_inode {
            stack.push(self.inodes[cur_inode].name.clone());
            match &self.inodes[cur_inode].kind {
                Kind::Dir { parent, .. } => {
                    if let Some(p) = parent {
                        cur_inode = *p;
                    }
                }
                _ => return Err(__WASI_EINVAL),
            }
        }

        let mut out = PathBuf::new();
        for p in stack.iter().rev() {
            out.push(p);
        }
        Ok(out)
    }

    /// finds the number of directories between the fd and the inode if they're connected
    /// expects inode to point to a directory
    pub(crate) fn path_depth_from_fd(
        &self,
        fd: __wasi_fd_t,
        inode: Inode,
    ) -> Result<usize, __wasi_errno_t> {
        let mut counter = 0;
        let base_fd = self.get_fd(fd)?;
        let base_inode = base_fd.inode;
        let mut cur_inode = inode;

        while cur_inode != base_inode {
            counter += 1;
            match &self.inodes[cur_inode].kind {
                Kind::Dir { parent, .. } => {
                    if let Some(p) = parent {
                        cur_inode = *p;
                    }
                }
                _ => return Err(__WASI_EINVAL),
            }
        }

        Ok(counter)
    }

    /// gets a host file from a base directory and a path
    /// this function ensures the fs remains sandboxed
    // NOTE: follow symlinks is super weird right now
    // even if it's false, it still follows symlinks, just not the last
    // symlink so
    // This will be resolved when we have tests asserting the correct behavior
    pub(crate) fn get_inode_at_path(
        &mut self,
        base: __wasi_fd_t,
        path: &str,
        follow_symlinks: bool,
    ) -> Result<Inode, __wasi_errno_t> {
        self.get_inode_at_path_inner(base, path, 0, follow_symlinks)
    }

    /// Returns the parent Dir or Root that the file at a given path is in and the file name
    /// stripped off
    pub(crate) fn get_parent_inode_at_path(
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

    /// gets either a normal inode or an orphaned inode
    pub fn get_inodeval_mut(&mut self, fd: __wasi_fd_t) -> Result<&mut InodeVal, __wasi_errno_t> {
        let inode = self.get_fd(fd)?.inode;
        if let Some(iv) = self.inodes.get_mut(inode) {
            Ok(iv)
        } else {
            self.orphan_fds.get_mut(&inode).ok_or(__WASI_EBADF)
        }
    }

    pub fn filestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let fd = self.get_fd(fd)?;

        Ok(self.inodes[fd.inode].stat)
    }

    pub fn fdstat(&self, fd: __wasi_fd_t) -> Result<__wasi_fdstat_t, __wasi_errno_t> {
        match fd {
            __WASI_STDOUT_FILENO => {
                return Ok(__wasi_fdstat_t {
                    fs_filetype: __WASI_FILETYPE_CHARACTER_DEVICE,
                    fs_flags: 0,
                    fs_rights_base: ALL_RIGHTS,
                    fs_rights_inheriting: ALL_RIGHTS,
                })
            }
            _ => (),
        }
        let fd = self.get_fd(fd)?;

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
            __WASI_STDIN_FILENO => (),
            __WASI_STDOUT_FILENO => self
                .stdout_mut()
                .map_err(WasiFsError::into_wasi_err)?
                .as_mut()
                .and_then(|f| f.flush().ok())
                .ok_or(__WASI_EIO)?,
            __WASI_STDERR_FILENO => self
                .stderr_mut()
                .map_err(WasiFsError::into_wasi_err)?
                .as_mut()
                .and_then(|f| f.flush().ok())
                .ok_or(__WASI_EIO)?,
            _ => {
                let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;
                if fd.rights & __WASI_RIGHT_FD_DATASYNC == 0 {
                    return Err(__WASI_EACCES);
                }

                let inode = &mut self.inodes[fd.inode];

                match &mut inode.kind {
                    Kind::File { handle, .. } => {
                        if let Some(file) = handle {
                            file.flush().map_err(|_| __WASI_EIO)?
                        } else {
                            return Err(__WASI_EIO);
                        }
                    }
                    // TODO: verify this behavior
                    Kind::Dir { .. } => return Err(__WASI_EISDIR),
                    Kind::Symlink { .. } => unimplemented!("WasiFs::flush Kind::Symlink"),
                    Kind::Buffer { .. } => (),
                    _ => return Err(__WASI_EIO),
                }
            }
        }
        Ok(())
    }

    /// Creates an inode and inserts it given a Kind and some extra data
    pub(crate) fn create_inode(
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

    /// creates an inode and inserts it given a Kind, does not assume the file exists to
    pub(crate) fn create_inode_with_default_stat(
        &mut self,
        kind: Kind,
        is_preopened: bool,
        name: String,
    ) -> Inode {
        let mut stat = __wasi_filestat_t::default();
        stat.st_ino = self.get_next_inode_index();

        self.inodes.insert(InodeVal {
            stat,
            is_preopened,
            name,
            kind,
        })
    }

    pub fn create_fd(
        &mut self,
        rights: __wasi_rights_t,
        rights_inheriting: __wasi_rights_t,
        flags: __wasi_fdflags_t,
        open_flags: u16,
        inode: Inode,
    ) -> Result<__wasi_fd_t, __wasi_errno_t> {
        let idx = self.next_fd.get();
        self.next_fd.set(idx + 1);
        self.fd_map.insert(
            idx,
            Fd {
                rights,
                rights_inheriting,
                flags,
                offset: 0,
                open_flags,
                inode,
            },
        );
        Ok(idx)
    }

    /// This function is unsafe because it's the caller's responsibility to ensure that
    /// all refences to the given inode have been removed from the filesystem
    ///
    /// returns the inode if it existed and was removed
    pub unsafe fn remove_inode(&mut self, inode: Inode) -> Option<InodeVal> {
        self.inodes.remove(inode)
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
            stat,
            is_preopened: true,
            name: "/".to_string(),
            kind: root_kind,
        })
    }

    fn create_stdout(&mut self) {
        self.create_std_dev_inner(
            Box::new(Stdout),
            "stdout",
            __WASI_STDOUT_FILENO,
            STDOUT_DEFAULT_RIGHTS,
            __WASI_FDFLAG_APPEND,
        );
    }
    fn create_stdin(&mut self) {
        self.create_std_dev_inner(
            Box::new(Stdin),
            "stdin",
            __WASI_STDIN_FILENO,
            STDIN_DEFAULT_RIGHTS,
            0,
        );
    }
    fn create_stderr(&mut self) {
        self.create_std_dev_inner(
            Box::new(Stderr),
            "stderr",
            __WASI_STDERR_FILENO,
            STDERR_DEFAULT_RIGHTS,
            __WASI_FDFLAG_APPEND,
        );
    }

    fn create_std_dev_inner(
        &mut self,
        handle: Box<dyn WasiFile>,
        name: &'static str,
        raw_fd: __wasi_fd_t,
        rights: __wasi_rights_t,
        fd_flags: __wasi_fdflags_t,
    ) {
        let stat = __wasi_filestat_t {
            st_filetype: __WASI_FILETYPE_CHARACTER_DEVICE,
            st_ino: self.get_next_inode_index(),
            ..__wasi_filestat_t::default()
        };
        let kind = Kind::File {
            fd: Some(raw_fd),
            handle: Some(handle),
            path: "".into(),
        };
        let inode = self.inodes.insert(InodeVal {
            stat,
            is_preopened: true,
            name: name.to_string(),
            kind,
        });
        self.fd_map.insert(
            raw_fd,
            Fd {
                rights,
                rights_inheriting: 0,
                flags: fd_flags,
                // since we're not calling open on this, we don't need open flags
                open_flags: 0,
                offset: 0,
                inode,
            },
        );
    }

    pub fn get_stat_for_kind(&self, kind: &Kind) -> Option<__wasi_filestat_t> {
        let md = match kind {
            Kind::File { handle, path, .. } => match handle {
                Some(wf) => {
                    return Some(__wasi_filestat_t {
                        st_filetype: __WASI_FILETYPE_REGULAR_FILE,
                        st_size: wf.size(),
                        st_atim: wf.last_accessed(),
                        st_mtim: wf.last_modified(),
                        st_ctim: wf.created_time(),

                        ..__wasi_filestat_t::default()
                    })
                }
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

    /// Closes an open FD, handling all details such as FD being preopen
    pub(crate) fn close_fd(&mut self, fd: __wasi_fd_t) -> Result<(), __wasi_errno_t> {
        let inodeval_mut = self.get_inodeval_mut(fd)?;
        let is_preopened = inodeval_mut.is_preopened;

        match &mut inodeval_mut.kind {
            Kind::File { ref mut handle, .. } => {
                let mut empty_handle = None;
                std::mem::swap(handle, &mut empty_handle);
            }
            Kind::Dir { parent, path, .. } => {
                debug!("Closing dir {:?}", &path);
                let key = path
                    .file_name()
                    .ok_or(__WASI_EINVAL)?
                    .to_string_lossy()
                    .to_string();
                if let Some(p) = parent.clone() {
                    match &mut self.inodes[p].kind {
                        Kind::Dir { entries, .. } | Kind::Root { entries } => {
                            self.fd_map.remove(&fd).unwrap();
                            if is_preopened {
                                let mut idx = None;
                                for (i, po_fd) in self.preopen_fds.iter().enumerate() {
                                    if *po_fd == fd {
                                        idx = Some(i);
                                        break;
                                    }
                                }
                                if let Some(i) = idx {
                                    // only remove entry properly if this is the original preopen FD
                                    // calling `path_open` can give you an fd to the same inode as a preopen fd
                                    entries.remove(&key);
                                    self.preopen_fds.remove(i);
                                    // Maybe recursively closes fds if original preopen?
                                }
                            }
                        }
                        _ => unreachable!(
                            "Fatal internal logic error, directory's parent is not a directory"
                        ),
                    }
                } else {
                    // this shouldn't be possible anymore due to Root
                    debug!("HIT UNREACHABLE CODE! Non-root directory does not have a parent");
                    return Err(__WASI_EINVAL);
                }
            }
            Kind::Root { .. } => return Err(__WASI_EACCES),
            Kind::Symlink { .. } | Kind::Buffer { .. } => return Err(__WASI_EINVAL),
        }

        Ok(())
    }
}

/// Top level data type containing all* the state with which WASI can
/// interact.
///
/// * The contents of files are not stored and may be modified by
/// other, concurrently running programs.  Data such as the contents
/// of directories are lazily loaded.
#[derive(Debug, Serialize, Deserialize)]
pub struct WasiState {
    pub fs: WasiFs,
    pub args: Vec<Vec<u8>>,
    pub envs: Vec<Vec<u8>>,
}

impl WasiState {
    /// Create a [`WasiStateBuilder`] to construct a validated instance of
    /// [`WasiState`].
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_wasi::state::WasiState;
    /// WasiState::new("program_name")
    ///    .env(b"HOME", "/home/home".to_string())
    ///    .arg("--help")
    ///    .envs({ let mut hm = std::collections::HashMap::new();
    ///            hm.insert("COLOR_OUTPUT", "TRUE");
    ///            hm.insert("PATH", "/usr/bin");
    ///            hm
    ///          })
    ///    .args(&["--verbose", "list"])
    ///    .preopen_dir("src")
    ///    .map_dir("dot", ".")
    ///    .build()
    ///    .unwrap();
    /// ```
    pub fn new(program_name: &str) -> WasiStateBuilder {
        create_wasi_state(program_name)
    }

    /// Turn the WasiState into bytes
    pub fn freeze(&self) -> Option<Vec<u8>> {
        bincode::serialize(self).ok()
    }

    /// Get a WasiState from bytes
    pub fn unfreeze(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }
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
