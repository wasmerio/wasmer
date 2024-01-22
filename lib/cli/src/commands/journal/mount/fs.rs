#![allow(unused)]
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    hash::{Hash, Hasher},
    io,
    path::Path,
    sync::{atomic::AtomicU32, Arc, Mutex},
    time::Duration,
};

use fuse::{
    FileAttr, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, Request,
};
use tokio::runtime::Handle;
use virtual_fs::{mem_fs, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, FileSystem, FsError};
use wasmer_wasix::{
    fs::WasiFdSeed,
    journal::{
        copy_journal, Journal, JournalEntry, LogFileJournal, ReadableJournal, WritableJournal,
    },
    types::Oflags,
    wasmer_wasix_types::wasi,
    VIRTUAL_ROOT_FD,
};

#[derive(Debug)]
struct State {
    handle: tokio::runtime::Handle,
    mem_fs: mem_fs::FileSystem,
    inos: HashMap<u64, Cow<'static, str>>,
    fuse_lookup: HashMap<u64, Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>>,
    seed: WasiFdSeed,
    journal_lookup: HashMap<
        u32,
        Arc<tokio::sync::Mutex<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>>>,
    >,
}

#[derive(Debug)]
struct MutexState {
    inner: Mutex<State>,
}

#[derive(Debug)]
pub struct JournalFileSystem {
    handle: tokio::runtime::Handle,
    journal: LogFileJournal,
    state: MutexState,
}

impl JournalFileSystem {
    // Opens the journal and copies all its contents into
    // and memory file system
    pub fn new(journal_path: &Path, fd_seed: WasiFdSeed) -> anyhow::Result<Self> {
        let journal = LogFileJournal::new(journal_path)?;

        let mem_fs = mem_fs::FileSystem::default();
        let state = MutexState {
            inner: Mutex::new(State {
                handle: tokio::runtime::Handle::current(),
                mem_fs,
                inos: Default::default(),
                seed: fd_seed,
                fuse_lookup: Default::default(),
                journal_lookup: Default::default(),
            }),
        };
        copy_journal(&journal, &state)?;

        let ret = Self {
            handle: tokio::runtime::Handle::current(),
            journal,
            state,
        };

        Ok(ret)
    }

    fn reverse_ino(&self, ino: u64) -> Result<Cow<'static, str>, libc::c_int> {
        if ino == 1 {
            return Ok("/".into());
        }
        let path = {
            let mut state = self.state.inner.lock().unwrap();
            match state.inos.get(&ino).cloned() {
                Some(path) => path,
                None => {
                    return Err(libc::ENOENT);
                }
            }
        };
        Ok(path)
    }

    fn attr<'a>(&self, name: Cow<'a, str>) -> Result<FileAttr, libc::c_int> {
        let mut state = self.state.inner.lock().unwrap();

        let res = state.mem_fs.metadata(&Path::new(name.as_ref()));
        match res {
            Ok(meta) => {
                // The ino is just the hash of the name
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                name.hash(&mut hasher);
                let ino = hasher.finish();
                state
                    .inos
                    .entry(ino)
                    .or_insert_with(|| name.into_owned().into());

                // Build a file attr and return it
                Ok(FileAttr {
                    ino,
                    size: meta.len,
                    blocks: (1u64.max(meta.len) - 1 / 512) + 1,
                    atime: time::Timespec::new(meta.accessed as i64, 0),
                    mtime: time::Timespec::new(meta.modified as i64, 0),
                    ctime: time::Timespec::new(meta.created as i64, 0),
                    crtime: time::Timespec::new(meta.created as i64, 0),
                    kind: file_type_to_kind(meta.ft),
                    perm: 0o644,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                    flags: 0,
                })
            }
            Err(FsError::EntryNotFound) => Err(libc::ENOENT),
            Err(_) => Err(libc::EIO),
        }
    }
}

