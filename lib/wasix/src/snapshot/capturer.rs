use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::time::SystemTime;
use std::{borrow::Cow, ops::Range};
use wasmer_wasix_types::wasi::{
    Advice, EpollCtl, ExitCode, Filesize, Fstflags, LookupFlags, Snapshot0Clockid, Timestamp, Tty,
};

use futures::future::LocalBoxFuture;
use virtual_fs::Fd;

use crate::WasiThreadId;

use super::SnapshotTrigger;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SocketSnapshot {
    TcpListen {
        listen_addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    },
    TcpStream {
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    },
    UdpSocket {
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        reuse_port: bool,
        reuse_addr: bool,
    },
    Icmp {
        addr: SocketAddr,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FdOpenSnapshot<'a> {
    Stdin {
        non_blocking: bool,
    },
    Stdout {
        non_blocking: bool,
    },
    Stderr {
        non_blocking: bool,
    },
    OpenFile {
        path: Cow<'a, str>,
        offset: u64,
        read: bool,
        write: bool,
        non_blocking: bool,
    },
    Socket {
        state: SocketSnapshot,
        non_blocking: bool,
    },
}

impl<'a> FdOpenSnapshot<'a> {
    pub fn into_owned(self) -> FdOpenSnapshot<'static> {
        match self {
            FdOpenSnapshot::Stdin { non_blocking } => FdOpenSnapshot::Stdin { non_blocking },
            FdOpenSnapshot::Stdout { non_blocking } => FdOpenSnapshot::Stdout { non_blocking },
            FdOpenSnapshot::Stderr { non_blocking } => FdOpenSnapshot::Stderr { non_blocking },
            FdOpenSnapshot::OpenFile {
                path,
                offset,
                read,
                write,
                non_blocking,
            } => FdOpenSnapshot::OpenFile {
                path: Cow::Owned(path.into_owned()),
                offset,
                read,
                write,
                non_blocking,
            },
            FdOpenSnapshot::Socket {
                state,
                non_blocking,
            } => FdOpenSnapshot::Socket {
                state,
                non_blocking,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileEntryType {
    Directory,
    File,
    Symlink,
    CharDevice,
    BlockDevice,
    Socket,
    Fifo,
}

/// Represents a log entry in a snapshot log stream that represents the total
/// state of a WASM process at a point in time.
pub enum SnapshotLog<'a> {
    Init {
        wasm_hash: [u8; 32],
    },
    FileDescriptorWrite {
        fd: Fd,
        offset: u64,
        data: Cow<'a, [u8]>,
        is_64bit: bool,
    },
    UpdateMemoryRegion {
        region: Range<u64>,
        data: Cow<'a, [u8]>,
    },
    SetClockTime {
        clock_id: Snapshot0Clockid,
        time: Timestamp,
    },
    CloseThread {
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    },
    SetThread {
        id: WasiThreadId,
        call_stack: Cow<'a, [u8]>,
        memory_stack: Cow<'a, [u8]>,
        store_data: Cow<'a, [u8]>,
        is_64bit: bool,
    },
    CloseFileDescriptor {
        fd: Fd,
    },
    OpenFileDescriptor {
        fd: Fd,
        state: FdOpenSnapshot<'a>,
    },
    RenumberFileDescriptor {
        old_fd: Fd,
        new_fd: Fd,
    },
    DuplicateFileDescriptor {
        old_fd: Fd,
        new_fd: Fd,
    },
    CreateDirectory {
        fd: Fd,
        path: Cow<'a, str>,
    },
    RemoveDirectory {
        fd: Fd,
        path: Cow<'a, str>,
    },
    FileDescriptorSetTimes {
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    },
    FileDescriptorSetSize {
        fd: Fd,
        size: Filesize,
    },
    FileDescriptorAdvise {
        fd: Fd,
        offset: Filesize,
        len: Filesize,
        advice: Advice,
    },
    FileDescriptorAllocate {
        fd: Fd,
        offset: Filesize,
        len: Filesize,
    },
    CreateHardLink {
        old_fd: Fd,
        old_path: Cow<'a, str>,
        old_flags: LookupFlags,
        new_fd: Fd,
        new_path: Cow<'a, str>,
    },
    CreateSymbolicLink {
        old_fd: Fd,
        old_path: Cow<'a, str>,
        new_fd: Fd,
        new_path: Cow<'a, str>,
    },
    UnlinkFile {
        fd: Fd,
        path: Cow<'a, str>,
    },
    PathRename {
        old_fd: Fd,
        old_path: Cow<'a, str>,
        new_fd: Fd,
        new_path: Cow<'a, str>,
    },
    ChangeDirectory {
        path: Cow<'a, str>,
    },
    EpollCreate {
        fd: Fd,
    },
    EpollCtl {
        epfd: Fd,
        op: EpollCtl,
        fd: Fd,
    },
    TtySet {
        tty: Tty,
    },
    CreatePipe {
        fd1: Fd,
        fd2: Fd,
    },
    /// Represents the marker for the end of a snapshot
    Snapshot {
        when: SystemTime,
        trigger: SnapshotTrigger,
    },
}

impl fmt::Debug for SnapshotLog<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Init { wasm_hash } => f
                .debug_struct("Init")
                .field("wasm_hash.len", &wasm_hash.len())
                .finish(),
            Self::FileDescriptorWrite {
                fd,
                offset,
                data,
                is_64bit,
            } => f
                .debug_struct("TerminalData")
                .field("fd", &fd)
                .field("offset", &offset)
                .field("data.len", &data.len())
                .field("is_64bit", &is_64bit)
                .finish(),
            Self::UpdateMemoryRegion { region, data } => f
                .debug_struct("UpdateMemoryRegion")
                .field("region", region)
                .field("data.len", &data.len())
                .finish(),
            Self::CloseThread { id, exit_code } => f
                .debug_struct("CloseThread")
                .field("id", id)
                .field("exit_code", exit_code)
                .finish(),
            Self::SetThread {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => f
                .debug_struct("SetThread")
                .field("id", id)
                .field("call_stack.len", &call_stack.len())
                .field("memory_stack.len", &memory_stack.len())
                .field("store_data.len", &store_data.len())
                .field("is_64bit", is_64bit)
                .finish(),
            Self::CloseFileDescriptor { fd } => f
                .debug_struct("CloseFileDescriptor")
                .field("fd", fd)
                .finish(),
            Self::OpenFileDescriptor { fd, state } => f
                .debug_struct("OpenFileDescriptor")
                .field("fd", fd)
                .field("state", state)
                .finish(),
            Self::RemoveDirectory { fd, path } => f
                .debug_struct("RemoveDirectory")
                .field("fd", fd)
                .field("path", path)
                .finish(),
            Self::UnlinkFile { fd, path } => f
                .debug_struct("UnlinkFile")
                .field("fd", fd)
                .field("path", path)
                .finish(),
            Self::PathRename {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => f
                .debug_struct("UnlinkFile")
                .field("old_fd", old_fd)
                .field("old_path", old_path)
                .field("new_fd", new_fd)
                .field("new_path", new_path)
                .finish(),
            Self::UpdateFileSystemEntry {
                path,
                ft,
                accessed,
                created,
                modified,
                len,
                data,
            } => f
                .debug_struct("UpdateFileSystemEntry")
                .field("path", path)
                .field("ft", ft)
                .field("accessed", accessed)
                .field("created", created)
                .field("modified", modified)
                .field("len", len)
                .field("data.len", &data.len())
                .finish(),
            Self::Snapshot { when, trigger } => f
                .debug_struct("Snapshot")
                .field("when", when)
                .field("trigger", trigger)
                .finish(),
        }
    }
}

/// The snapshot capturer will take a series of objects that represents the state of
/// a WASM process at a point in time and saves it so that it can be restored.
/// It also allows for the restoration of that state at a later moment
#[allow(unused_variables)]
pub trait SnapshotCapturer {
    /// Takes in a stream of snapshot log entries and saves them so that they
    /// may be restored at a later moment
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>>;

    /// Returns a stream of snapshot objects that the runtime will use
    /// to restore the state of a WASM process to a previous moment in time
    fn read<'a>(&'a self) -> LocalBoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>>;
}

pub type DynSnapshotCapturer = dyn SnapshotCapturer + Send + Sync;
