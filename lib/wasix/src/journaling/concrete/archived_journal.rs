use rkyv::{Archive, CheckBytes, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde;
use std::borrow::Cow;
use std::{net::Shutdown, time::SystemTime};
use virtual_net::{Duration, IpAddr, IpCidr, Ipv4Addr, Ipv6Addr, SocketAddr, StreamSecurity};
use wasmer_wasix_types::wasi::{self, EpollEventCtl, EpollType, Fdflags, Rights, Sockoption};

use crate::net::socket::TimeType;

use super::*;

pub const JOURNAL_MAGIC_NUMBER: u64 = 0x310d6dd027362979;

/// Version of the archived journal
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalControlBatchType {
    Abort,
    Commit,
}

/// Represents a batch of journal log entries
#[allow(clippy::large_enum_variant)]
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) struct JournalBatch {
    pub records: Vec<JournalBatchEntry>,
}

/// The journal log entries are serializable which
/// allows them to be written directly to a file
///
/// Note: This structure is versioned which allows for
/// changes to the journal entry types without having to
/// worry about backward and forward compatibility
#[allow(clippy::large_enum_variant)]
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalBatchEntry {
    InitModuleV1 {
        wasm_hash: [u8; 32],
    },
    ProcessExitV1 {
        exit_code: Option<JournalExitCodeV1>,
    },
    SetThreadV1 {
        id: u32,
        call_stack: Vec<u8>,
        memory_stack: Vec<u8>,
        store_data: Vec<u8>,
        is_64bit: bool,
    },
    CloseThreadV1 {
        id: u32,
        exit_code: Option<JournalExitCodeV1>,
    },
    FileDescriptorSeekV1 {
        fd: u32,
        offset: i64,
        whence: JournalWhenceV1,
    },
    FileDescriptorWriteV1 {
        fd: u32,
        offset: u64,
        data: Vec<u8>,
        is_64bit: bool,
    },
    UpdateMemoryRegionV1 {
        start: u64,
        end: u64,
        data: Vec<u8>,
    },
    SetClockTimeV1 {
        clock_id: JournalSnapshot0ClockidV1,
        time: u64,
    },
    OpenFileDescriptorV1 {
        fd: u32,
        dirfd: u32,
        dirflags: u32,
        path: String,
        o_flags: u16,
        fs_rights_base: u64,
        fs_rights_inheriting: u64,
        fs_flags: u16,
    },
    CloseFileDescriptorV1 {
        fd: u32,
    },
    RenumberFileDescriptorV1 {
        old_fd: u32,
        new_fd: u32,
    },
    DuplicateFileDescriptorV1 {
        original_fd: u32,
        copied_fd: u32,
    },
    CreateDirectoryV1 {
        fd: u32,
        path: String,
    },
    RemoveDirectoryV1 {
        fd: u32,
        path: String,
    },
    PathSetTimesV1 {
        fd: u32,
        flags: u32,
        path: String,
        st_atim: u64,
        st_mtim: u64,
        fst_flags: u16,
    },
    FileDescriptorSetTimesV1 {
        fd: u32,
        st_atim: u64,
        st_mtim: u64,
        fst_flags: u16,
    },
    FileDescriptorSetSizeV1 {
        fd: u32,
        st_size: u64,
    },
    FileDescriptorSetFlagsV1 {
        fd: u32,
        flags: u16,
    },
    FileDescriptorSetRightsV1 {
        fd: u32,
        fs_rights_base: u64,
        fs_rights_inheriting: u64,
    },
    FileDescriptorAdviseV1 {
        fd: u32,
        offset: u64,
        len: u64,
        advice: JournalAdviceV1,
    },
    FileDescriptorAllocateV1 {
        fd: u32,
        offset: u64,
        len: u64,
    },
    CreateHardLinkV1 {
        old_fd: u32,
        old_path: String,
        old_flags: u32,
        new_fd: u32,
        new_path: String,
    },
    CreateSymbolicLinkV1 {
        old_path: String,
        fd: u32,
        new_path: String,
    },
    UnlinkFileV1 {
        fd: u32,
        path: String,
    },
    PathRenameV1 {
        old_fd: u32,
        old_path: String,
        new_fd: u32,
        new_path: String,
    },
    ChangeDirectoryV1 {
        path: String,
    },
    EpollCreateV1 {
        fd: u32,
    },
    EpollCtlV1 {
        epfd: u32,
        op: JournalEpollCtlV1,
        fd: u32,
        event: Option<JournalEpollEventCtlV1>,
    },
    TtySetV1 {
        cols: u32,
        rows: u32,
        width: u32,
        height: u32,
        stdin_tty: bool,
        stdout_tty: bool,
        stderr_tty: bool,
        echo: bool,
        line_buffered: bool,
        line_feeds: bool,
    },
    CreatePipeV1 {
        fd1: u32,
        fd2: u32,
    },
    CreateEventV1 {
        initial_val: u64,
        flags: u16,
        fd: u32,
    },
    PortAddAddrV1 {
        cidr: IpCidr,
    },
    PortDelAddrV1 {
        addr: IpAddr,
    },
    PortAddrClearV1,
    PortBridgeV1 {
        network: String,
        token: String,
        security: JournalStreamSecurityV1,
    },
    PortUnbridgeV1,
    PortDhcpAcquireV1,
    PortGatewaySetV1 {
        ip: IpAddr,
    },
    PortRouteAddV1 {
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    },
    PortRouteClearV1,
    PortRouteDelV1 {
        ip: IpAddr,
    },
    SocketOpenV1 {
        af: JournalAddressfamilyV1,
        ty: JournalSocktypeV1,
        pt: u16,
        fd: u32,
    },
    SocketListenV1 {
        fd: u32,
        backlog: u32,
    },
    SocketBindV1 {
        fd: u32,
        addr: SocketAddr,
    },
    SocketConnectedV1 {
        fd: u32,
        addr: SocketAddr,
    },
    SocketAcceptedV1 {
        listen_fd: u32,
        fd: u32,
        peer_addr: SocketAddr,
        fd_flags: u16,
        nonblocking: bool,
    },
    SocketJoinIpv4MulticastV1 {
        fd: u32,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketJoinIpv6MulticastV1 {
        fd: u32,
        multiaddr: Ipv6Addr,
        iface: u32,
    },
    SocketLeaveIpv4MulticastV1 {
        fd: u32,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketLeaveIpv6MulticastV1 {
        fd: u32,
        multiaddr: Ipv6Addr,
        iface: u32,
    },
    SocketSendFileV1 {
        socket_fd: u32,
        file_fd: u32,
        offset: u64,
        count: u64,
    },
    SocketSendToV1 {
        fd: u32,
        data: Vec<u8>,
        flags: u16,
        addr: SocketAddr,
        is_64bit: bool,
    },
    SocketSendV1 {
        fd: u32,
        data: Vec<u8>,
        flags: u16,
        is_64bit: bool,
    },
    SocketSetOptFlagV1 {
        fd: u32,
        opt: JournalSockoptionV1,
        flag: bool,
    },
    SocketSetOptSizeV1 {
        fd: u32,
        opt: JournalSockoptionV1,
        size: u64,
    },
    SocketSetOptTimeV1 {
        fd: u32,
        ty: JournalTimeTypeV1,
        time: Option<Duration>,
    },
    SocketShutdownV1 {
        fd: u32,
        how: JournalSocketShutdownV1,
    },
    SnapshotV1 {
        since_epoch: Duration,
        trigger: JournalSnapshotTriggerV1,
    },
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalSnapshot0ClockidV1 {
    Realtime,
    Monotonic,
    ProcessCputimeId,
    ThreadCputimeId,
    Unknown = 255,
}

impl From<wasi::Snapshot0Clockid> for JournalSnapshot0ClockidV1 {
    fn from(val: wasi::Snapshot0Clockid) -> Self {
        match val {
            wasi::Snapshot0Clockid::Realtime => JournalSnapshot0ClockidV1::Realtime,
            wasi::Snapshot0Clockid::Monotonic => JournalSnapshot0ClockidV1::Monotonic,
            wasi::Snapshot0Clockid::ProcessCputimeId => JournalSnapshot0ClockidV1::ProcessCputimeId,
            wasi::Snapshot0Clockid::ThreadCputimeId => JournalSnapshot0ClockidV1::ThreadCputimeId,
            wasi::Snapshot0Clockid::Unknown => JournalSnapshot0ClockidV1::Unknown,
        }
    }
}

impl From<JournalSnapshot0ClockidV1> for wasi::Snapshot0Clockid {
    fn from(val: JournalSnapshot0ClockidV1) -> Self {
        match val {
            JournalSnapshot0ClockidV1::Realtime => wasi::Snapshot0Clockid::Realtime,
            JournalSnapshot0ClockidV1::Monotonic => wasi::Snapshot0Clockid::Monotonic,
            JournalSnapshot0ClockidV1::ProcessCputimeId => wasi::Snapshot0Clockid::ProcessCputimeId,
            JournalSnapshot0ClockidV1::ThreadCputimeId => wasi::Snapshot0Clockid::ThreadCputimeId,
            JournalSnapshot0ClockidV1::Unknown => wasi::Snapshot0Clockid::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalSnapshot0ClockidV1> for wasi::Snapshot0Clockid {
    fn from(val: &'_ ArchivedJournalSnapshot0ClockidV1) -> Self {
        match val {
            ArchivedJournalSnapshot0ClockidV1::Realtime => wasi::Snapshot0Clockid::Realtime,
            ArchivedJournalSnapshot0ClockidV1::Monotonic => wasi::Snapshot0Clockid::Monotonic,
            ArchivedJournalSnapshot0ClockidV1::ProcessCputimeId => {
                wasi::Snapshot0Clockid::ProcessCputimeId
            }
            ArchivedJournalSnapshot0ClockidV1::ThreadCputimeId => {
                wasi::Snapshot0Clockid::ThreadCputimeId
            }
            ArchivedJournalSnapshot0ClockidV1::Unknown => wasi::Snapshot0Clockid::Unknown,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalWhenceV1 {
    Set,
    Cur,
    End,
    Unknown = 255,
}

impl From<wasi::Whence> for JournalWhenceV1 {
    fn from(val: wasi::Whence) -> Self {
        match val {
            wasi::Whence::Set => JournalWhenceV1::Set,
            wasi::Whence::Cur => JournalWhenceV1::Cur,
            wasi::Whence::End => JournalWhenceV1::End,
            wasi::Whence::Unknown => JournalWhenceV1::Unknown,
        }
    }
}

impl From<JournalWhenceV1> for wasi::Whence {
    fn from(val: JournalWhenceV1) -> Self {
        match val {
            JournalWhenceV1::Set => wasi::Whence::Set,
            JournalWhenceV1::Cur => wasi::Whence::Cur,
            JournalWhenceV1::End => wasi::Whence::End,
            JournalWhenceV1::Unknown => wasi::Whence::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalWhenceV1> for wasi::Whence {
    fn from(val: &'_ ArchivedJournalWhenceV1) -> Self {
        match val {
            ArchivedJournalWhenceV1::Set => wasi::Whence::Set,
            ArchivedJournalWhenceV1::Cur => wasi::Whence::Cur,
            ArchivedJournalWhenceV1::End => wasi::Whence::End,
            ArchivedJournalWhenceV1::Unknown => wasi::Whence::Unknown,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalAdviceV1 {
    Normal,
    Sequential,
    Random,
    Willneed,
    Dontneed,
    Noreuse,
    Unknown = 255,
}

impl From<wasi::Advice> for JournalAdviceV1 {
    fn from(val: wasi::Advice) -> Self {
        match val {
            wasi::Advice::Normal => JournalAdviceV1::Normal,
            wasi::Advice::Sequential => JournalAdviceV1::Sequential,
            wasi::Advice::Random => JournalAdviceV1::Random,
            wasi::Advice::Willneed => JournalAdviceV1::Willneed,
            wasi::Advice::Dontneed => JournalAdviceV1::Dontneed,
            wasi::Advice::Noreuse => JournalAdviceV1::Noreuse,
            wasi::Advice::Unknown => JournalAdviceV1::Unknown,
        }
    }
}

impl From<JournalAdviceV1> for wasi::Advice {
    fn from(val: JournalAdviceV1) -> Self {
        match val {
            JournalAdviceV1::Normal => wasi::Advice::Normal,
            JournalAdviceV1::Sequential => wasi::Advice::Sequential,
            JournalAdviceV1::Random => wasi::Advice::Random,
            JournalAdviceV1::Willneed => wasi::Advice::Willneed,
            JournalAdviceV1::Dontneed => wasi::Advice::Dontneed,
            JournalAdviceV1::Noreuse => wasi::Advice::Noreuse,
            JournalAdviceV1::Unknown => wasi::Advice::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalAdviceV1> for wasi::Advice {
    fn from(val: &'_ ArchivedJournalAdviceV1) -> Self {
        match val {
            ArchivedJournalAdviceV1::Normal => wasi::Advice::Normal,
            ArchivedJournalAdviceV1::Sequential => wasi::Advice::Sequential,
            ArchivedJournalAdviceV1::Random => wasi::Advice::Random,
            ArchivedJournalAdviceV1::Willneed => wasi::Advice::Willneed,
            ArchivedJournalAdviceV1::Dontneed => wasi::Advice::Dontneed,
            ArchivedJournalAdviceV1::Noreuse => wasi::Advice::Noreuse,
            ArchivedJournalAdviceV1::Unknown => wasi::Advice::Unknown,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalExitCodeV1 {
    Errno(u16),
    Other(i32),
}

impl From<wasi::ExitCode> for JournalExitCodeV1 {
    fn from(val: wasi::ExitCode) -> Self {
        match val {
            wasi::ExitCode::Errno(errno) => JournalExitCodeV1::Errno(errno as u16),
            wasi::ExitCode::Other(id) => JournalExitCodeV1::Other(id),
        }
    }
}

impl From<JournalExitCodeV1> for wasi::ExitCode {
    fn from(val: JournalExitCodeV1) -> Self {
        match val {
            JournalExitCodeV1::Errno(errno) => {
                wasi::ExitCode::Errno(errno.try_into().unwrap_or(wasi::Errno::Unknown))
            }
            JournalExitCodeV1::Other(id) => wasi::ExitCode::Other(id),
        }
    }
}

impl From<&'_ ArchivedJournalExitCodeV1> for wasi::ExitCode {
    fn from(val: &'_ ArchivedJournalExitCodeV1) -> Self {
        match val {
            ArchivedJournalExitCodeV1::Errno(errno) => {
                wasi::ExitCode::Errno((*errno).try_into().unwrap_or(wasi::Errno::Unknown))
            }
            ArchivedJournalExitCodeV1::Other(id) => wasi::ExitCode::Other(*id),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalSnapshotTriggerV1 {
    Idle,
    Listen,
    Environ,
    Stdin,
    Timer,
    Sigint,
    Sigalrm,
    Sigtstp,
    Sigstop,
    NonDeterministicCall,
}

impl From<SnapshotTrigger> for JournalSnapshotTriggerV1 {
    fn from(val: SnapshotTrigger) -> Self {
        match val {
            SnapshotTrigger::Idle => JournalSnapshotTriggerV1::Idle,
            SnapshotTrigger::FirstListen => JournalSnapshotTriggerV1::Listen,
            SnapshotTrigger::FirstEnviron => JournalSnapshotTriggerV1::Environ,
            SnapshotTrigger::FirstStdin => JournalSnapshotTriggerV1::Stdin,
            SnapshotTrigger::PeriodicInterval => JournalSnapshotTriggerV1::Timer,
            SnapshotTrigger::Sigint => JournalSnapshotTriggerV1::Sigint,
            SnapshotTrigger::Sigalrm => JournalSnapshotTriggerV1::Sigalrm,
            SnapshotTrigger::Sigtstp => JournalSnapshotTriggerV1::Sigtstp,
            SnapshotTrigger::Sigstop => JournalSnapshotTriggerV1::Sigstop,
            SnapshotTrigger::NonDeterministicCall => JournalSnapshotTriggerV1::NonDeterministicCall,
        }
    }
}

impl From<JournalSnapshotTriggerV1> for SnapshotTrigger {
    fn from(val: JournalSnapshotTriggerV1) -> Self {
        match val {
            JournalSnapshotTriggerV1::Idle => SnapshotTrigger::Idle,
            JournalSnapshotTriggerV1::Listen => SnapshotTrigger::FirstListen,
            JournalSnapshotTriggerV1::Environ => SnapshotTrigger::FirstEnviron,
            JournalSnapshotTriggerV1::Stdin => SnapshotTrigger::FirstStdin,
            JournalSnapshotTriggerV1::Timer => SnapshotTrigger::PeriodicInterval,
            JournalSnapshotTriggerV1::Sigint => SnapshotTrigger::Sigint,
            JournalSnapshotTriggerV1::Sigalrm => SnapshotTrigger::Sigalrm,
            JournalSnapshotTriggerV1::Sigtstp => SnapshotTrigger::Sigtstp,
            JournalSnapshotTriggerV1::Sigstop => SnapshotTrigger::Sigstop,
            JournalSnapshotTriggerV1::NonDeterministicCall => SnapshotTrigger::NonDeterministicCall,
        }
    }
}

impl From<&'_ ArchivedJournalSnapshotTriggerV1> for SnapshotTrigger {
    fn from(val: &'_ ArchivedJournalSnapshotTriggerV1) -> Self {
        match val {
            ArchivedJournalSnapshotTriggerV1::Idle => SnapshotTrigger::Idle,
            ArchivedJournalSnapshotTriggerV1::Listen => SnapshotTrigger::FirstListen,
            ArchivedJournalSnapshotTriggerV1::Environ => SnapshotTrigger::FirstEnviron,
            ArchivedJournalSnapshotTriggerV1::Stdin => SnapshotTrigger::FirstStdin,
            ArchivedJournalSnapshotTriggerV1::Timer => SnapshotTrigger::PeriodicInterval,
            ArchivedJournalSnapshotTriggerV1::Sigint => SnapshotTrigger::Sigint,
            ArchivedJournalSnapshotTriggerV1::Sigalrm => SnapshotTrigger::Sigalrm,
            ArchivedJournalSnapshotTriggerV1::Sigtstp => SnapshotTrigger::Sigtstp,
            ArchivedJournalSnapshotTriggerV1::Sigstop => SnapshotTrigger::Sigstop,
            ArchivedJournalSnapshotTriggerV1::NonDeterministicCall => {
                SnapshotTrigger::NonDeterministicCall
            }
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub(crate) enum JournalEpollCtlV1 {
    Add,
    Mod,
    Del,
    Unknown,
}

impl From<wasi::EpollCtl> for JournalEpollCtlV1 {
    fn from(val: wasi::EpollCtl) -> Self {
        match val {
            wasi::EpollCtl::Add => JournalEpollCtlV1::Add,
            wasi::EpollCtl::Mod => JournalEpollCtlV1::Mod,
            wasi::EpollCtl::Del => JournalEpollCtlV1::Del,
            wasi::EpollCtl::Unknown => JournalEpollCtlV1::Unknown,
        }
    }
}

impl From<JournalEpollCtlV1> for wasi::EpollCtl {
    fn from(val: JournalEpollCtlV1) -> Self {
        match val {
            JournalEpollCtlV1::Add => wasi::EpollCtl::Add,
            JournalEpollCtlV1::Mod => wasi::EpollCtl::Mod,
            JournalEpollCtlV1::Del => wasi::EpollCtl::Del,
            JournalEpollCtlV1::Unknown => wasi::EpollCtl::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalEpollCtlV1> for wasi::EpollCtl {
    fn from(val: &'_ ArchivedJournalEpollCtlV1) -> Self {
        match val {
            ArchivedJournalEpollCtlV1::Add => wasi::EpollCtl::Add,
            ArchivedJournalEpollCtlV1::Mod => wasi::EpollCtl::Mod,
            ArchivedJournalEpollCtlV1::Del => wasi::EpollCtl::Del,
            ArchivedJournalEpollCtlV1::Unknown => wasi::EpollCtl::Unknown,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, RkyvSerialize, RkyvDeserialize, Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEpollEventCtlV1 {
    pub events: u32,
    pub ptr: u64,
    pub fd: u32,
    pub data1: u32,
    pub data2: u64,
}

impl From<EpollEventCtl> for JournalEpollEventCtlV1 {
    fn from(val: EpollEventCtl) -> Self {
        JournalEpollEventCtlV1 {
            events: val.events.bits(),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<JournalEpollEventCtlV1> for EpollEventCtl {
    fn from(val: JournalEpollEventCtlV1) -> Self {
        Self {
            events: EpollType::from_bits_truncate(val.events),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<&'_ ArchivedJournalEpollEventCtlV1> for EpollEventCtl {
    fn from(val: &'_ ArchivedJournalEpollEventCtlV1) -> Self {
        Self {
            events: EpollType::from_bits_truncate(val.events),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalStreamSecurityV1 {
    Unencrypted,
    AnyEncryption,
    ClassicEncryption,
    DoubleEncryption,
    Unknown,
}

impl From<StreamSecurity> for JournalStreamSecurityV1 {
    fn from(val: StreamSecurity) -> Self {
        match val {
            StreamSecurity::Unencrypted => JournalStreamSecurityV1::Unencrypted,
            StreamSecurity::AnyEncyption => JournalStreamSecurityV1::AnyEncryption,
            StreamSecurity::ClassicEncryption => JournalStreamSecurityV1::ClassicEncryption,
            StreamSecurity::DoubleEncryption => JournalStreamSecurityV1::DoubleEncryption,
        }
    }
}

impl From<JournalStreamSecurityV1> for StreamSecurity {
    fn from(val: JournalStreamSecurityV1) -> Self {
        match val {
            JournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            JournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            JournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            JournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            JournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

impl From<&'_ ArchivedJournalStreamSecurityV1> for StreamSecurity {
    fn from(val: &'_ ArchivedJournalStreamSecurityV1) -> Self {
        match val {
            ArchivedJournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            ArchivedJournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            ArchivedJournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            ArchivedJournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            ArchivedJournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalAddressfamilyV1 {
    Unspec,
    Inet4,
    Inet6,
    Unix,
}

impl From<wasi::Addressfamily> for JournalAddressfamilyV1 {
    fn from(val: wasi::Addressfamily) -> Self {
        match val {
            wasi::Addressfamily::Unspec => JournalAddressfamilyV1::Unspec,
            wasi::Addressfamily::Inet4 => JournalAddressfamilyV1::Inet4,
            wasi::Addressfamily::Inet6 => JournalAddressfamilyV1::Inet6,
            wasi::Addressfamily::Unix => JournalAddressfamilyV1::Unix,
        }
    }
}

impl From<JournalAddressfamilyV1> for wasi::Addressfamily {
    fn from(val: JournalAddressfamilyV1) -> Self {
        match val {
            JournalAddressfamilyV1::Unspec => wasi::Addressfamily::Unspec,
            JournalAddressfamilyV1::Inet4 => wasi::Addressfamily::Inet4,
            JournalAddressfamilyV1::Inet6 => wasi::Addressfamily::Inet6,
            JournalAddressfamilyV1::Unix => wasi::Addressfamily::Unix,
        }
    }
}

impl From<&'_ ArchivedJournalAddressfamilyV1> for wasi::Addressfamily {
    fn from(val: &'_ ArchivedJournalAddressfamilyV1) -> Self {
        match val {
            ArchivedJournalAddressfamilyV1::Unspec => wasi::Addressfamily::Unspec,
            ArchivedJournalAddressfamilyV1::Inet4 => wasi::Addressfamily::Inet4,
            ArchivedJournalAddressfamilyV1::Inet6 => wasi::Addressfamily::Inet6,
            ArchivedJournalAddressfamilyV1::Unix => wasi::Addressfamily::Unix,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSocktypeV1 {
    Unknown,
    Stream,
    Dgram,
    Raw,
    Seqpacket,
}

impl From<wasi::Socktype> for JournalSocktypeV1 {
    fn from(val: wasi::Socktype) -> Self {
        match val {
            wasi::Socktype::Stream => JournalSocktypeV1::Stream,
            wasi::Socktype::Dgram => JournalSocktypeV1::Dgram,
            wasi::Socktype::Raw => JournalSocktypeV1::Raw,
            wasi::Socktype::Seqpacket => JournalSocktypeV1::Seqpacket,
            wasi::Socktype::Unknown => JournalSocktypeV1::Unknown,
        }
    }
}

impl From<JournalSocktypeV1> for wasi::Socktype {
    fn from(val: JournalSocktypeV1) -> Self {
        match val {
            JournalSocktypeV1::Stream => wasi::Socktype::Stream,
            JournalSocktypeV1::Dgram => wasi::Socktype::Dgram,
            JournalSocktypeV1::Raw => wasi::Socktype::Raw,
            JournalSocktypeV1::Seqpacket => wasi::Socktype::Seqpacket,
            JournalSocktypeV1::Unknown => wasi::Socktype::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalSocktypeV1> for wasi::Socktype {
    fn from(val: &'_ ArchivedJournalSocktypeV1) -> Self {
        match val {
            ArchivedJournalSocktypeV1::Stream => wasi::Socktype::Stream,
            ArchivedJournalSocktypeV1::Dgram => wasi::Socktype::Dgram,
            ArchivedJournalSocktypeV1::Raw => wasi::Socktype::Raw,
            ArchivedJournalSocktypeV1::Seqpacket => wasi::Socktype::Seqpacket,
            ArchivedJournalSocktypeV1::Unknown => wasi::Socktype::Unknown,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSockoptionV1 {
    Noop,
    ReusePort,
    ReuseAddr,
    NoDelay,
    DontRoute,
    OnlyV6,
    Broadcast,
    MulticastLoopV4,
    MulticastLoopV6,
    Promiscuous,
    Listening,
    LastError,
    KeepAlive,
    Linger,
    OobInline,
    RecvBufSize,
    SendBufSize,
    RecvLowat,
    SendLowat,
    RecvTimeout,
    SendTimeout,
    ConnectTimeout,
    AcceptTimeout,
    Ttl,
    MulticastTtlV4,
    Type,
    Proto,
}

impl From<Sockoption> for JournalSockoptionV1 {
    fn from(val: wasi::Sockoption) -> Self {
        match val {
            wasi::Sockoption::Noop => JournalSockoptionV1::Noop,
            wasi::Sockoption::ReusePort => JournalSockoptionV1::ReusePort,
            wasi::Sockoption::ReuseAddr => JournalSockoptionV1::ReuseAddr,
            wasi::Sockoption::NoDelay => JournalSockoptionV1::NoDelay,
            wasi::Sockoption::DontRoute => JournalSockoptionV1::DontRoute,
            wasi::Sockoption::OnlyV6 => JournalSockoptionV1::OnlyV6,
            wasi::Sockoption::Broadcast => JournalSockoptionV1::Broadcast,
            wasi::Sockoption::MulticastLoopV4 => JournalSockoptionV1::MulticastLoopV4,
            wasi::Sockoption::MulticastLoopV6 => JournalSockoptionV1::MulticastLoopV6,
            wasi::Sockoption::Promiscuous => JournalSockoptionV1::Promiscuous,
            wasi::Sockoption::Listening => JournalSockoptionV1::Listening,
            wasi::Sockoption::LastError => JournalSockoptionV1::LastError,
            wasi::Sockoption::KeepAlive => JournalSockoptionV1::KeepAlive,
            wasi::Sockoption::Linger => JournalSockoptionV1::Linger,
            wasi::Sockoption::OobInline => JournalSockoptionV1::OobInline,
            wasi::Sockoption::RecvBufSize => JournalSockoptionV1::RecvBufSize,
            wasi::Sockoption::SendBufSize => JournalSockoptionV1::SendBufSize,
            wasi::Sockoption::RecvLowat => JournalSockoptionV1::RecvLowat,
            wasi::Sockoption::SendLowat => JournalSockoptionV1::SendLowat,
            wasi::Sockoption::RecvTimeout => JournalSockoptionV1::RecvTimeout,
            wasi::Sockoption::SendTimeout => JournalSockoptionV1::SendTimeout,
            wasi::Sockoption::ConnectTimeout => JournalSockoptionV1::ConnectTimeout,
            wasi::Sockoption::AcceptTimeout => JournalSockoptionV1::AcceptTimeout,
            wasi::Sockoption::Ttl => JournalSockoptionV1::Ttl,
            wasi::Sockoption::MulticastTtlV4 => JournalSockoptionV1::MulticastTtlV4,
            wasi::Sockoption::Type => JournalSockoptionV1::Type,
            wasi::Sockoption::Proto => JournalSockoptionV1::Proto,
        }
    }
}

impl From<JournalSockoptionV1> for wasi::Sockoption {
    fn from(val: JournalSockoptionV1) -> Self {
        match val {
            JournalSockoptionV1::Noop => wasi::Sockoption::Noop,
            JournalSockoptionV1::ReusePort => wasi::Sockoption::ReusePort,
            JournalSockoptionV1::ReuseAddr => wasi::Sockoption::ReuseAddr,
            JournalSockoptionV1::NoDelay => wasi::Sockoption::NoDelay,
            JournalSockoptionV1::DontRoute => wasi::Sockoption::DontRoute,
            JournalSockoptionV1::OnlyV6 => wasi::Sockoption::OnlyV6,
            JournalSockoptionV1::Broadcast => wasi::Sockoption::Broadcast,
            JournalSockoptionV1::MulticastLoopV4 => wasi::Sockoption::MulticastLoopV4,
            JournalSockoptionV1::MulticastLoopV6 => wasi::Sockoption::MulticastLoopV6,
            JournalSockoptionV1::Promiscuous => wasi::Sockoption::Promiscuous,
            JournalSockoptionV1::Listening => wasi::Sockoption::Listening,
            JournalSockoptionV1::LastError => wasi::Sockoption::LastError,
            JournalSockoptionV1::KeepAlive => wasi::Sockoption::KeepAlive,
            JournalSockoptionV1::Linger => wasi::Sockoption::Linger,
            JournalSockoptionV1::OobInline => wasi::Sockoption::OobInline,
            JournalSockoptionV1::RecvBufSize => wasi::Sockoption::RecvBufSize,
            JournalSockoptionV1::SendBufSize => wasi::Sockoption::SendBufSize,
            JournalSockoptionV1::RecvLowat => wasi::Sockoption::RecvLowat,
            JournalSockoptionV1::SendLowat => wasi::Sockoption::SendLowat,
            JournalSockoptionV1::RecvTimeout => wasi::Sockoption::RecvTimeout,
            JournalSockoptionV1::SendTimeout => wasi::Sockoption::SendTimeout,
            JournalSockoptionV1::ConnectTimeout => wasi::Sockoption::ConnectTimeout,
            JournalSockoptionV1::AcceptTimeout => wasi::Sockoption::AcceptTimeout,
            JournalSockoptionV1::Ttl => wasi::Sockoption::Ttl,
            JournalSockoptionV1::MulticastTtlV4 => wasi::Sockoption::MulticastTtlV4,
            JournalSockoptionV1::Type => wasi::Sockoption::Type,
            JournalSockoptionV1::Proto => wasi::Sockoption::Proto,
        }
    }
}

impl From<&'_ ArchivedJournalSockoptionV1> for wasi::Sockoption {
    fn from(val: &'_ ArchivedJournalSockoptionV1) -> Self {
        match val {
            ArchivedJournalSockoptionV1::Noop => wasi::Sockoption::Noop,
            ArchivedJournalSockoptionV1::ReusePort => wasi::Sockoption::ReusePort,
            ArchivedJournalSockoptionV1::ReuseAddr => wasi::Sockoption::ReuseAddr,
            ArchivedJournalSockoptionV1::NoDelay => wasi::Sockoption::NoDelay,
            ArchivedJournalSockoptionV1::DontRoute => wasi::Sockoption::DontRoute,
            ArchivedJournalSockoptionV1::OnlyV6 => wasi::Sockoption::OnlyV6,
            ArchivedJournalSockoptionV1::Broadcast => wasi::Sockoption::Broadcast,
            ArchivedJournalSockoptionV1::MulticastLoopV4 => wasi::Sockoption::MulticastLoopV4,
            ArchivedJournalSockoptionV1::MulticastLoopV6 => wasi::Sockoption::MulticastLoopV6,
            ArchivedJournalSockoptionV1::Promiscuous => wasi::Sockoption::Promiscuous,
            ArchivedJournalSockoptionV1::Listening => wasi::Sockoption::Listening,
            ArchivedJournalSockoptionV1::LastError => wasi::Sockoption::LastError,
            ArchivedJournalSockoptionV1::KeepAlive => wasi::Sockoption::KeepAlive,
            ArchivedJournalSockoptionV1::Linger => wasi::Sockoption::Linger,
            ArchivedJournalSockoptionV1::OobInline => wasi::Sockoption::OobInline,
            ArchivedJournalSockoptionV1::RecvBufSize => wasi::Sockoption::RecvBufSize,
            ArchivedJournalSockoptionV1::SendBufSize => wasi::Sockoption::SendBufSize,
            ArchivedJournalSockoptionV1::RecvLowat => wasi::Sockoption::RecvLowat,
            ArchivedJournalSockoptionV1::SendLowat => wasi::Sockoption::SendLowat,
            ArchivedJournalSockoptionV1::RecvTimeout => wasi::Sockoption::RecvTimeout,
            ArchivedJournalSockoptionV1::SendTimeout => wasi::Sockoption::SendTimeout,
            ArchivedJournalSockoptionV1::ConnectTimeout => wasi::Sockoption::ConnectTimeout,
            ArchivedJournalSockoptionV1::AcceptTimeout => wasi::Sockoption::AcceptTimeout,
            ArchivedJournalSockoptionV1::Ttl => wasi::Sockoption::Ttl,
            ArchivedJournalSockoptionV1::MulticastTtlV4 => wasi::Sockoption::MulticastTtlV4,
            ArchivedJournalSockoptionV1::Type => wasi::Sockoption::Type,
            ArchivedJournalSockoptionV1::Proto => wasi::Sockoption::Proto,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalTimeTypeV1 {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    BindTimeout,
    Linger,
}

impl From<TimeType> for JournalTimeTypeV1 {
    fn from(val: TimeType) -> Self {
        match val {
            TimeType::ReadTimeout => JournalTimeTypeV1::ReadTimeout,
            TimeType::WriteTimeout => JournalTimeTypeV1::WriteTimeout,
            TimeType::AcceptTimeout => JournalTimeTypeV1::AcceptTimeout,
            TimeType::ConnectTimeout => JournalTimeTypeV1::ConnectTimeout,
            TimeType::BindTimeout => JournalTimeTypeV1::BindTimeout,
            TimeType::Linger => JournalTimeTypeV1::Linger,
        }
    }
}

impl From<JournalTimeTypeV1> for TimeType {
    fn from(val: JournalTimeTypeV1) -> Self {
        match val {
            JournalTimeTypeV1::ReadTimeout => TimeType::ReadTimeout,
            JournalTimeTypeV1::WriteTimeout => TimeType::WriteTimeout,
            JournalTimeTypeV1::AcceptTimeout => TimeType::AcceptTimeout,
            JournalTimeTypeV1::ConnectTimeout => TimeType::ConnectTimeout,
            JournalTimeTypeV1::BindTimeout => TimeType::BindTimeout,
            JournalTimeTypeV1::Linger => TimeType::Linger,
        }
    }
}

impl From<&'_ ArchivedJournalTimeTypeV1> for TimeType {
    fn from(val: &'_ ArchivedJournalTimeTypeV1) -> Self {
        match val {
            ArchivedJournalTimeTypeV1::ReadTimeout => TimeType::ReadTimeout,
            ArchivedJournalTimeTypeV1::WriteTimeout => TimeType::WriteTimeout,
            ArchivedJournalTimeTypeV1::AcceptTimeout => TimeType::AcceptTimeout,
            ArchivedJournalTimeTypeV1::ConnectTimeout => TimeType::ConnectTimeout,
            ArchivedJournalTimeTypeV1::BindTimeout => TimeType::BindTimeout,
            ArchivedJournalTimeTypeV1::Linger => TimeType::Linger,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSocketShutdownV1 {
    Read,
    Write,
    Both,
}

impl From<Shutdown> for JournalSocketShutdownV1 {
    fn from(val: Shutdown) -> Self {
        match val {
            Shutdown::Read => JournalSocketShutdownV1::Read,
            Shutdown::Write => JournalSocketShutdownV1::Write,
            Shutdown::Both => JournalSocketShutdownV1::Both,
        }
    }
}

impl From<JournalSocketShutdownV1> for Shutdown {
    fn from(val: JournalSocketShutdownV1) -> Self {
        match val {
            JournalSocketShutdownV1::Read => Shutdown::Read,
            JournalSocketShutdownV1::Write => Shutdown::Write,
            JournalSocketShutdownV1::Both => Shutdown::Both,
        }
    }
}

impl From<&'_ ArchivedJournalSocketShutdownV1> for Shutdown {
    fn from(val: &'_ ArchivedJournalSocketShutdownV1) -> Self {
        match val {
            ArchivedJournalSocketShutdownV1::Read => Shutdown::Read,
            ArchivedJournalSocketShutdownV1::Write => Shutdown::Write,
            ArchivedJournalSocketShutdownV1::Both => Shutdown::Both,
        }
    }
}

impl<'a> From<JournalEntry<'a>> for JournalBatchEntry {
    fn from(value: JournalEntry<'a>) -> Self {
        match value {
            JournalEntry::InitModule { wasm_hash } => Self::InitModuleV1 { wasm_hash },
            JournalEntry::UpdateMemoryRegion { region, data } => Self::UpdateMemoryRegionV1 {
                start: region.start,
                end: region.end,
                data: data.into_owned(),
            },
            JournalEntry::ProcessExit { exit_code } => Self::ProcessExitV1 {
                exit_code: exit_code.map(|code| code.into()),
            },
            JournalEntry::SetThread {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThreadV1 {
                id: id.into(),
                call_stack: call_stack.into_owned(),
                memory_stack: memory_stack.into_owned(),
                store_data: store_data.into_owned(),
                is_64bit,
            },
            JournalEntry::CloseThread { id, exit_code } => Self::CloseThreadV1 {
                id: id.into(),
                exit_code: exit_code.map(|code| code.into()),
            },
            JournalEntry::FileDescriptorWrite {
                fd,
                offset,
                data,
                is_64bit,
            } => Self::FileDescriptorWriteV1 {
                fd,
                offset,
                data: data.into_owned(),
                is_64bit,
            },
            JournalEntry::FileDescriptorSeek { fd, offset, whence } => Self::FileDescriptorSeekV1 {
                fd,
                offset,
                whence: whence.into(),
            },
            JournalEntry::OpenFileDescriptor {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => Self::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path: path.into_owned(),
                o_flags: o_flags.bits(),
                fs_rights_base: fs_rights_base.bits(),
                fs_rights_inheriting: fs_rights_inheriting.bits(),
                fs_flags: fs_flags.bits(),
            },
            JournalEntry::CloseFileDescriptor { fd } => Self::CloseFileDescriptorV1 { fd },
            JournalEntry::RemoveDirectory { fd, path } => Self::RemoveDirectoryV1 {
                fd,
                path: path.into_owned(),
            },
            JournalEntry::UnlinkFile { fd, path } => Self::UnlinkFileV1 {
                fd,
                path: path.into_owned(),
            },
            JournalEntry::PathRename {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => Self::PathRenameV1 {
                old_fd,
                old_path: old_path.into_owned(),
                new_fd,
                new_path: new_path.into_owned(),
            },
            JournalEntry::Snapshot { when, trigger } => Self::SnapshotV1 {
                since_epoch: when
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO),
                trigger: trigger.into(),
            },
            JournalEntry::SetClockTime { clock_id, time } => Self::SetClockTimeV1 {
                clock_id: clock_id.into(),
                time,
            },
            JournalEntry::RenumberFileDescriptor { old_fd, new_fd } => {
                Self::RenumberFileDescriptorV1 { old_fd, new_fd }
            }
            JournalEntry::DuplicateFileDescriptor {
                original_fd,
                copied_fd,
            } => Self::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            },
            JournalEntry::CreateDirectory { fd, path } => Self::CreateDirectoryV1 {
                fd,
                path: path.into_owned(),
            },
            JournalEntry::PathSetTimes {
                fd,
                path,
                flags,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::PathSetTimesV1 {
                fd,
                path: path.into_owned(),
                flags,
                st_atim,
                st_mtim,
                fst_flags: fst_flags.bits(),
            },
            JournalEntry::FileDescriptorSetTimes {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags: fst_flags.bits(),
            },
            JournalEntry::FileDescriptorSetSize { fd, st_size } => {
                Self::FileDescriptorSetSizeV1 { fd, st_size }
            }
            JournalEntry::FileDescriptorSetFlags { fd, flags } => Self::FileDescriptorSetFlagsV1 {
                fd,
                flags: flags.bits(),
            },
            JournalEntry::FileDescriptorSetRights {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => Self::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base: fs_rights_base.bits(),
                fs_rights_inheriting: fs_rights_inheriting.bits(),
            },
            JournalEntry::FileDescriptorAdvise {
                fd,
                offset,
                len,
                advice,
            } => Self::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice: advice.into(),
            },
            JournalEntry::FileDescriptorAllocate { fd, offset, len } => {
                Self::FileDescriptorAllocateV1 { fd, offset, len }
            }
            JournalEntry::CreateHardLink {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => Self::CreateHardLinkV1 {
                old_fd,
                old_path: old_path.into_owned(),
                old_flags,
                new_fd,
                new_path: new_path.into_owned(),
            },
            JournalEntry::CreateSymbolicLink {
                old_path,
                fd,
                new_path,
            } => Self::CreateSymbolicLinkV1 {
                old_path: old_path.into_owned(),
                fd,
                new_path: new_path.into_owned(),
            },
            JournalEntry::ChangeDirectory { path } => Self::ChangeDirectoryV1 {
                path: path.into_owned(),
            },
            JournalEntry::EpollCreate { fd } => Self::EpollCreateV1 { fd },
            JournalEntry::EpollCtl {
                epfd,
                op,
                fd,
                event,
            } => Self::EpollCtlV1 {
                epfd,
                op: op.into(),
                fd,
                event: event.map(|e| e.into()),
            },
            JournalEntry::TtySet { tty, line_feeds } => Self::TtySetV1 {
                cols: tty.cols,
                rows: tty.rows,
                width: tty.width,
                height: tty.height,
                stdin_tty: tty.stdin_tty,
                stdout_tty: tty.stdout_tty,
                stderr_tty: tty.stderr_tty,
                echo: tty.echo,
                line_buffered: tty.line_buffered,
                line_feeds,
            },
            JournalEntry::CreatePipe { fd1, fd2 } => Self::CreatePipeV1 { fd1, fd2 },
            JournalEntry::PortAddAddr { cidr } => Self::PortAddAddrV1 { cidr },
            JournalEntry::PortDelAddr { addr } => Self::PortDelAddrV1 { addr },
            JournalEntry::PortAddrClear => Self::PortAddrClearV1,
            JournalEntry::PortBridge {
                network,
                token,
                security,
            } => Self::PortBridgeV1 {
                network: network.into(),
                token: token.into(),
                security: security.into(),
            },
            JournalEntry::PortUnbridge => Self::PortUnbridgeV1,
            JournalEntry::PortDhcpAcquire => Self::PortDhcpAcquireV1,
            JournalEntry::PortGatewaySet { ip } => Self::PortGatewaySetV1 { ip },
            JournalEntry::PortRouteAdd {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => Self::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            },
            JournalEntry::PortRouteClear => Self::PortRouteClearV1,
            JournalEntry::PortRouteDel { ip } => Self::PortRouteDelV1 { ip },
            JournalEntry::SocketOpen { af, ty, pt, fd } => Self::SocketOpenV1 {
                af: af.into(),
                ty: ty.into(),
                pt: pt as u16,
                fd,
            },
            JournalEntry::SocketListen { fd, backlog } => Self::SocketListenV1 { fd, backlog },
            JournalEntry::SocketBind { fd, addr } => Self::SocketBindV1 { fd, addr },
            JournalEntry::SocketConnected { fd, addr } => Self::SocketConnectedV1 { fd, addr },
            JournalEntry::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            } => Self::SocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags: fd_flags.bits(),
                nonblocking,
            },
            JournalEntry::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
            JournalEntry::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
            JournalEntry::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
            JournalEntry::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
            JournalEntry::SocketSendFile {
                socket_fd,
                file_fd,
                offset,
                count,
            } => Self::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            },
            JournalEntry::SocketSendTo {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => Self::SocketSendToV1 {
                fd,
                data: data.into(),
                flags,
                addr,
                is_64bit,
            },
            JournalEntry::SocketSend {
                fd,
                data,
                flags,
                is_64bit,
            } => Self::SocketSendV1 {
                fd,
                data: data.into(),
                flags,
                is_64bit,
            },
            JournalEntry::SocketSetOptFlag { fd, opt, flag } => Self::SocketSetOptFlagV1 {
                fd,
                opt: opt.into(),
                flag,
            },
            JournalEntry::SocketSetOptSize { fd, opt, size } => Self::SocketSetOptSizeV1 {
                fd,
                opt: opt.into(),
                size,
            },
            JournalEntry::SocketSetOptTime { fd, ty, time } => Self::SocketSetOptTimeV1 {
                fd,
                ty: ty.into(),
                time,
            },
            JournalEntry::SocketShutdown { fd, how } => Self::SocketShutdownV1 {
                fd,
                how: how.into(),
            },
            JournalEntry::CreateEvent {
                initial_val,
                flags,
                fd,
            } => Self::CreateEventV1 {
                initial_val,
                flags,
                fd,
            },
        }
    }
}

impl<'a> From<JournalBatchEntry> for JournalEntry<'a> {
    fn from(value: JournalBatchEntry) -> Self {
        match value {
            JournalBatchEntry::InitModuleV1 { wasm_hash } => Self::InitModule { wasm_hash },
            JournalBatchEntry::UpdateMemoryRegionV1 { start, end, data } => {
                Self::UpdateMemoryRegion {
                    region: start..end,
                    data: data.into(),
                }
            }
            JournalBatchEntry::ProcessExitV1 { exit_code } => Self::ProcessExit {
                exit_code: exit_code.map(|code| code.into()),
            },
            JournalBatchEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThread {
                id: id.into(),
                call_stack: call_stack.into(),
                memory_stack: memory_stack.into(),
                store_data: store_data.into(),
                is_64bit,
            },
            JournalBatchEntry::CloseThreadV1 { id, exit_code } => Self::CloseThread {
                id: id.into(),
                exit_code: exit_code.map(|code| code.into()),
            },
            JournalBatchEntry::FileDescriptorWriteV1 {
                data,
                fd,
                offset,
                is_64bit,
            } => Self::FileDescriptorWrite {
                data: data.into(),
                fd,
                offset,
                is_64bit,
            },
            JournalBatchEntry::FileDescriptorSeekV1 { fd, offset, whence } => {
                Self::FileDescriptorSeek {
                    fd,
                    offset,
                    whence: whence.into(),
                }
            }
            JournalBatchEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => Self::OpenFileDescriptor {
                fd,
                dirfd,
                dirflags,
                path: path.into(),
                o_flags: wasi::Oflags::from_bits_truncate(o_flags),
                fs_rights_base: wasi::Rights::from_bits_truncate(fs_rights_base),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(fs_rights_inheriting),
                fs_flags: wasi::Fdflags::from_bits_truncate(fs_flags),
            },
            JournalBatchEntry::CloseFileDescriptorV1 { fd } => Self::CloseFileDescriptor { fd },
            JournalBatchEntry::RemoveDirectoryV1 { fd, path } => Self::RemoveDirectory {
                fd,
                path: path.into(),
            },
            JournalBatchEntry::UnlinkFileV1 { fd, path } => Self::UnlinkFile {
                fd,
                path: path.into(),
            },
            JournalBatchEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => Self::PathRename {
                old_fd,
                old_path: old_path.into(),
                new_fd,
                new_path: new_path.into(),
            },
            JournalBatchEntry::SnapshotV1 {
                since_epoch,
                trigger,
            } => Self::Snapshot {
                when: SystemTime::UNIX_EPOCH
                    .checked_add(since_epoch)
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                trigger: trigger.into(),
            },
            JournalBatchEntry::SetClockTimeV1 { clock_id, time } => Self::SetClockTime {
                clock_id: clock_id.into(),
                time,
            },
            JournalBatchEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                Self::RenumberFileDescriptor { old_fd, new_fd }
            }
            JournalBatchEntry::DuplicateFileDescriptorV1 {
                original_fd: old_fd,
                copied_fd: new_fd,
            } => Self::DuplicateFileDescriptor {
                original_fd: old_fd,
                copied_fd: new_fd,
            },
            JournalBatchEntry::CreateDirectoryV1 { fd, path } => Self::CreateDirectory {
                fd,
                path: path.into(),
            },
            JournalBatchEntry::PathSetTimesV1 {
                fd,
                path,
                flags,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::PathSetTimes {
                fd,
                path: path.into(),
                flags,
                st_atim,
                st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(fst_flags),
            },
            JournalBatchEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::FileDescriptorSetTimes {
                fd,
                st_atim,
                st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(fst_flags),
            },
            JournalBatchEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                Self::FileDescriptorSetSize { fd, st_size }
            }
            JournalBatchEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                Self::FileDescriptorSetFlags {
                    fd,
                    flags: Fdflags::from_bits_truncate(flags),
                }
            }
            JournalBatchEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => Self::FileDescriptorSetRights {
                fd,
                fs_rights_base: Rights::from_bits_truncate(fs_rights_base),
                fs_rights_inheriting: Rights::from_bits_truncate(fs_rights_inheriting),
            },
            JournalBatchEntry::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => Self::FileDescriptorAdvise {
                fd,
                offset,
                len,
                advice: advice.into(),
            },
            JournalBatchEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                Self::FileDescriptorAllocate { fd, offset, len }
            }
            JournalBatchEntry::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => Self::CreateHardLink {
                old_fd,
                old_path: old_path.into(),
                old_flags,
                new_fd,
                new_path: new_path.into(),
            },
            JournalBatchEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => Self::CreateSymbolicLink {
                old_path: old_path.into(),
                fd,
                new_path: new_path.into(),
            },
            JournalBatchEntry::ChangeDirectoryV1 { path } => {
                Self::ChangeDirectory { path: path.into() }
            }
            JournalBatchEntry::EpollCreateV1 { fd } => Self::EpollCreate { fd },
            JournalBatchEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => Self::EpollCtl {
                epfd,
                op: op.into(),
                fd,
                event: event.map(|e| e.into()),
            },
            JournalBatchEntry::TtySetV1 {
                cols,
                rows,
                width,
                height,
                stdin_tty,
                stdout_tty,
                stderr_tty,
                echo,
                line_buffered,
                line_feeds,
            } => Self::TtySet {
                tty: wasi::Tty {
                    cols,
                    rows,
                    width,
                    height,
                    stdin_tty,
                    stdout_tty,
                    stderr_tty,
                    echo,
                    line_buffered,
                },
                line_feeds,
            },
            JournalBatchEntry::CreatePipeV1 { fd1, fd2 } => Self::CreatePipe { fd1, fd2 },
            JournalBatchEntry::PortAddAddrV1 { cidr } => Self::PortAddAddr { cidr },
            JournalBatchEntry::PortDelAddrV1 { addr } => Self::PortDelAddr { addr },
            JournalBatchEntry::PortAddrClearV1 => Self::PortAddrClear,
            JournalBatchEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => Self::PortBridge {
                network: network.into(),
                token: token.into(),
                security: security.into(),
            },
            JournalBatchEntry::PortUnbridgeV1 => Self::PortUnbridge,
            JournalBatchEntry::PortDhcpAcquireV1 => Self::PortDhcpAcquire,
            JournalBatchEntry::PortGatewaySetV1 { ip } => Self::PortGatewaySet { ip },
            JournalBatchEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => Self::PortRouteAdd {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            },
            JournalBatchEntry::PortRouteClearV1 => Self::PortRouteClear,
            JournalBatchEntry::PortRouteDelV1 { ip } => Self::PortRouteDel { ip },
            JournalBatchEntry::SocketOpenV1 { af, ty, pt, fd } => Self::SocketOpen {
                af: af.into(),
                ty: ty.into(),
                pt: pt.try_into().unwrap_or(wasi::SockProto::Max),
                fd,
            },
            JournalBatchEntry::SocketListenV1 { fd, backlog } => Self::SocketListen { fd, backlog },
            JournalBatchEntry::SocketBindV1 { fd, addr } => Self::SocketBind { fd, addr },
            JournalBatchEntry::SocketConnectedV1 { fd, addr } => Self::SocketConnected { fd, addr },
            JournalBatchEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            } => Self::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                fd_flags: Fdflags::from_bits_truncate(fd_flags),
                nonblocking,
            },
            JournalBatchEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            JournalBatchEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            JournalBatchEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            JournalBatchEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            JournalBatchEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => Self::SocketSendFile {
                socket_fd,
                file_fd,
                offset,
                count,
            },
            JournalBatchEntry::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => Self::SocketSendTo {
                fd,
                data: data.into(),
                flags,
                addr,
                is_64bit,
            },
            JournalBatchEntry::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => Self::SocketSend {
                fd,
                data: data.into(),
                flags,
                is_64bit,
            },
            JournalBatchEntry::SocketSetOptFlagV1 { fd, opt, flag } => Self::SocketSetOptFlag {
                fd,
                opt: opt.into(),
                flag,
            },
            JournalBatchEntry::SocketSetOptSizeV1 { fd, opt, size } => Self::SocketSetOptSize {
                fd,
                opt: opt.into(),
                size,
            },
            JournalBatchEntry::SocketSetOptTimeV1 { fd, ty, time } => Self::SocketSetOptTime {
                fd,
                ty: ty.into(),
                time,
            },
            JournalBatchEntry::SocketShutdownV1 { fd, how } => Self::SocketShutdown {
                fd,
                how: how.into(),
            },
            JournalBatchEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => Self::CreateEvent {
                initial_val,
                flags,
                fd,
            },
        }
    }
}

impl<'a> From<&'a ArchivedJournalBatchEntry> for JournalEntry<'a> {
    fn from(value: &'a ArchivedJournalBatchEntry) -> Self {
        type A = ArchivedJournalBatchEntry;
        match value {
            A::InitModuleV1 { wasm_hash } => Self::InitModule {
                wasm_hash: *wasm_hash,
            },
            A::UpdateMemoryRegionV1 { start, end, data } => Self::UpdateMemoryRegion {
                region: *start..*end,
                data: Cow::Borrowed(data.as_ref()),
            },
            A::ProcessExitV1 { exit_code } => Self::ProcessExit {
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            A::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThread {
                id: (*id).into(),
                call_stack: Cow::Borrowed(call_stack.as_ref()),
                memory_stack: Cow::Borrowed(memory_stack.as_ref()),
                store_data: Cow::Borrowed(store_data.as_ref()),
                is_64bit: *is_64bit,
            },
            A::CloseThreadV1 { id, exit_code } => Self::CloseThread {
                id: (*id).into(),
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            A::FileDescriptorWriteV1 {
                data,
                fd,
                offset,
                is_64bit,
            } => Self::FileDescriptorWrite {
                data: Cow::Borrowed(data.as_ref()),
                fd: *fd,
                offset: *offset,
                is_64bit: *is_64bit,
            },
            A::FileDescriptorSeekV1 { fd, offset, whence } => Self::FileDescriptorSeek {
                fd: *fd,
                offset: *offset,
                whence: whence.into(),
            },
            A::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => Self::OpenFileDescriptor {
                fd: *fd,
                dirfd: *dirfd,
                dirflags: *dirflags,
                path: Cow::Borrowed(path.as_str()),
                o_flags: wasi::Oflags::from_bits_truncate(*o_flags),
                fs_rights_base: wasi::Rights::from_bits_truncate(*fs_rights_base),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(*fs_rights_inheriting),
                fs_flags: wasi::Fdflags::from_bits_truncate(*fs_flags),
            },
            A::CloseFileDescriptorV1 { fd } => Self::CloseFileDescriptor { fd: *fd },
            A::RemoveDirectoryV1 { fd, path } => Self::RemoveDirectory {
                fd: *fd,
                path: Cow::Borrowed(path.as_str()),
            },
            A::UnlinkFileV1 { fd, path } => Self::UnlinkFile {
                fd: *fd,
                path: Cow::Borrowed(path.as_str()),
            },
            A::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => Self::PathRename {
                old_fd: *old_fd,
                old_path: Cow::Borrowed(old_path.as_str()),
                new_fd: *new_fd,
                new_path: Cow::Borrowed(new_path.as_str()),
            },
            A::SnapshotV1 {
                since_epoch,
                trigger,
            } => Self::Snapshot {
                when: SystemTime::UNIX_EPOCH
                    .checked_add(Duration::from_nanos(since_epoch.as_nanos() as u64))
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                trigger: trigger.into(),
            },
            A::SetClockTimeV1 { clock_id, time } => Self::SetClockTime {
                clock_id: clock_id.into(),
                time: *time,
            },
            A::RenumberFileDescriptorV1 { old_fd, new_fd } => Self::RenumberFileDescriptor {
                old_fd: *old_fd,
                new_fd: *new_fd,
            },
            A::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => Self::DuplicateFileDescriptor {
                original_fd: *original_fd,
                copied_fd: *copied_fd,
            },
            A::CreateDirectoryV1 { fd, path } => Self::CreateDirectory {
                fd: *fd,
                path: Cow::Borrowed(path.as_str()),
            },
            A::PathSetTimesV1 {
                fd,
                path,
                flags,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::PathSetTimes {
                fd: *fd,
                path: Cow::Borrowed(path.as_str()),
                flags: *flags,
                st_atim: *st_atim,
                st_mtim: *st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(*fst_flags),
            },
            A::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => Self::FileDescriptorSetTimes {
                fd: *fd,
                st_atim: *st_atim,
                st_mtim: *st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(*fst_flags),
            },
            A::FileDescriptorSetSizeV1 { fd, st_size } => Self::FileDescriptorSetSize {
                fd: *fd,
                st_size: *st_size,
            },
            A::FileDescriptorSetFlagsV1 { fd, flags } => Self::FileDescriptorSetFlags {
                fd: *fd,
                flags: Fdflags::from_bits_truncate(*flags),
            },
            A::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => Self::FileDescriptorSetRights {
                fd: *fd,
                fs_rights_base: Rights::from_bits_truncate(*fs_rights_base),
                fs_rights_inheriting: Rights::from_bits_truncate(*fs_rights_inheriting),
            },
            A::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => Self::FileDescriptorAdvise {
                fd: *fd,
                offset: *offset,
                len: *len,
                advice: advice.into(),
            },
            A::FileDescriptorAllocateV1 { fd, offset, len } => Self::FileDescriptorAllocate {
                fd: *fd,
                offset: *offset,
                len: *len,
            },
            A::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => Self::CreateHardLink {
                old_fd: *old_fd,
                old_path: Cow::Borrowed(old_path.as_str()),
                old_flags: *old_flags,
                new_fd: *new_fd,
                new_path: Cow::Borrowed(new_path.as_str()),
            },
            A::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => Self::CreateSymbolicLink {
                old_path: Cow::Borrowed(old_path.as_str()),
                fd: *fd,
                new_path: Cow::Borrowed(new_path.as_str()),
            },
            A::ChangeDirectoryV1 { path } => Self::ChangeDirectory {
                path: Cow::Borrowed(path.as_str()),
            },
            A::EpollCreateV1 { fd } => Self::EpollCreate { fd: *fd },
            A::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => Self::EpollCtl {
                epfd: *epfd,
                op: op.into(),
                fd: *fd,
                event: event.as_ref().map(|e| e.into()),
            },
            A::TtySetV1 {
                cols,
                rows,
                width,
                height,
                stdin_tty,
                stdout_tty,
                stderr_tty,
                echo,
                line_buffered,
                line_feeds,
            } => Self::TtySet {
                tty: wasi::Tty {
                    cols: *cols,
                    rows: *rows,
                    width: *width,
                    height: *height,
                    stdin_tty: *stdin_tty,
                    stdout_tty: *stdout_tty,
                    stderr_tty: *stderr_tty,
                    echo: *echo,
                    line_buffered: *line_buffered,
                },
                line_feeds: *line_feeds,
            },
            A::CreatePipeV1 { fd1, fd2 } => Self::CreatePipe {
                fd1: *fd1,
                fd2: *fd2,
            },
            A::PortAddAddrV1 { cidr } => Self::PortAddAddr {
                cidr: IpCidr {
                    ip: cidr.ip.as_ipaddr(),
                    prefix: cidr.prefix,
                },
            },
            A::PortDelAddrV1 { addr } => Self::PortDelAddr {
                addr: addr.as_ipaddr(),
            },
            A::PortAddrClearV1 => Self::PortAddrClear,
            A::PortBridgeV1 {
                network,
                token,
                security,
            } => Self::PortBridge {
                network: Cow::Borrowed(network.as_str()),
                token: Cow::Borrowed(token.as_str()),
                security: security.into(),
            },
            A::PortUnbridgeV1 => Self::PortUnbridge,
            A::PortDhcpAcquireV1 => Self::PortDhcpAcquire,
            A::PortGatewaySetV1 { ip } => Self::PortGatewaySet { ip: ip.as_ipaddr() },
            A::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => Self::PortRouteAdd {
                cidr: IpCidr {
                    ip: cidr.ip.as_ipaddr(),
                    prefix: cidr.prefix,
                },
                via_router: via_router.as_ipaddr(),
                preferred_until: preferred_until
                    .as_ref()
                    .map(|d| Duration::from_nanos(d.as_nanos() as u64)),
                expires_at: expires_at
                    .as_ref()
                    .map(|d| Duration::from_nanos(d.as_nanos() as u64)),
            },
            A::PortRouteClearV1 => Self::PortRouteClear,
            A::PortRouteDelV1 { ip } => Self::PortRouteDel { ip: ip.as_ipaddr() },
            A::SocketOpenV1 { af, ty, pt, fd } => Self::SocketOpen {
                af: af.into(),
                ty: ty.into(),
                pt: (*pt).try_into().unwrap_or(wasi::SockProto::Max),
                fd: *fd,
            },
            A::SocketListenV1 { fd, backlog } => Self::SocketListen {
                fd: *fd,
                backlog: *backlog,
            },
            A::SocketBindV1 { fd, addr } => Self::SocketBind {
                fd: *fd,
                addr: addr.as_socket_addr(),
            },
            A::SocketConnectedV1 { fd, addr } => Self::SocketConnected {
                fd: *fd,
                addr: addr.as_socket_addr(),
            },
            A::SocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            } => Self::SocketAccepted {
                listen_fd: *listen_fd,
                fd: *fd,
                peer_addr: peer_addr.as_socket_addr(),
                fd_flags: Fdflags::from_bits_truncate(*fd_flags),
                nonblocking: *nonblocking,
            },
            A::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv4Multicast {
                fd: *fd,
                multiaddr: multiaddr.as_ipv4(),
                iface: iface.as_ipv4(),
            },
            A::SocketJoinIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv6Multicast {
                fd: *fd,
                multiaddr: multiaddr.as_ipv6(),
                iface: *iface,
            },
            A::SocketLeaveIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv4Multicast {
                fd: *fd,
                multiaddr: multiaddr.as_ipv4(),
                iface: iface.as_ipv4(),
            },
            A::SocketLeaveIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv6Multicast {
                fd: *fd,
                multiaddr: multiaddr.as_ipv6(),
                iface: *iface,
            },
            A::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => Self::SocketSendFile {
                socket_fd: *socket_fd,
                file_fd: *file_fd,
                offset: *offset,
                count: *count,
            },
            A::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => Self::SocketSendTo {
                fd: *fd,
                data: Cow::Borrowed(data.as_ref()),
                flags: *flags,
                addr: addr.as_socket_addr(),
                is_64bit: *is_64bit,
            },
            A::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => Self::SocketSend {
                fd: *fd,
                data: Cow::Borrowed(data.as_ref()),
                flags: *flags,
                is_64bit: *is_64bit,
            },
            A::SocketSetOptFlagV1 { fd, opt, flag } => Self::SocketSetOptFlag {
                fd: *fd,
                opt: opt.into(),
                flag: *flag,
            },
            A::SocketSetOptSizeV1 { fd, opt, size } => Self::SocketSetOptSize {
                fd: *fd,
                opt: opt.into(),
                size: *size,
            },
            A::SocketSetOptTimeV1 { fd, ty, time } => Self::SocketSetOptTime {
                fd: *fd,
                ty: ty.into(),
                time: time
                    .as_ref()
                    .map(|d| Duration::from_nanos(d.as_nanos() as u64)),
            },
            A::SocketShutdownV1 { fd, how } => Self::SocketShutdown {
                fd: *fd,
                how: how.into(),
            },
            A::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => Self::CreateEvent {
                initial_val: *initial_val,
                flags: *flags,
                fd: *fd,
            },
        }
    }
}
