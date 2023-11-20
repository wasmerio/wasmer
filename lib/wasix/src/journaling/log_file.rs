use serde::{Deserialize, Serialize};
use std::{
    io::{self, ErrorKind, SeekFrom},
    mem::MaybeUninit,
    path::Path,
    time::SystemTime,
};
use tokio::runtime::Handle;
use virtual_net::{IpAddr, IpCidr, Ipv4Addr, Ipv6Addr, SocketAddr};
use wasmer_wasix_types::wasi::{
    self, Addressfamily, EpollEventCtl, Sockoption, Socktype, Streamsecurity,
};

use futures::future::LocalBoxFuture;
use virtual_fs::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::WasiThreadId;

use super::*;

/// The journal log entries are serializable which
/// allows them to be written directly to a file
///
/// Note: This structure is versioned which allows for
/// changes to the journal entry types without having to
/// worry about backward and forward compatibility
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum LogFileJournalEntry {
    InitModuleV1 {
        wasm_hash: [u8; 32],
    },
    ProcessExitV1 {
        exit_code: Option<JournalExitCodeV1>,
    },
    SetThreadV1 {
        id: WasiThreadId,
        call_stack: Vec<u8>,
        memory_stack: Vec<u8>,
        store_data: Vec<u8>,
        is_64bit: bool,
    },
    CloseThreadV1 {
        id: WasiThreadId,
        exit_code: Option<JournalExitCodeV1>,
    },
    FileDescriptorSeekV1 {
        fd: Fd,
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
        fd: Fd,
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
        event: Option<EpollEventCtl>,
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
        preferred_until: Option<Timestamp>,
        expires_at: Option<Timestamp>,
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
    SocketConnectV1 {
        fd: u32,
        addr: SocketAddr,
    },
    SocketAcceptV1 {
        listen_fd: u32,
        fd: u32,
        peer_addr: SocketAddr,
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
    },
    SocketSendToV1 {
        fd: u32,
        data: Vec<u8>,
        flags: u16,
        addr: SocketAddr,
    },
    SocketSendV1 {
        fd: u32,
        data: Vec<u8>,
        flags: u16,
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
        opt: JournalSockoptionV1,
        size: Option<u64>,
    },
    SocketShutdownV1 {
        fd: u32,
        how: u8,
    },
    SnapshotV1 {
        when: SystemTime,
        trigger: JournalSnapshotTriggerV1,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            Snapshot0Clockid::Realtime => JournalSnapshot0ClockidV1::Realtime,
            Snapshot0Clockid::Monotonic => JournalSnapshot0ClockidV1::Monotonic,
            Snapshot0Clockid::ProcessCputimeId => JournalSnapshot0ClockidV1::ProcessCputimeId,
            Snapshot0Clockid::ThreadCputimeId => JournalSnapshot0ClockidV1::ThreadCputimeId,
            Snapshot0Clockid::Unknown => JournalSnapshot0ClockidV1::Unknown,
        }
    }
}

impl From<JournalSnapshot0ClockidV1> for wasi::Snapshot0Clockid {
    fn from(val: JournalSnapshot0ClockidV1) -> Self {
        match val {
            JournalSnapshot0ClockidV1::Realtime => Snapshot0Clockid::Realtime,
            JournalSnapshot0ClockidV1::Monotonic => Snapshot0Clockid::Monotonic,
            JournalSnapshot0ClockidV1::ProcessCputimeId => Snapshot0Clockid::ProcessCputimeId,
            JournalSnapshot0ClockidV1::ThreadCputimeId => Snapshot0Clockid::ThreadCputimeId,
            JournalSnapshot0ClockidV1::Unknown => Snapshot0Clockid::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            JournalWhenceV1::Set => Whence::Set,
            JournalWhenceV1::Cur => Whence::Cur,
            JournalWhenceV1::End => Whence::End,
            JournalWhenceV1::Unknown => Whence::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            Advice::Normal => JournalAdviceV1::Normal,
            Advice::Sequential => JournalAdviceV1::Sequential,
            Advice::Random => JournalAdviceV1::Random,
            Advice::Willneed => JournalAdviceV1::Willneed,
            Advice::Dontneed => JournalAdviceV1::Dontneed,
            Advice::Noreuse => JournalAdviceV1::Noreuse,
            Advice::Unknown => JournalAdviceV1::Unknown,
        }
    }
}

