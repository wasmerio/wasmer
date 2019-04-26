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
//use zbox::init_env as zbox_init_env;

pub const MAX_SYMLINKS: usize = 100;

#[derive(Debug)]
pub enum WasiFile {
    #[allow(dead_code)]
    ZboxFile(zbox::File),
    HostFile(fs::File),
}

impl Write for WasiFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.write(buf),
            WasiFile::HostFile(hf) => hf.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.flush(),
            WasiFile::HostFile(hf) => hf.flush(),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.write_all(buf),
            WasiFile::HostFile(hf) => hf.write_all(buf),
        }
    }

    fn write_fmt(&mut self, fmt: ::std::fmt::Arguments) -> io::Result<()> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.write_fmt(fmt),
            WasiFile::HostFile(hf) => hf.write_fmt(fmt),
        }
    }
}

impl Read for WasiFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.read(buf),
            WasiFile::HostFile(hf) => hf.read(buf),
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.read_to_end(buf),
            WasiFile::HostFile(hf) => hf.read_to_end(buf),
        }
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.read_to_string(buf),
            WasiFile::HostFile(hf) => hf.read_to_string(buf),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.read_exact(buf),
            WasiFile::HostFile(hf) => hf.read_exact(buf),
        }
    }
}

impl Seek for WasiFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        match self {
            WasiFile::ZboxFile(zbf) => zbf.seek(pos),
            WasiFile::HostFile(hf) => hf.seek(pos),
        }
    }
}

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
        handle: WasiFile,
    },
    Dir {
        // parent directory
        parent: Option<Inode>,
        handle: WasiFile,
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
    pub fn new(preopened_files: &[String]) -> Result<Self, String> {
        debug!("wasi::fs::init");
        /*zbox_init_env();
        debug!("wasi::fs::repo");
        let repo = RepoOpener::new()
        .create(true)
        .open("mem://wasmer-test-fs", "")
        .map_err(|e| e.to_string())?;*/
        debug!("wasi::fs::inodes");
        let inodes = Arena::new();
        let mut wasi_fs = Self {
            //repo: repo,
            name_map: HashMap::new(),
            inodes: inodes,
            fd_map: HashMap::new(),
            next_fd: Cell::new(3),
            inode_counter: Cell::new(1000),
        };
        let cur_dir = std::env::current_dir().expect("failed to get current directory");
        let mut canonical_preopen_dirs: Vec<PathBuf> = preopened_files
            .iter()
            .map(|pod| fs::canonicalize(pod))
            .collect::<Result<Vec<PathBuf>, _>>()
            .map_err(|err| format!("Error while preopening files: {}", err))?;
        // TODO: more aggressively canonicalize these to remove _all_ ..
        // that is, we should pin to specific directories now (.. + symlinks means the directories can change)
        let mut canonical_preopen_dirs: Vec<(PathBuf, String)> = canonical_preopen_dirs
            .drain(..)
            .zip(preopened_files.iter().cloned())
            .collect();
        canonical_preopen_dirs
            .sort_by(|(canonical_a, _), (canonical_b, _)| canonical_a.cmp(canonical_b));
        /*let canonical_preopen_dirs: Vec<PathBuf> = canonical_preopen_dirs
        .drain(..)
        .map(|path| {
            if path.starts_with(&cur_dir) {
                PathBuf::from(path.strip_prefix(&cur_dir).unwrap())
            } else {
                path
            }
        })
        .collect();*/
        // hack(mark): rather than grouping by least common ancestor, we just assume there is either 0 or 1
        // common ancestor and use that (to be fixed once this gets working)
        let lca: PathBuf = canonical_preopen_dirs
            .iter()
            .min_by(|(a, _), (b, _)| a.to_string_lossy().len().cmp(&b.to_string_lossy().len()))
            .map(|(pb, _)| pb.clone())
            .unwrap_or(PathBuf::from(""));
        dbg!(&lca);
        dbg!(&canonical_preopen_dirs);
        std::env::set_current_dir(&lca);

        // set of preopen dirs for parent detection
        let mut preopen_dir_map: HashMap<PathBuf, u32> = HashMap::new();
        for (canonical_file, raw) in canonical_preopen_dirs.iter().cloned() {
            let canonical_file = if canonical_file == lca {
                PathBuf::from(".")
            } else {
                PathBuf::from(canonical_file.strip_prefix(&lca).unwrap_or(&canonical_file))
            };
            dbg!(&canonical_file);
            debug!(
                "Attempting to preopen {}",
                &canonical_file.to_string_lossy()
            );
            // TODO: think about this
            let default_rights = 0x1FFFFFFF; // all rights
            let cur_file: fs::File = fs::OpenOptions::new()
                .read(true)
                .open(&canonical_file)
                .expect("Could not find file");
            let cur_file_metadata = cur_file
                .metadata()
                .expect("Could not get metadata for file");
            // return how deep we had to go to find a parent
            let mut depth_seen_and_parent_fd: Option<(usize, u32, &Path)> = None;
            for (i, parent) in canonical_file.ancestors().enumerate().skip(1) {
                if let Some(parent_fd_idx) = preopen_dir_map.get(parent) {
                    depth_seen_and_parent_fd = Some((i, *parent_fd_idx, parent));
                    break;
                }
            }

            // find parent
            let parent = if let Some((depth_seen, parent_fd_idx, parent_path)) =
                depth_seen_and_parent_fd
            {
                // depth seen can be computed from the result of strip_prefix
                // possibly worth rming depth_seen
                if depth_seen > 1 {
                    let dist_path = canonical_file
                        .strip_prefix(parent_path)
                        .expect("Failed to reduce child path by parent path");
                    let parent_inode = wasi_fs.fd_map[&parent_fd_idx].inode;
                    let mut cur_parent = parent_inode;
                    let mut cumulative_path = canonical_file.clone();
                    for comp in dist_path.components() {
                        if let Kind::Dir { entries, .. } = &mut wasi_fs.inodes[parent_inode].kind {
                            if let Some(ent) =
                                entries.get(&comp.as_os_str().to_string_lossy().to_string())
                            {
                                cur_parent = *ent;
                            } else {
                                // create it
                                // TODO: massive refactoring
                                cumulative_path.push(comp);
                                let mid_dir_name = comp.as_os_str().to_string_lossy().to_string();

                                let default_rights = 0x1FFFFFFF; // all rights
                                let cur_file: fs::File = fs::OpenOptions::new()
                                    .read(true)
                                    .open(&cumulative_path)
                                    .expect("Could not find file");
                                let mid_dir_metadata = cur_file
                                    .metadata()
                                    .expect("Could not get metadata for intermediate file");

                                let kind = if mid_dir_metadata.is_dir() {
                                    Kind::Dir {
                                        parent: Some(cur_parent),
                                        handle: WasiFile::HostFile(cur_file),
                                        entries: Default::default(),
                                    }
                                } else {
                                    // TODO: properly handle symlinks
                                    unreachable!(
                                        "When connecting intermediate directories while preopening directories, found something that's not a directory"
                                    );
                                };

                                let inode_val = InodeVal::from_file_metadata(
                                    &mid_dir_metadata,
                                    mid_dir_name.clone(),
                                    true,
                                    kind,
                                );

                                let inode = wasi_fs.inodes.insert(inode_val);
                                let new_inode = wasi_fs.inode_counter.get();
                                wasi_fs.inodes[inode].stat.st_ino = new_inode;
                                wasi_fs.inode_counter.replace(new_inode + 1);
                                // reborrow to insert entry
                                if let Kind::Dir { entries, .. } =
                                    &mut wasi_fs.inodes[cur_parent].kind
                                {
                                    entries.insert(mid_dir_name, inode);

                                    cur_parent = inode;
                                }
                                let fd = wasi_fs
                                    .create_fd(default_rights, default_rights, 0, inode)
                                    .expect("Could not open fd");
                                preopen_dir_map.insert(cumulative_path.clone(), fd);
                            }
                        } else {
                            unreachable!(
                                "Internal error: pre-opened parent directory is not a directory"
                            );
                        }
                    }
                    Some(cur_parent)
                } else {
                    debug!("Connecting as parent");
                    Some(wasi_fs.fd_map[&parent_fd_idx].inode)
                }
            } else {
                None
            };
            let file_name = canonical_file
                .file_name()
                .map(|file_name| file_name.to_string_lossy().to_string())
                // TODO: seriously check this, this looks like a bad bug in the future
                .unwrap_or(String::from("."));
            let kind = if cur_file_metadata.is_dir() {
                Kind::Dir {
                    parent,
                    handle: WasiFile::HostFile(cur_file),
                    entries: Default::default(),
                }
            } else {
                return Err(format!(
                    "WASI only supports pre-opened directories; found \"{}\"",
                    &file_name,
                ));
            };
            let inode_val =
                InodeVal::from_file_metadata(&cur_file_metadata, file_name.clone(), true, kind);

            let inode = wasi_fs.inodes.insert(inode_val);
            let new_inode = wasi_fs.inode_counter.get();
            wasi_fs.inodes[inode].stat.st_ino = new_inode;
            wasi_fs.inode_counter.replace(new_inode + 1);
            let fd = wasi_fs
                .create_fd(default_rights, default_rights, 0, inode)
                .expect("Could not open fd");

            // now connect the parent to the child
            if let Some(p) = parent {
                if let Kind::Dir { entries, .. } = &mut wasi_fs.inodes[p].kind {
                    entries.insert(file_name, inode);
                } else {
                    unreachable!("Internal error: pre-opened parent directory is not a directory");
                }
            }

            preopen_dir_map.insert(canonical_file, fd);
        }
        std::env::set_current_dir(&cur_dir);
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
            fs_rights_inheriting: fd.rights_inheriting,
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

    pub fn get_base_path_for_directory(&self, directory: Inode) -> Option<String> {
        let mut path_segments = vec![];
        let mut cur_inode = directory;
        loop {
            path_segments.push(self.inodes[cur_inode].name.clone());

            if let Kind::Dir { parent, .. } = &self.inodes[cur_inode].kind {
                if let Some(p_inode) = parent {
                    cur_inode = *p_inode;
                } else {
                    break;
                }
            } else {
                return None;
            }
        }

        path_segments.reverse();
        Some(
            path_segments
                .iter()
                .fold("".to_string(), |a, b| a + "/" + b),
        )
    }
}

#[derive(Debug)]
pub struct WasiState<'a> {
    pub fs: WasiFs,
    pub args: &'a [Vec<u8>],
    pub envs: &'a [Vec<u8>],
}
