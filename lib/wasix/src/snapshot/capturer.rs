use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::time::SystemTime;
use std::{borrow::Cow, ops::Range};
use wasmer_wasix_types::wasi::ExitCode;

use futures::future::BoxFuture;
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
pub enum FdSnapshot<'a> {
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

impl<'a> FdSnapshot<'a> {
    pub fn into_owned(self) -> FdSnapshot<'static> {
        match self {
            FdSnapshot::Stdin { non_blocking } => FdSnapshot::Stdin { non_blocking },
            FdSnapshot::Stdout { non_blocking } => FdSnapshot::Stdout { non_blocking },
            FdSnapshot::Stderr { non_blocking } => FdSnapshot::Stderr { non_blocking },
            FdSnapshot::OpenFile {
                path,
                offset,
                read,
                write,
                non_blocking,
            } => FdSnapshot::OpenFile {
                path: Cow::Owned(path.into_owned()),
                offset,
                read,
                write,
                non_blocking,
            },
            FdSnapshot::Socket {
                state,
                non_blocking,
            } => FdSnapshot::Socket {
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
    TerminalData {
        data: Cow<'a, [u8]>,
    },
    UpdateMemoryRegion {
        region: Range<u64>,
        data: Cow<'a, [u8]>,
    },
    CloseThread {
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    },
    SetThread {
        id: WasiThreadId,
        call_stack: Cow<'a, [u8]>,
        memory_stack: Cow<'a, [u8]>,
    },
    CloseFileDescriptor {
        fd: Fd,
    },
    OpenFileDescriptor {
        fd: Fd,
        state: FdSnapshot<'a>,
    },
    RemoveFileSystemEntry {
        path: Cow<'a, str>,
    },
    UpdateFileSystemEntry {
        path: Cow<'a, str>,
        ft: FileEntryType,
        accessed: u64,
        created: u64,
        modified: u64,
        len: u64,
        data: Cow<'a, [u8]>,
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
            Self::TerminalData { data } => f
                .debug_struct("TerminalData")
                .field("data.len", &data.len())
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
            } => f
                .debug_struct("SetThread")
                .field("id", id)
                .field("call_stack.len", &call_stack.len())
                .field("memory_stack.len", &memory_stack.len())
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
            Self::RemoveFileSystemEntry { path } => f
                .debug_struct("RemoveFileSystemEntry")
                .field("path", path)
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
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Returns a stream of snapshot objects that the runtime will use
    /// to restore the state of a WASM process to a previous moment in time
    fn read<'a>(&'a self) -> BoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>>;
}

pub type DynSnapshotCapturer = dyn SnapshotCapturer + Send + Sync;
