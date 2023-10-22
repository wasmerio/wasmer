use serde::{Deserialize, Serialize};
use std::{borrow::Cow, ops::Range};

use futures::future::BoxFuture;
use virtual_fs::Fd;

use crate::WasiThreadId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FdSnapshot {
    Stdin,
    Stdout,
    OpenFile,
    Socket,
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
    TerminalData {
        data: Cow<'a, [u8]>,
    },
    UpdateMemoryRegion {
        region: Range<u64>,
        data: Cow<'a, [u8]>,
    },
    CloseThread {
        id: WasiThreadId,
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
        state: FdSnapshot,
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
    Snapshot,
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