impl WritableJournal for MutexState {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        let ret = entry.estimate_size() as u64;
        let mut state = self.inner.lock().unwrap();
        match entry {
            JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => {
                let handle = state.handle.clone();
                if let Some(file) = state.journal_lookup.get_mut(&fd) {
                    handle.block_on(async {
                        let mut file = file.lock().await;
                        file.seek(io::SeekFrom::Start(offset)).await;
                        file.write_all(&data).await
                    })?;
                }
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                state.journal_lookup.remove(&fd);
            }
            JournalEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => {
                state.seed.clip_val(fd + 1);
                let file = state
                    .mem_fs
                    .new_open_options()
                    .create(o_flags.contains(Oflags::CREATE))
                    .truncate(o_flags.contains(Oflags::TRUNC))
                    .open(path.as_ref())?;
                state
                    .journal_lookup
                    .insert(fd, Arc::new(tokio::sync::Mutex::new(file)));
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                state.seed.clip_val(new_fd + 1);
                if let Some(file) = state.journal_lookup.remove(&old_fd) {
                    state.journal_lookup.insert(new_fd, file);
                }
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                state.seed.clip_val(copied_fd + 1);
                if let Some(file) = state.journal_lookup.get(&original_fd).cloned() {
                    state.journal_lookup.insert(copied_fd, file);
                }
            }
            JournalEntry::CreateDirectoryV1 { fd, path } => {
                state.mem_fs.create_dir(&Path::new(path.as_ref())).ok();
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                state.mem_fs.remove_dir(&Path::new(path.as_ref()))?;
            }
            JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                let handle = state.handle.clone();
                if let Some(file) = state.journal_lookup.get(&fd) {
                    handle.block_on(async {
                        let mut file = file.lock().await;
                        file.set_len(st_size)
                    })?;
                }
            }
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                let handle = state.handle.clone();
                if let Some(file) = state.journal_lookup.get(&fd) {
                    handle.block_on(async {
                        let mut file = file.lock().await;
                        file.set_len(offset + len)
                    })?;
                }
            }
            JournalEntry::UnlinkFileV1 { fd, path } => {
                state.mem_fs.remove_file(&Path::new(path.as_ref()))?;
            }
            JournalEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => {
                let handle = state.handle.clone();
                handle.block_on(async {
                    state
                        .mem_fs
                        .rename(&Path::new(old_path.as_ref()), &Path::new(new_path.as_ref()))
                        .await
                })?;
            }
            JournalEntry::SocketOpenV1 { fd, .. } => {
                state.seed.clip_val(fd + 1);
            }
            JournalEntry::CreatePipeV1 { fd1, fd2 } => {
                state.seed.clip_val(fd1 + 1);
                state.seed.clip_val(fd2 + 1);
            }
            JournalEntry::CreateEventV1 { fd, .. } => {
                state.seed.clip_val(fd + 1);
            }
            JournalEntry::EpollCreateV1 { fd } => {
                state.seed.clip_val(fd + 1);
            }
            JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => {
                state.seed.clip_val(fd + 1);
            }
            JournalEntry::SocketAcceptedV1 { fd, .. } => {
                state.seed.clip_val(fd + 1);
            }
            _ => {}
        }
        Ok(ret)
    }
}

impl JournalFileSystem {
    fn compute_path<'a>(&'a self, parent: u64, name: &'a OsStr) -> Result<Cow<'_, str>, i32> {
        // Get the path from the ino otherwise it is not a known
        // path (this means the other methods have to be hit first)
        let path = match self.reverse_ino(parent) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::compute_path reverse_ino({parent}) errno={err}");
                return Err(err);
            }
        };

        // Add the name as a postfix
        let name = name.to_string_lossy();
        let path = if path.ends_with("/") {
            path + name
        } else {
            path + "/" + name
        };
        Ok(path)
    }
}

