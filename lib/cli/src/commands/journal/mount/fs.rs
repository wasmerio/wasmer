#![allow(unused)]
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    fs::File,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    sync::{atomic::AtomicU32, Arc, Mutex},
    time::Duration,
};

use fuse::{
    FileAttr, Filesystem, ReplyAttr, ReplyBmap, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyLock, ReplyOpen, ReplyStatfs, ReplyWrite, ReplyXattr, Request,
};
use indicatif::{ProgressBar, ProgressStyle};
use shared_buffer::OwnedBuffer;
use tokio::runtime::Handle;
use virtual_fs::{
    mem_fs::{self, OffloadBackingStore},
    AsyncReadExt, AsyncSeekExt, AsyncWriteExt, FileOpener, FileSystem, FsError,
};
use wasmer_wasix::{
    fs::WasiFdSeed,
    journal::{
        copy_journal, ArchivedJournalEntry, ArchivedJournalEntryFileDescriptorWriteV1, Journal,
        JournalEntry, JournalEntryFileDescriptorWriteV1, LogFileJournal, LogWriteResult,
        ReadableJournal, WritableJournal,
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
    lookup: HashMap<
        u32,
        Arc<tokio::sync::Mutex<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>>>,
    >,
    seed: WasiFdSeed,
    fake_offset: u64,
}

#[derive(Debug)]
struct MutexState {
    inner: Mutex<State>,
}

#[derive(Debug)]
pub struct JournalFileSystemBuilder {
    path: PathBuf,
    fd_seed: WasiFdSeed,
    progress_bar: bool,
}

impl JournalFileSystemBuilder {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            fd_seed: WasiFdSeed::default(),
            progress_bar: false,
        }
    }

    pub fn with_fd_seed(mut self, fd_seed: WasiFdSeed) -> Self {
        self.fd_seed = fd_seed;
        self
    }

    pub fn with_progress_bar(mut self, val: bool) -> Self {
        self.progress_bar = val;
        self
    }

    // Opens the journal and copies all its contents into
    // and memory file system
    pub fn build(self) -> anyhow::Result<JournalFileSystem> {
        let journal = LogFileJournal::new(&self.path)?;
        let backing_store = journal.backing_store();
        let file_len = backing_store.owned_buffer().len();

        let mem_fs = mem_fs::FileSystem::default().with_backing_offload(backing_store)?;
        let state = MutexState {
            inner: Mutex::new(State {
                handle: tokio::runtime::Handle::current(),
                mem_fs,
                inos: Default::default(),
                seed: self.fd_seed,
                lookup: Default::default(),
                fake_offset: 0,
            }),
        };

        let progress = if self.progress_bar {
            let mut pb = ProgressBar::new(file_len as u64);
            pb.set_style(ProgressStyle::with_template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                .unwrap()
                .progress_chars("#>-"));
            pb.set_message("Loading journal...");

            Some(pb)
        } else {
            None
        };

        tokio::task::block_in_place(|| {
            if let Some(progress) = progress {
                copy_journal_with_progress(&journal, &state, progress)
            } else {
                copy_journal(&journal, &state)
            }
        })?;

        let ret = JournalFileSystem {
            handle: tokio::runtime::Handle::current(),
            journal,
            state,
        };

        Ok(ret)
    }
}

pub fn copy_journal_with_progress<R: ReadableJournal, W: WritableJournal>(
    from: &R,
    to: &W,
    mut progress: ProgressBar,
) -> anyhow::Result<()> {
    while let Some(record) = from.read()? {
        progress.set_position(record.record_end);
        to.write(record.into_inner())?;
    }
    progress.finish_and_clear();
    println!("Journal is mounted");
    Ok(())
}

#[derive(Debug)]
pub struct JournalFileSystem {
    handle: tokio::runtime::Handle,
    journal: LogFileJournal,
    state: MutexState,
}

impl JournalFileSystem {
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