impl From<JournalAdviceV1> for wasi::Advice {
    fn from(val: JournalAdviceV1) -> Self {
        match val {
            JournalAdviceV1::Normal => Advice::Normal,
            JournalAdviceV1::Sequential => Advice::Sequential,
            JournalAdviceV1::Random => Advice::Random,
            JournalAdviceV1::Willneed => Advice::Willneed,
            JournalAdviceV1::Dontneed => Advice::Dontneed,
            JournalAdviceV1::Noreuse => Advice::Noreuse,
            JournalAdviceV1::Unknown => Advice::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
            JournalEpollCtlV1::Add => EpollCtl::Add,
            JournalEpollCtlV1::Mod => EpollCtl::Mod,
            JournalEpollCtlV1::Del => EpollCtl::Del,
            JournalEpollCtlV1::Unknown => EpollCtl::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum JournalStreamSecurityV1 {
    Unencrypted,
    AnyEncryption,
    ClassicEncryption,
    DoubleEncryption,
    Unknown,
}

impl From<Streamsecurity> for JournalStreamSecurityV1 {
    fn from(val: Streamsecurity) -> Self {
        match val {
            Streamsecurity::Unencrypted => JournalStreamSecurityV1::Unencrypted,
            Streamsecurity::AnyEncryption => JournalStreamSecurityV1::AnyEncryption,
            Streamsecurity::ClassicEncryption => JournalStreamSecurityV1::ClassicEncryption,
            Streamsecurity::DoubleEncryption => JournalStreamSecurityV1::DoubleEncryption,
            Streamsecurity::Unknown => JournalStreamSecurityV1::Unknown,
        }
    }
}

impl From<JournalStreamSecurityV1> for Streamsecurity {
    fn from(val: JournalStreamSecurityV1) -> Self {
        match val {
            JournalStreamSecurityV1::Unencrypted => Streamsecurity::Unencrypted,
            JournalStreamSecurityV1::AnyEncryption => Streamsecurity::AnyEncryption,
            JournalStreamSecurityV1::ClassicEncryption => Streamsecurity::ClassicEncryption,
            JournalStreamSecurityV1::DoubleEncryption => Streamsecurity::DoubleEncryption,
            JournalStreamSecurityV1::Unknown => Streamsecurity::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum JournalAddressfamilyV1 {
    Unspec,
    Inet4,
    Inet6,
    Unix,
}

impl From<Addressfamily> for JournalAddressfamilyV1 {
    fn from(val: Addressfamily) -> Self {
        match val {
            Addressfamily::Unspec => JournalAddressfamilyV1::Unspec,
            Addressfamily::Inet4 => JournalAddressfamilyV1::Inet4,
            Addressfamily::Inet6 => JournalAddressfamilyV1::Inet6,
            Addressfamily::Unix => JournalAddressfamilyV1::Unix,
        }
    }
}

impl From<JournalAddressfamilyV1> for Addressfamily {
    fn from(val: JournalAddressfamilyV1) -> Self {
        match val {
            JournalAddressfamilyV1::Unspec => Addressfamily::Unspec,
            JournalAddressfamilyV1::Inet4 => Addressfamily::Inet4,
            JournalAddressfamilyV1::Inet6 => Addressfamily::Inet6,
            JournalAddressfamilyV1::Unix => Addressfamily::Unix,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum JournalSocktypeV1 {
    Unknown,
    Stream,
    Dgram,
    Raw,
    Seqpacket,
}

impl From<Socktype> for JournalSocktypeV1 {
    fn from(val: Socktype) -> Self {
        match val {
            Socktype::Stream => JournalSocktypeV1::Stream,
            Socktype::Dgram => JournalSocktypeV1::Dgram,
            Socktype::Raw => JournalSocktypeV1::Raw,
            Socktype::Seqpacket => JournalSocktypeV1::Seqpacket,
            Socktype::Unknown => JournalSocktypeV1::Unknown,
        }
    }
}

impl From<JournalSocktypeV1> for Socktype {
    fn from(val: JournalSocktypeV1) -> Self {
        match val {
            JournalSocktypeV1::Stream => Socktype::Stream,
            JournalSocktypeV1::Dgram => Socktype::Dgram,
            JournalSocktypeV1::Raw => Socktype::Raw,
            JournalSocktypeV1::Seqpacket => Socktype::Seqpacket,
            JournalSocktypeV1::Unknown => Socktype::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    fn from(val: Sockoption) -> Self {
        match val {
            Sockoption::Noop => JournalSockoptionV1::Noop,
            Sockoption::ReusePort => JournalSockoptionV1::ReusePort,
            Sockoption::ReuseAddr => JournalSockoptionV1::ReuseAddr,
            Sockoption::NoDelay => JournalSockoptionV1::NoDelay,
            Sockoption::DontRoute => JournalSockoptionV1::DontRoute,
            Sockoption::OnlyV6 => JournalSockoptionV1::OnlyV6,
            Sockoption::Broadcast => JournalSockoptionV1::Broadcast,
            Sockoption::MulticastLoopV4 => JournalSockoptionV1::MulticastLoopV4,
            Sockoption::MulticastLoopV6 => JournalSockoptionV1::MulticastLoopV6,
            Sockoption::Promiscuous => JournalSockoptionV1::Promiscuous,
            Sockoption::Listening => JournalSockoptionV1::Listening,
            Sockoption::LastError => JournalSockoptionV1::LastError,
            Sockoption::KeepAlive => JournalSockoptionV1::KeepAlive,
            Sockoption::Linger => JournalSockoptionV1::Linger,
            Sockoption::OobInline => JournalSockoptionV1::OobInline,
            Sockoption::RecvBufSize => JournalSockoptionV1::RecvBufSize,
            Sockoption::SendBufSize => JournalSockoptionV1::SendBufSize,
            Sockoption::RecvLowat => JournalSockoptionV1::RecvLowat,
            Sockoption::SendLowat => JournalSockoptionV1::SendLowat,
            Sockoption::RecvTimeout => JournalSockoptionV1::RecvTimeout,
            Sockoption::SendTimeout => JournalSockoptionV1::SendTimeout,
            Sockoption::ConnectTimeout => JournalSockoptionV1::ConnectTimeout,
            Sockoption::AcceptTimeout => JournalSockoptionV1::AcceptTimeout,
            Sockoption::Ttl => JournalSockoptionV1::Ttl,
            Sockoption::MulticastTtlV4 => JournalSockoptionV1::MulticastTtlV4,
            Sockoption::Type => JournalSockoptionV1::Type,
            Sockoption::Proto => JournalSockoptionV1::Proto,
        }
    }
}

impl From<JournalSockoptionV1> for Sockoption {
    fn from(val: JournalSockoptionV1) -> Self {
        match val {
            JournalSockoptionV1::Noop => Sockoption::Noop,
            JournalSockoptionV1::ReusePort => Sockoption::ReusePort,
            JournalSockoptionV1::ReuseAddr => Sockoption::ReuseAddr,
            JournalSockoptionV1::NoDelay => Sockoption::NoDelay,
            JournalSockoptionV1::DontRoute => Sockoption::DontRoute,
            JournalSockoptionV1::OnlyV6 => Sockoption::OnlyV6,
            JournalSockoptionV1::Broadcast => Sockoption::Broadcast,
            JournalSockoptionV1::MulticastLoopV4 => Sockoption::MulticastLoopV4,
            JournalSockoptionV1::MulticastLoopV6 => Sockoption::MulticastLoopV6,
            JournalSockoptionV1::Promiscuous => Sockoption::Promiscuous,
            JournalSockoptionV1::Listening => Sockoption::Listening,
            JournalSockoptionV1::LastError => Sockoption::LastError,
            JournalSockoptionV1::KeepAlive => Sockoption::KeepAlive,
            JournalSockoptionV1::Linger => Sockoption::Linger,
            JournalSockoptionV1::OobInline => Sockoption::OobInline,
            JournalSockoptionV1::RecvBufSize => Sockoption::RecvBufSize,
            JournalSockoptionV1::SendBufSize => Sockoption::SendBufSize,
            JournalSockoptionV1::RecvLowat => Sockoption::RecvLowat,
            JournalSockoptionV1::SendLowat => Sockoption::SendLowat,
            JournalSockoptionV1::RecvTimeout => Sockoption::RecvTimeout,
            JournalSockoptionV1::SendTimeout => Sockoption::SendTimeout,
            JournalSockoptionV1::ConnectTimeout => Sockoption::ConnectTimeout,
            JournalSockoptionV1::AcceptTimeout => Sockoption::AcceptTimeout,
            JournalSockoptionV1::Ttl => Sockoption::Ttl,
            JournalSockoptionV1::MulticastTtlV4 => Sockoption::MulticastTtlV4,
            JournalSockoptionV1::Type => Sockoption::Type,
            JournalSockoptionV1::Proto => Sockoption::Proto,
        }
    }
}

impl<'a> From<JournalEntry<'a>> for LogFileJournalEntry {
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
                id,
                call_stack: call_stack.into_owned(),
                memory_stack: memory_stack.into_owned(),
                store_data: store_data.into_owned(),
                is_64bit,
            },
            JournalEntry::CloseThread { id, exit_code } => Self::CloseThreadV1 {
                id,
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
                when,
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
                event,
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
            JournalEntry::SocketConnect { fd, addr } => Self::SocketConnectV1 { fd, addr },
            JournalEntry::SocketAccept {
                listen_fd,
                fd,
                peer_addr,
            } => Self::SocketAcceptV1 {
                listen_fd,
                fd,
                peer_addr,
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
            JournalEntry::SocketSendFile { socket_fd, file_fd } => {
                Self::SocketSendFileV1 { socket_fd, file_fd }
            }
            JournalEntry::SocketSendTo {
                fd,
                data,
                flags,
                addr,
            } => Self::SocketSendToV1 {
                fd,
                data: data.into(),
                flags: flags as u16,
                addr,
            },
            JournalEntry::SocketSend { fd, data, flags } => Self::SocketSendV1 {
                fd,
                data: data.into(),
                flags: flags as u16,
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
            JournalEntry::SocketSetOptTime { fd, opt, size } => Self::SocketSetOptTimeV1 {
                fd,
                opt: opt.into(),
                size,
            },
            JournalEntry::SocketShutdown { fd, how } => Self::SocketShutdownV1 { fd, how },
        }
    }
}

impl<'a> From<LogFileJournalEntry> for JournalEntry<'a> {
    fn from(value: LogFileJournalEntry) -> Self {
        match value {
            LogFileJournalEntry::InitModuleV1 { wasm_hash } => Self::InitModule { wasm_hash },
            LogFileJournalEntry::UpdateMemoryRegionV1 { start, end, data } => {
                Self::UpdateMemoryRegion {
                    region: start..end,
                    data: data.into(),
                }
            }
            LogFileJournalEntry::ProcessExitV1 { exit_code } => Self::ProcessExit {
                exit_code: exit_code.map(|code| code.into()),
            },
            LogFileJournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThread {
                id,
                call_stack: call_stack.into(),
                memory_stack: memory_stack.into(),
                store_data: store_data.into(),
                is_64bit,
            },
            LogFileJournalEntry::CloseThreadV1 { id, exit_code } => Self::CloseThread {
                id,
                exit_code: exit_code.map(|code| code.into()),
            },
            LogFileJournalEntry::FileDescriptorWriteV1 {
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
            LogFileJournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => {
                Self::FileDescriptorSeek {
                    fd,
                    offset,
                    whence: whence.into(),
                }
            }
            LogFileJournalEntry::OpenFileDescriptorV1 {
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
            LogFileJournalEntry::CloseFileDescriptorV1 { fd } => Self::CloseFileDescriptor { fd },
            LogFileJournalEntry::RemoveDirectoryV1 { fd, path } => Self::RemoveDirectory {
                fd,
                path: path.into(),
            },
            LogFileJournalEntry::UnlinkFileV1 { fd, path } => Self::UnlinkFile {
                fd,
                path: path.into(),
            },
            LogFileJournalEntry::PathRenameV1 {
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
            LogFileJournalEntry::SnapshotV1 { when, trigger } => Self::Snapshot {
                when,
                trigger: trigger.into(),
            },
            LogFileJournalEntry::SetClockTimeV1 { clock_id, time } => Self::SetClockTime {
                clock_id: clock_id.into(),
                time,
            },
            LogFileJournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                Self::RenumberFileDescriptor { old_fd, new_fd }
            }
            LogFileJournalEntry::DuplicateFileDescriptorV1 {
                original_fd: old_fd,
                copied_fd: new_fd,
            } => Self::DuplicateFileDescriptor {
                original_fd: old_fd,
                copied_fd: new_fd,
            },
            LogFileJournalEntry::CreateDirectoryV1 { fd, path } => Self::CreateDirectory {
                fd,
                path: path.into(),
            },
            LogFileJournalEntry::PathSetTimesV1 {
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
            LogFileJournalEntry::FileDescriptorSetTimesV1 {
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
            LogFileJournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                Self::FileDescriptorSetSize { fd, st_size }
            }
            LogFileJournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                Self::FileDescriptorSetFlags {
                    fd,
                    flags: Fdflags::from_bits_truncate(flags),
                }
            }
            LogFileJournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => Self::FileDescriptorSetRights {
                fd,
                fs_rights_base: Rights::from_bits_truncate(fs_rights_base),
                fs_rights_inheriting: Rights::from_bits_truncate(fs_rights_inheriting),
            },
            LogFileJournalEntry::FileDescriptorAdviseV1 {
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
            LogFileJournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                Self::FileDescriptorAllocate { fd, offset, len }
            }
            LogFileJournalEntry::CreateHardLinkV1 {
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
            LogFileJournalEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => Self::CreateSymbolicLink {
                old_path: old_path.into(),
                fd,
                new_path: new_path.into(),
            },
            LogFileJournalEntry::ChangeDirectoryV1 { path } => {
                Self::ChangeDirectory { path: path.into() }
            }
            LogFileJournalEntry::EpollCreateV1 { fd } => Self::EpollCreate { fd },
            LogFileJournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => Self::EpollCtl {
                epfd,
                op: op.into(),
                fd,
                event,
            },
            LogFileJournalEntry::TtySetV1 {
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
            LogFileJournalEntry::CreatePipeV1 { fd1, fd2 } => Self::CreatePipe { fd1, fd2 },
            LogFileJournalEntry::PortAddAddrV1 { cidr } => Self::PortAddAddr { cidr },
            LogFileJournalEntry::PortDelAddrV1 { addr } => Self::PortDelAddr { addr },
            LogFileJournalEntry::PortAddrClearV1 => Self::PortAddrClear,
            LogFileJournalEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => Self::PortBridge {
                network: network.into(),
                token: token.into(),
                security: security.into(),
            },
            LogFileJournalEntry::PortUnbridgeV1 => Self::PortUnbridge,
            LogFileJournalEntry::PortDhcpAcquireV1 => Self::PortDhcpAcquire,
            LogFileJournalEntry::PortGatewaySetV1 { ip } => Self::PortGatewaySet { ip },
            LogFileJournalEntry::PortRouteAddV1 {
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
            LogFileJournalEntry::PortRouteClearV1 => Self::PortRouteClear,
            LogFileJournalEntry::PortRouteDelV1 { ip } => Self::PortRouteDel { ip },
            LogFileJournalEntry::SocketOpenV1 { af, ty, pt, fd } => Self::SocketOpen {
                af: af.into(),
                ty: ty.into(),
                pt: pt.try_into().unwrap_or(wasi::SockProto::Max),
                fd,
            },
            LogFileJournalEntry::SocketListenV1 { fd, backlog } => {
                Self::SocketListen { fd, backlog }
            }
            LogFileJournalEntry::SocketBindV1 { fd, addr } => Self::SocketBind { fd, addr },
            LogFileJournalEntry::SocketConnectV1 { fd, addr } => Self::SocketConnect { fd, addr },
            LogFileJournalEntry::SocketAcceptV1 {
                listen_fd,
                fd,
                peer_addr,
            } => Self::SocketAccept {
                listen_fd,
                fd,
                peer_addr,
            },
            LogFileJournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            LogFileJournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            LogFileJournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            LogFileJournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => Self::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            LogFileJournalEntry::SocketSendFileV1 { socket_fd, file_fd } => {
                Self::SocketSendFile { socket_fd, file_fd }
            }
            LogFileJournalEntry::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
            } => Self::SocketSendTo {
                fd,
                data: data.into(),
                flags,
                addr,
            },
            LogFileJournalEntry::SocketSendV1 { fd, data, flags } => Self::SocketSend {
                fd,
                data: data.into(),
                flags,
            },
            LogFileJournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => Self::SocketSetOptFlag {
                fd,
                opt: opt.into(),
                flag,
            },
            LogFileJournalEntry::SocketSetOptSizeV1 { fd, opt, size } => Self::SocketSetOptSize {
                fd,
                opt: opt.into(),
                size,
            },
            LogFileJournalEntry::SocketSetOptTimeV1 { fd, opt, size } => Self::SocketSetOptTime {
                fd,
                opt: opt.into(),
                size,
            },
            LogFileJournalEntry::SocketShutdownV1 { fd, how } => Self::SocketShutdown { fd, how },
        }
    }
}

struct State {
    file: tokio::fs::File,
    at_end: bool,
}

/// The LogFile snapshot capturer will write its snapshots to a linear journal
/// and read them when restoring. It uses the `bincode` serializer which
/// means that forwards and backwards compatibility must be dealt with
/// carefully.
///
/// When opening an existing journal file that was previously saved
/// then new entries will be added to the end regardless of if
/// its been read.
///
/// The logfile snapshot capturer uses a 64bit number as a entry encoding
/// delimiter.
pub struct LogFileJournal {
    state: tokio::sync::Mutex<State>,
    handle: Handle,
}

impl LogFileJournal {
    pub async fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let state = State {
            file: tokio::fs::File::options()
                .read(true)
                .write(true)
                .create(true)
                .open(path)
                .await?,
            at_end: false,
        };
        Ok(Self {
            state: tokio::sync::Mutex::new(state),
            handle: Handle::current(),
        })
    }

    pub fn new_std(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let state = State {
            file: tokio::fs::File::from_std(file),
            at_end: false,
        };
        Ok(Self {
            state: tokio::sync::Mutex::new(state),
            handle: Handle::current(),
        })
    }
}

#[async_trait::async_trait]
impl Journal for LogFileJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        tracing::debug!("journal event: {:?}", entry);
        Box::pin(async {
            let entry: LogFileJournalEntry = entry.into();

            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut state = self.state.lock().await;
            if !state.at_end {
                state.file.seek(SeekFrom::End(0)).await?;
                state.at_end = true;
            }

            let data = bincode::serialize(&entry)?;
            let data_len = data.len() as u64;
            let data_len = data_len.to_ne_bytes();

            state.file.write_all(&data_len).await?;
            state.file.write_all(&data).await?;
            Ok(())
        })
    }

    /// UNSAFE: This method uses unsafe operations to remove the need to zero
    /// the buffer before its read the log entries into it
    fn read(&self) -> LocalBoxFuture<'_, anyhow::Result<Option<JournalEntry<'_>>>> {
        Box::pin(async {
            let mut state = self.state.lock().await;

            let mut data_len = [0u8; 8];
            match state.file.read_exact(&mut data_len).await {
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
                Err(err) => return Err(err.into()),
                Ok(_) => {}
            }

            let data_len = u64::from_ne_bytes(data_len);
            let mut data = Vec::with_capacity(data_len as usize);
            let data_unsafe: &mut [MaybeUninit<u8>] = data.spare_capacity_mut();
            let data_unsafe: &mut [u8] = unsafe { std::mem::transmute(data_unsafe) };
            state.file.read_exact(data_unsafe).await?;
            unsafe {
                data.set_len(data_len as usize);
            }

            let entry: LogFileJournalEntry = bincode::deserialize(&data)?;
            Ok(Some(entry.into()))
        })
    }
}