impl Filesystem for JournalFileSystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => return reply.error(err),
        };

        match self.attr(path) {
            Ok(meta) => reply.entry(&time::Timespec::new(1, 0), &meta, 0),
            Err(err) => reply.error(err),
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let path = match self.reverse_ino(ino) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::getattr reverse_ino({ino}) errno={err}");
                reply.error(err);
                return;
            }
        };

        match self.attr(path) {
            Ok(meta) => reply.attr(&time::Timespec::new(1, 0), &meta),
            Err(err) => reply.error(err),
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        let path = match self.reverse_ino(ino) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::open reverse_ino({ino}) errno={err}");
                reply.error(err);
                return;
            }
        };

        let mut state = self.state.inner.lock().unwrap();
        let file = state
            .mem_fs
            .new_open_options()
            .write(true)
            .read(true)
            .open(&Path::new(path.as_ref()));
        let file = match file {
            Ok(a) => a,
            Err(FsError::EntryNotFound) => {
                tracing::trace!("fs::open new_open_options({}) err=ENOENT", path);
                reply.error(libc::ENOENT);
                return;
            }
            Err(err) => {
                tracing::trace!("fs::open new_open_options({}) err={}", path, err);
                reply.error(libc::EIO);
                return;
            }
        };

        let fh = state.seed.next_val();
        state
            .journal_lookup
            .insert(fh, Arc::new(tokio::sync::Mutex::new(file)));
        drop(state);

        // Write the journals
        let entry = JournalEntry::OpenFileDescriptorV1 {
            fd: fh,
            dirfd: VIRTUAL_ROOT_FD,
            dirflags: 0,
            path,
            o_flags: wasi::Oflags::empty(),
            fs_rights_base: wasi::Rights::all(),
            fs_rights_inheriting: wasi::Rights::all(),
            fs_flags: wasi::Fdflags::empty(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);

        reply.opened(fh as u64, flags);
    }

    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        flags: u32,
        reply: ReplyCreate,
    ) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => return reply.error(err),
        };

        // The ino is just the hash of the name
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        let ino = hasher.finish();

        // Now create the new file
        let mut state = self.state.inner.lock().unwrap();
        let file = state
            .mem_fs
            .new_open_options()
            .create(true)
            .write(true)
            .read(true)
            .open(&Path::new(path.as_ref()));
        let file = match file {
            Ok(a) => a,
            Err(FsError::EntryNotFound) => {
                tracing::trace!("fs::create new_open_options({}) err=ENOENT", path);
                reply.error(libc::ENOENT);
                return;
            }
            Err(err) => {
                tracing::trace!("fs::create new_open_options({}) err={}", path, err);
                reply.error(libc::EIO);
                return;
            }
        };

        // Create the file and load it into the lookup
        let fh = state.seed.next_val();
        state
            .journal_lookup
            .insert(fh, Arc::new(tokio::sync::Mutex::new(file)));
        drop(state);

        // Write the journals
        let entry = JournalEntry::OpenFileDescriptorV1 {
            fd: fh,
            dirfd: VIRTUAL_ROOT_FD,
            dirflags: 0,
            path,
            o_flags: wasi::Oflags::CREATE,
            fs_rights_base: wasi::Rights::all(),
            fs_rights_inheriting: wasi::Rights::all(),
            fs_flags: wasi::Fdflags::empty(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);

        let now = time::get_time();
        reply.created(
            &time::Timespec::new(1, 0),
            &FileAttr {
                ino,
                size: 0,
                blocks: 0,
                atime: now,
                mtime: now,
                ctime: now,
                crtime: now,
                kind: fuse::FileType::RegularFile,
                perm: 0o644,
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
            },
            0,
            fh as u64,
            flags,
        );
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        // Grab the file from the file handle
        let mut state = self.state.inner.lock().unwrap();
        let file = match state.fuse_lookup.get_mut(&fh) {
            Some(a) => a,
            None => {
                tracing::trace!("fs::read lookup(fh={fh}) noent err=EIO");
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Read the data from the file and return it
        let data: Result<_, io::Error> = self.handle.block_on(async {
            let mut buf = Vec::with_capacity(size as usize);
            unsafe { buf.set_len(size as usize) };
            file.seek(io::SeekFrom::Start(offset as u64)).await?;
            let amt = file.read(&mut buf).await?;
            unsafe { buf.set_len(amt) };
            Ok(buf)
        });
        let data = match data {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::read data err=EIO");
                reply.error(libc::EIO);
                return;
            }
        };

        // Return the data
        reply.data(&data);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        // Get the path from the ino otherwise it is not a known
        // path (this means the other methods have to be hit first)
        let path = match self.reverse_ino(ino) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::readir reverse_ino({ino}) err={}", err);
                reply.error(err);
                return;
            }
        };

        let mut state = self.state.inner.lock().unwrap();
        let read_dir = state.mem_fs.read_dir(&Path::new(path.as_ref()));
        let read_dir = match read_dir {
            Ok(a) => a,
            Err(FsError::EntryNotFound) => {
                tracing::trace!("fs::readir read_dir({}) err=ENOENT", path);
                return;
            }
            Err(err) => {
                tracing::trace!("fs::readir read_dir({}) err={}", path, err);
                reply.error(libc::EIO);
                return;
            }
        };

        for (i, entry) in read_dir.into_iter().enumerate().skip(offset as usize) {
            let entry = match entry {
                Ok(a) => a,
                Err(err) => {
                    tracing::trace!("fs::readir direntry(index={i}) err={}", err);
                    reply.error(libc::EIO);
                    return;
                }
            };
            let path = entry.path.to_string_lossy();
            let name = match entry.path.file_name() {
                Some(n) => n,
                None => {
                    tracing::trace!("fs::readir file_name err=EIO");
                    reply.error(libc::EIO);
                    return;
                }
            };

            // The ino is just the hash of the name
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut hasher);
            let ino = hasher.finish();
            state
                .inos
                .entry(ino)
                .or_insert_with(|| path.into_owned().into());

            // Compute the directory kind
            let kind = match entry.file_type() {
                Ok(ft) => file_type_to_kind(ft),
                _ => fuse::FileType::RegularFile,
            };

            // i + 1 means the index of the next entry
            reply.add(ino, (i + 1) as i64, kind, name);
        }
        reply.ok();
    }

    fn mkdir(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, reply: ReplyEntry) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => return reply.error(err),
        };

        let entry = JournalEntry::CreateDirectoryV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);

        match self.attr(path) {
            Ok(meta) => reply.entry(&time::Timespec::new(1, 0), &meta, 0),
            Err(err) => reply.error(err),
        }
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => return reply.error(err),
        };

        let entry = JournalEntry::RemoveDirectoryV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);
        reply.ok();
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => return reply.error(err),
        };

        let entry = JournalEntry::UnlinkFileV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);
        reply.ok();
    }
}

fn file_type_to_kind(ft: virtual_fs::FileType) -> fuse::FileType {
    if ft.dir {
        fuse::FileType::Directory
    } else if ft.symlink {
        fuse::FileType::Symlink
    } else if ft.block_device {
        fuse::FileType::BlockDevice
    } else if ft.char_device {
        fuse::FileType::CharDevice
    } else if ft.socket {
        fuse::FileType::Socket
    } else {
        fuse::FileType::RegularFile
    }
}