    fn attr<'a>(&self, path: Cow<'a, str>) -> Result<FileAttr, libc::c_int> {
        let mut state = self.state.inner.lock().unwrap();

        let res = state.mem_fs.metadata(&Path::new(path.as_ref()));
        match res {
            Ok(meta) => {
                // The ino is just the hash of the name
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                path.hash(&mut hasher);
                let ino = hasher.finish();
                state
                    .inos
                    .entry(ino)
                    .or_insert_with(|| path.into_owned().into());

                // Build a file attr and return it
                Ok(FileAttr {
                    ino,
                    size: meta.len,
                    blocks: (1u64.max(meta.len) - 1 / 512) + 1,
                    atime: time01::Timespec::new(meta.accessed as i64, 0),
                    mtime: time01::Timespec::new(meta.modified as i64, 0),
                    ctime: time01::Timespec::new(meta.created as i64, 0),
                    crtime: time01::Timespec::new(meta.created as i64, 0),
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
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let mut state = self.inner.lock().unwrap();
        let ret = LogWriteResult {
            record_start: state.fake_offset,
            record_end: state.fake_offset + entry.estimate_size() as u64,
        };
        state.fake_offset += ret.record_size();
        match entry {
            JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => {
                let handle = state.handle.clone();
                if let Some(file) = state.lookup.get_mut(&fd) {
                    handle.block_on(async {
                        let mut file = file.lock().await;
                        file.seek(io::SeekFrom::Start(offset)).await;
                        file.write_all(&data).await
                    })?;
                }
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                state.lookup.remove(&fd);
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
                    .write(true)
                    .read(true)
                    .open(path.as_ref())?;
                state
                    .lookup
                    .insert(fd, Arc::new(tokio::sync::Mutex::new(file)));
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                state.seed.clip_val(new_fd + 1);
                if let Some(file) = state.lookup.remove(&old_fd) {
                    state.lookup.insert(new_fd, file);
                }
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                state.seed.clip_val(copied_fd + 1);
                if let Some(file) = state.lookup.get(&original_fd).cloned() {
                    state.lookup.insert(copied_fd, file);
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
                if let Some(file) = state.lookup.get(&fd) {
                    handle.block_on(async {
                        let mut file = file.lock().await;
                        file.set_len(st_size)
                    })?;
                }
            }
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                let handle = state.handle.clone();
                if let Some(file) = state.lookup.get(&fd) {
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
    fn init(&mut self, _req: &Request) -> Result<(), libc::c_int> {
        Ok(())
    }

    fn destroy(&mut self, _req: &Request) {}

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::lookup err={err}");
                return reply.error(err);
            }
        };

        match self.attr(path) {
            Ok(meta) => reply.entry(&time01::Timespec::new(1, 0), &meta, 0),
            Err(err) => {
                tracing::trace!("fs::lookup err={err}");
                reply.error(err)
            }
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
            Ok(meta) => reply.attr(&time01::Timespec::new(1, 0), &meta),
            Err(err) => reply.error(err),
        }
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<time01::Timespec>,
        _mtime: Option<time01::Timespec>,
        fh: Option<u64>,
        _crtime: Option<time01::Timespec>,
        _chgtime: Option<time01::Timespec>,
        _bkuptime: Option<time01::Timespec>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let mut entries = Vec::new();

        let attr = match fh {
            Some(fd) => {
                let fd = fd as u32;
                let mut state = self.state.inner.lock().unwrap();
                let file = match state.lookup.get_mut(&fd) {
                    Some(f) => f.clone(),
                    None => {
                        tracing::trace!("fs::getattr noent (fd={fd})");
                        reply.error(libc::ENOENT);
                        return;
                    }
                };

                self.handle.block_on(async {
                    let mut file = file.lock().await;

                    if let Some(size) = size {
                        entries.push(JournalEntry::FileDescriptorSetSizeV1 {
                            fd: fd as u32,
                            st_size: size,
                        })
                    }

                    FileAttr {
                        ino,
                        size: file.size(),
                        blocks: (1u64.max(file.size()) - 1 / 512) + 1,
                        atime: time01::Timespec::new(file.last_accessed() as i64, 0),
                        mtime: time01::Timespec::new(file.last_modified() as i64, 0),
                        ctime: time01::Timespec::new(file.created_time() as i64, 0),
                        crtime: time01::Timespec::new(file.created_time() as i64, 0),
                        kind: fuse::FileType::RegularFile,
                        perm: 0o644,
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        rdev: 0,
                        flags: 0,
                    }
                })
            }
            None => {
                let path = match self.reverse_ino(ino) {
                    Ok(a) => a,
                    Err(err) => {
                        tracing::trace!("fs::setattr reverse_ino({ino}) errno={err}");
                        reply.error(err);
                        return;
                    }
                };

                let fh;
                let mut state = self.state.inner.lock().unwrap();
                let file = state
                    .mem_fs
                    .new_open_options()
                    .read(true)
                    .write(true)
                    .open(&Path::new(path.as_ref()));
                match file {
                    Ok(file) => {
                        // Reserve a file descriptor and close the state
                        fh = state.seed.next_val();
                        drop(state);

                        entries.push(JournalEntry::OpenFileDescriptorV1 {
                            fd: fh,
                            dirfd: VIRTUAL_ROOT_FD,
                            dirflags: 0,
                            path,
                            o_flags: wasi::Oflags::empty(),
                            fs_rights_base: wasi::Rights::all(),
                            fs_rights_inheriting: wasi::Rights::all(),
                            fs_flags: wasi::Fdflags::empty(),
                        });
                        if let Some(size) = size {
                            entries.push(JournalEntry::FileDescriptorSetSizeV1 {
                                fd: fh as u32,
                                st_size: size,
                            })
                        }
                        entries.push(JournalEntry::CloseFileDescriptorV1 { fd: fh });

                        for entry in entries.iter() {
                            if self.state.write(entry.clone()).is_err() {
                                tracing::trace!("fs::open err=EIO");
                                reply.error(libc::EIO);
                                return;
                            }
                        }
                        for entry in entries.iter() {
                            if self.journal.write(entry.clone()).is_err() {
                                tracing::trace!("fs::open err=EIO");
                                reply.error(libc::EIO);
                                return;
                            }
                        }
                        FileAttr {
                            ino,
                            size: file.size(),
                            blocks: (1u64.max(file.size()) - 1 / 512) + 1,
                            atime: time01::Timespec::new(file.last_accessed() as i64, 0),
                            mtime: time01::Timespec::new(file.last_modified() as i64, 0),
                            ctime: time01::Timespec::new(file.created_time() as i64, 0),
                            crtime: time01::Timespec::new(file.created_time() as i64, 0),
                            kind: fuse::FileType::RegularFile,
                            perm: 0o644,
                            nlink: 1,
                            uid: 0,
                            gid: 0,
                            rdev: 0,
                            flags: 0,
                        }
                    }
                    Err(FsError::EntryNotFound) => {
                        // Maybe its a directory, in which case we are done
                        if let Ok(meta) = state.mem_fs.metadata(&Path::new(path.as_ref())) {
                            FileAttr {
                                ino,
                                size: meta.len,
                                blocks: (1u64.max(meta.len) - 1 / 512) + 1,
                                atime: time01::Timespec::new(meta.accessed as i64, 0),
                                mtime: time01::Timespec::new(meta.modified as i64, 0),
                                ctime: time01::Timespec::new(meta.created as i64, 0),
                                crtime: time01::Timespec::new(meta.created as i64, 0),
                                kind: file_type_to_kind(meta.ft),
                                perm: 0o644,
                                nlink: 1,
                                uid: 0,
                                gid: 0,
                                rdev: 0,
                                flags: 0,
                            }
                        } else {
                            tracing::trace!("fs::setattr open_file({path}) err=ENOENT");
                            reply.error(libc::ENOENT);
                            return;
                        }
                    }
                    Err(err) => {
                        tracing::trace!("fs::setattr open_file({path}) err={err}");
                        reply.error(libc::EIO);
                        return;
                    }
                }
            }
        };

        // Return the data
        reply.attr(&time01::Timespec::new(1, 0), &attr)
    }

    fn setxattr(
        &mut self,
        _req: &Request,
        _ino: u64,
        _name: &OsStr,
        _value: &[u8],
        _flags: u32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        tracing::trace!("fs::setxattr err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn getxattr(
        &mut self,
        _req: &Request,
        _ino: u64,
        _name: &OsStr,
        _size: u32,
        reply: ReplyXattr,
    ) {
        tracing::trace!("fs::getxattr size(0)");
        reply.size(0)
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

        // Reserve a file descriptor
        let fh = {
            let mut state = self.state.inner.lock().unwrap();
            state.seed.next_val()
        };

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
        if self.state.write(entry.clone()).is_err() {
            tracing::trace!("fs::open err=EIO");
            reply.error(libc::EIO);
            return;
        }
        if self.journal.write(entry).is_err() {
            tracing::trace!("fs::open err=EIO");
            reply.error(libc::EIO);
            return;
        }

        tracing::trace!("fs::open opened fh={fh}");
        reply.opened(fh as u64, flags);
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let fh = fh as u32;

        {
            // Check that the file handle exists
            let mut state = self.state.inner.lock().unwrap();
            if !state.lookup.contains_key(&fh) {
                tracing::trace!("fs::release err=ENOENT (fd={fh})");
                reply.error(libc::ENOENT);
                return;
            }
        }

        // Write the journals
        let entry = JournalEntry::CloseFileDescriptorV1 { fd: fh };
        if self.state.write(entry.clone()).is_err() {
            tracing::trace!("fs::release err=EIO");
            reply.error(libc::EIO);
            return;
        }
        if self.journal.write(entry).is_err() {
            tracing::trace!("fs::release err=EIO");
            reply.error(libc::EIO);
            return;
        }

        tracing::trace!("fs::release ok");
        reply.ok();
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

        // Reserve a file descriptor
        let fh = {
            let mut state = self.state.inner.lock().unwrap();
            state.seed.next_val()
        };

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
        if let Err(err) = self.state.write(entry.clone()) {
            tracing::trace!("fs::create (j1) err=EIO - {err}");
            reply.error(libc::EIO);
            return;
        }
        if let Err(err) = self.journal.write(entry) {
            tracing::trace!("fs::create (j2) err=EIO - {err}");
            reply.error(libc::EIO);
            return;
        }

        let now = time01::get_time();
        reply.created(
            &time01::Timespec::new(1, 0),
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
        let fh = fh as u32;

        // Grab the file from the file handle
        let mut state = self.state.inner.lock().unwrap();
        let file = match state.lookup.get_mut(&fh) {
            Some(a) => a,
            None => {
                tracing::trace!("fs::read lookup(fh={fh}) noent err=EIO");
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Read the data from the file and return it
        let data: Result<_, io::Error> = self.handle.block_on(async {
            let mut file = file.lock().await;

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

    fn write(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _flags: u32,
        reply: ReplyWrite,
    ) {
        let fh = fh as u32;

        {
            // Check that the file handle exists
            let mut state = self.state.inner.lock().unwrap();
            if !state.lookup.contains_key(&fh) {
                tracing::trace!("fs::write err=ENOENT");
                reply.error(libc::ENOENT);
                return;
            }
        }

        // Write the entry to the log file
        let fd = fh as u32;
        let entry = JournalEntry::FileDescriptorWriteV1 {
            fd,
            offset: offset as u64,
            data: data.into(),
            is_64bit: false,
        };

        let res = match self.journal.write(entry) {
            Ok(res) => res,
            Err(err) => {
                tracing::trace!("fs::write err=EIO - {err}");
                reply.error(libc::EIO);
                return;
            }
        };

        // We load the record from the journal and use this to write to the memory file system
        // because the memory file system has an optimization where it will automatically offload
        // the data to the mmap of the journal rather than store it in memory. In effect it offloads
        // to the disk
        {
            let mut state = self.state.inner.lock().unwrap();
            let handle = state.handle.clone();
            if let Some(file) = state.lookup.get_mut(&fd) {
                let res: Result<_, io::Error> = handle.block_on(async {
                    let mut file = file.lock().await;
                    file.seek(io::SeekFrom::Start(offset as u64)).await;

                    // Unsafe!!! This assumes the structure does not change
                    // where the first bytes in the entry are an aligned
                    // array that corresponds to the data itself
                    let size = data.len() as u64;
                    let mut mmap_offset = res.record_start;
                    let align = mmap_offset % 16;
                    if align != 0 {
                        mmap_offset += 16 - align;
                    }

                    // Add the entry
                    if file.write_from_mmap(mmap_offset, size).is_err() {
                        // We fall back on just writing the data normally
                        file.seek(io::SeekFrom::Start(offset as u64)).await;
                        file.write_all(&data).await?;
                    }
                    Ok(())
                });
                if let Err(err) = res {
                    tracing::trace!("fs::write err=EIO");
                    reply.error(libc::EIO);
                    return;
                }
            } else {
                tracing::trace!("fs::write err=EIO");
                reply.error(libc::EIO);
                return;
            }
        }

        reply.written(data.len() as u32);
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
            Err(err) => {
                tracing::trace!("fs::mkdir compute_path err={err}");
                return reply.error(err);
            }
        };

        let entry = JournalEntry::CreateDirectoryV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);

        match self.attr(path) {
            Ok(meta) => {
                tracing::trace!("fs::mkdir ok");
                reply.entry(&time01::Timespec::new(1, 0), &meta, 0)
            }
            Err(err) => {
                tracing::trace!("fs::mkdir attr err={err}");
                reply.error(err)
            }
        }
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::rmdir err={err}");
                return reply.error(err);
            }
        };

        let entry = JournalEntry::RemoveDirectoryV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);
        tracing::trace!("fs::rmdir ok");
        reply.ok();
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let path = match self.compute_path(parent, name) {
            Ok(a) => a,
            Err(err) => {
                tracing::trace!("fs::unlink err={err}");
                return reply.error(err);
            }
        };

        let entry = JournalEntry::UnlinkFileV1 {
            fd: VIRTUAL_ROOT_FD,
            path: path.clone(),
        };
        self.state.write(entry.clone());
        self.journal.write(entry);
        tracing::trace!("fs::unlink ok");
        reply.ok();
    }

    fn forget(&mut self, _req: &Request, _ino: u64, _nlookup: u64) {
        tracing::trace!("fs::forget ok");
    }

    fn readlink(&mut self, _req: &Request, _ino: u64, reply: ReplyData) {
        tracing::trace!("fs::readlink err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn mknod(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _mode: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        tracing::trace!("fs::mknod err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn symlink(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _link: &Path,
        reply: ReplyEntry,
    ) {
        tracing::trace!("fs::symlink err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn rename(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _newparent: u64,
        _newname: &OsStr,
        reply: ReplyEmpty,
    ) {
        tracing::trace!("fs::rename err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn link(
        &mut self,
        _req: &Request,
        _ino: u64,
        _newparent: u64,
        _newname: &OsStr,
        reply: ReplyEntry,
    ) {
        tracing::trace!("fs::link err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn fsync(&mut self, _req: &Request, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty) {
        tracing::trace!("fs::fsync err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn opendir(&mut self, _req: &Request, _ino: u64, _flags: u32, reply: ReplyOpen) {
        tracing::trace!("fs::opendir opened");
        reply.opened(0, 0);
    }

    fn releasedir(&mut self, _req: &Request, _ino: u64, _fh: u64, _flags: u32, reply: ReplyEmpty) {
        tracing::trace!("fs::releasedir ok");
        reply.ok();
    }

    fn fsyncdir(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        tracing::trace!("fs::fsyncdir err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        tracing::trace!("fs::statfs ok");
        reply.statfs(0, 0, 0, 0, 0, 512, 255, 0);
    }

    fn listxattr(&mut self, _req: &Request, _ino: u64, _size: u32, reply: ReplyXattr) {
        tracing::trace!("fs::listxattr err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn removexattr(&mut self, _req: &Request, _ino: u64, _name: &OsStr, reply: ReplyEmpty) {
        tracing::trace!("fs::removexattr err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn access(&mut self, _req: &Request, _ino: u64, _mask: u32, reply: ReplyEmpty) {
        tracing::trace!("fs::access err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn getlk(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        _start: u64,
        _end: u64,
        _typ: u32,
        _pid: u32,
        reply: ReplyLock,
    ) {
        tracing::trace!("fs::getlk err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn setlk(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        _start: u64,
        _end: u64,
        _typ: u32,
        _pid: u32,
        _sleep: bool,
        reply: ReplyEmpty,
    ) {
        tracing::trace!("fs::setlk err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    fn bmap(&mut self, _req: &Request, _ino: u64, _blocksize: u32, _idx: u64, reply: ReplyBmap) {
        tracing::trace!("fs::bmp err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    #[cfg(target_os = "macos")]
    fn setvolname(&mut self, _req: &Request, _name: &OsStr, reply: ReplyEmpty) {
        tracing::trace!("fs::setvolname err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    #[cfg(target_os = "macos")]
    fn exchange(
        &mut self,
        _req: &Request,
        _parent: u64,
        _name: &OsStr,
        _newparent: u64,
        _newname: &OsStr,
        _options: u64,
        reply: ReplyEmpty,
    ) {
        tracing::trace!("fs::exchange err=ENOSYS");
        reply.error(libc::ENOSYS);
    }

    #[cfg(target_os = "macos")]
    fn getxtimes(&mut self, _req: &Request, _ino: u64, reply: ReplyXTimes) {
        tracing::trace!("fs::getxtimes err=ENOSYS");
        reply.error(libc::ENOSYS);
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
