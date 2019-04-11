// use wasmer_runtime_abi::vfs::{
//     vfs::Vfs,
//     file_like::{FileLike, Metadata};
// };
use crate::syscalls::types::*;
use generational_arena::{Arena, Index as Inode};
use hashbrown::hash_map::{Entry, HashMap};
use std::{
    cell::Cell,
    io::{self, Write},
    time::SystemTime,
};
use wasmer_runtime_core::debug;
use zbox::{init_env as zbox_init_env, File, FileType, OpenOptions, Repo, RepoOpener};

pub const MAX_SYMLINKS: usize = 100;

pub struct InodeVal {
    pub stat: __wasi_filestat_t,
    pub is_preopened: bool,
    pub name: String,
    pub kind: Kind,
}

#[allow(dead_code)]
pub enum Kind {
    File {
        handle: File,
    },
    Dir {
        handle: File,
        /// The entries of a directory are lazily filled.
        entries: HashMap<String, Inode>,
    },
    Symlink {
        forwarded: Inode,
    },
    Buffer {
        buffer: Vec<u8>,
    },
}

#[derive(Clone)]
pub struct Fd {
    pub rights: __wasi_rights_t,
    pub rights_inheriting: __wasi_rights_t,
    pub flags: __wasi_fdflags_t,
    pub offset: u64,
    pub inode: Inode,
}

pub struct WasiFs {
    // pub repo: Repo,
    pub name_map: HashMap<String, Inode>,
    pub inodes: Arena<InodeVal>,
    pub fd_map: HashMap<u32, Fd>,
    pub next_fd: Cell<u32>,
    pub inode_counter: Cell<u64>,
}

impl WasiFs {
    pub fn new() -> Result<Self, String> {
        debug!("wasi::fs::init");
        zbox_init_env();
        debug!("wasi::fs::repo");
        // let repo = RepoOpener::new()
        //         .create(true)
        //         .open("mem://wasmer-test-fs", "")
        //         .map_err(|e| e.to_string())?;
        debug!("wasi::fs::inodes");
        let inodes = Arena::new();
        let res = Ok(Self {
            // repo: repo,
            name_map: HashMap::new(),
            inodes: inodes,
            fd_map: HashMap::new(),
            next_fd: Cell::new(3),
            inode_counter: Cell::new(1000),
        });
        debug!("wasi::fs::end");
        res
    }

    #[allow(dead_code)]
    fn get_inode(&mut self, path: &str) -> Option<Inode> {
        Some(match self.name_map.entry(path.to_string()) {
            Entry::Occupied(o) => *o.get(),
            Entry::Vacant(v) => {
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

    #[allow(dead_code)]
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

    pub fn filestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_filestat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

        Ok(self.inodes[fd.inode].stat)
    }

    pub fn fdstat(&self, fd: __wasi_fd_t) -> Result<__wasi_fdstat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

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

    pub fn prestat_fd(&self, fd: __wasi_fd_t) -> Result<__wasi_prestat_t, __wasi_errno_t> {
        let fd = self.fd_map.get(&fd).ok_or(__WASI_EBADF)?;

        let inode_val = &self.inodes[fd.inode];

        if inode_val.is_preopened {
            Ok(__wasi_prestat_t {
                pr_type: __WASI_PREOPENTYPE_DIR,
                u: PrestatEnum::Dir {
                    pr_name_len: inode_val.name.len() as u32,
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
                    Kind::File { handle } => handle.flush().map_err(|_| __WASI_EIO)?,
                    // TODO: verify this behavior
                    Kind::Dir { .. } => return Err(__WASI_EISDIR),
                    Kind::Symlink { .. } => unimplemented!(),
                    Kind::Buffer { .. } => (),
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
}

pub struct WasiState<'a> {
    pub fs: WasiFs,
    pub args: &'a [Vec<u8>],
    pub envs: &'a [Vec<u8>],
}
