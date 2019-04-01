// use wasmer_runtime_abi::vfs::{
//     vfs::Vfs,
//     file_like::{FileLike, Metadata};
// };
use crate::syscalls::types::*;
use generational_arena::{Arena, Index as Inode};
use hashbrown::hash_map::{Entry, HashMap};
use std::{
    cell::{Cell, RefCell},
    ops::{Index, IndexMut},
    rc::Rc,
    time::SystemTime,
};
use zbox::{File, FileType, OpenOptions, Repo, RepoOpener};

pub const MAX_SYMLINKS: usize = 100;

pub struct InodeVal {
    stat: __wasi_filestat_t,
    is_preopened: bool,
    name: String,
    kind: Kind,
}

pub enum Kind {
    File {
        handle: File,
    },
    Dir {
        handle: File,
        /// The entries of a directory are lazily filled.
        entries: Vec<Inode>,
    },
    Symlink {
        forwarded: Inode,
    },
    Buffer {
        buffer: Vec<u8>,
    },
}

pub struct Fd {
    rights: __wasi_rights_t,
    flags: __wasi_fdflags_t,
    offset: u64,
    inode: Inode,
}

pub struct WasiFs {
    repo: Repo,
    name_map: HashMap<String, Inode>,
    inodes: Arena<InodeVal>,
    fd_map: HashMap<u32, Fd>,
    next_fd: Cell<u32>,
    inode_counter: Cell<u64>,
}

impl WasiFs {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            repo: RepoOpener::new()
                .create(true)
                .open("mem://📂", "very unsafe pwd")
                .map_err(|e| e.to_string())?,
            name_map: HashMap::new(),
            inodes: Arena::new(),
            fd_map: HashMap::new(),
            next_fd: Cell::new(3),
            inode_counter: Cell::new(1000),
        })
    }

    fn get_inode(&mut self, path: &str) -> Option<Inode> {
        Some(match self.name_map.entry(path.to_string()) {
            Entry::Occupied(o) => *o.get(),
            Entry::Vacant(v) => {
                let file = if let Ok(file) = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(false)
                    .open(&mut self.repo, path)
                {
                    file
                } else {
                    return None;
                };

                let metadata = file.metadata().unwrap();
                let inode_index = {
                    let index = self.inode_counter.get();
                    self.inode_counter.replace(index + 1)
                };

                let systime_to_nanos = |systime: SystemTime| {
                    let duration = systime
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("should always be after unix epoch");
                    duration.as_nanos() as u64
                };

                let inode = self.inodes.insert(InodeVal {
                    stat: __wasi_filestat_t {
                        st_dev: 0,
                        st_ino: inode_index,
                        st_filetype: match metadata.file_type() {
                            FileType::File => __WASI_FILETYPE_REGULAR_FILE,
                            FileType::Dir => __WASI_FILETYPE_DIRECTORY,
                        },
                        st_nlink: 0,
                        st_size: metadata.len() as u64,
                        st_atim: systime_to_nanos(SystemTime::now()),
                        st_mtim: systime_to_nanos(metadata.modified()),
                        st_ctim: systime_to_nanos(metadata.created()),
                    },
                    is_preopened: false,
                    name: path.to_string(),
                    kind: match metadata.file_type() {
                        FileType::File => Kind::File { handle: file },
                        FileType::Dir => Kind::Dir {
                            handle: file,
                            entries: Vec::new(),
                        },
                    },
                });
                v.insert(inode);
                inode
            }
        })
    }

    fn filestat_inode(
        &self,
        inode: Inode,
        flags: __wasi_lookupflags_t,
    ) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let inode_val = &self.inodes[inode];
        if let (true, Kind::Symlink { mut forwarded }) =
            (flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0, &inode_val.kind)
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

    pub fn filestat_path(
        &mut self,
        preopened_fd: __wasi_fd_t,
        flags: __wasi_lookupflags_t,
        path: &str,
    ) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        warn!("Should use preopned_fd: {}", preopened_fd);
        let inode = if let Some(inode) = self.get_inode(path) {
            inode
        } else {
            return Err(__WASI_EINVAL);
        };

        self.filestat_inode(inode, flags)
    }

    pub fn filestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let fd = if let Some(fd) = self.fd_map.get(&fd) {
            fd
        } else {
            return Err(__WASI_EBADF);
        };

        Ok(self.inodes[fd.inode].stat)
    }

    pub fn fdstat(&self, fd: __wasi_fd_t) -> Result<__wasi_fdstat_t, __wasi_errno_t> {
        let fd = if let Some(fd) = self.fd_map.get(&fd) {
            fd
        } else {
            return Err(__WASI_EBADF);
        };

        Ok(__wasi_fdstat_t {
            fs_filetype: match self.inodes[fd.inode].kind {
                Kind::File { .. } => __WASI_FILETYPE_REGULAR_FILE,
                Kind::Dir { .. } => __WASI_FILETYPE_DIRECTORY,
                Kind::Symlink { .. } => __WASI_FILETYPE_SYMBOLIC_LINK,
                _ => __WASI_FILETYPE_UNKNOWN,
            },
            fs_flags: fd.flags,
            fs_rights_base: fd.rights,
            fs_rights_inheriting: fd.rights, // TODO(lachlan): Is this right?
        })
    }
}

pub struct WasiState<'a> {
    pub fs: WasiFs,
    pub args: &'a [Vec<u8>],
    pub envs: &'a [Vec<u8>],
}
