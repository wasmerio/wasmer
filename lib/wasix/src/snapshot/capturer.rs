use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::SystemTime;
use std::{borrow::Cow, ops::Range};
use wasmer_wasix_types::wasi::{
    Advice, EpollCtl, EpollEventCtl, ExitCode, Fdflags, FileDelta, Filesize, Fstflags, LookupFlags,
    Oflags, Rights, Snapshot0Clockid, Timestamp, Tty, Whence,
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

/// Represents a log entry in a snapshot log stream that represents the total
/// state of a WASM process at a point in time.
#[derive(Debug)]
pub enum SnapshotLog<'a> {
    Init {
        wasm_hash: [u8; 32],
    },
    FileDescriptorSeek {
        fd: Fd,
        offset: FileDelta,
        whence: Whence,
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
        dirfd: Fd,
        dirflags: LookupFlags,
        path: Cow<'a, str>,
        o_flags: Oflags,
        fs_rights_base: Rights,
        fs_rights_inheriting: Rights,
        fs_flags: Fdflags,
        is_64bit: bool,
    },
    RenumberFileDescriptor {
        old_fd: Fd,
        new_fd: Fd,
    },
    DuplicateFileDescriptor {
        original_fd: Fd,
        copied_fd: Fd,
    },
    CreateDirectory {
        fd: Fd,
        path: Cow<'a, str>,
    },
    RemoveDirectory {
        fd: Fd,
        path: Cow<'a, str>,
    },
    PathSetTimes {
        fd: Fd,
        flags: LookupFlags,
        path: Cow<'a, str>,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    },
    FileDescriptorSetTimes {
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    },
    FileDescriptorSetFlags {
        fd: Fd,
        flags: Fdflags,
    },
    FileDescriptorSetRights {
        fd: Fd,
        fs_rights_base: Rights,
        fs_rights_inheriting: Rights,
    },
    FileDescriptorSetSize {
        fd: Fd,
        st_size: Filesize,
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
        old_path: Cow<'a, str>,
        fd: Fd,
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
        event: Option<EpollEventCtl>,
    },
    TtySet {
        tty: Tty,
        line_feeds: bool,
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
