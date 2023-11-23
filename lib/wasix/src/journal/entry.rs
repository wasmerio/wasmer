use super::base64;
use serde::{Deserialize, Serialize};
use std::net::{Shutdown, SocketAddr};
use std::time::SystemTime;
use std::{borrow::Cow, ops::Range};
use virtual_net::{Duration, IpAddr, IpCidr, Ipv4Addr, Ipv6Addr, StreamSecurity};
use wasmer_wasix_types::wasi::{
    Addressfamily, Advice, EpollCtl, EpollEventCtl, EventFdFlags, ExitCode, Fdflags, FileDelta,
    Filesize, Fstflags, LookupFlags, Oflags, Rights, SiFlags, Snapshot0Clockid, SockProto,
    Sockoption, Socktype, Timestamp, Tty, Whence,
};

use virtual_fs::Fd;

use crate::net::socket::TimeType;
use crate::WasiThreadId;

use super::SnapshotTrigger;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SocketJournalEvent {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum SocketShutdownHow {
    Read,
    Write,
    Both,
}
impl From<Shutdown> for SocketShutdownHow {
    fn from(value: Shutdown) -> Self {
        match value {
            Shutdown::Read => Self::Read,
            Shutdown::Write => Self::Write,
            Shutdown::Both => Self::Both,
        }
    }
}
impl From<SocketShutdownHow> for Shutdown {
    fn from(value: SocketShutdownHow) -> Self {
        match value {
            SocketShutdownHow::Read => Self::Read,
            SocketShutdownHow::Write => Self::Write,
            SocketShutdownHow::Both => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum SocketOptTimeType {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    BindTimeout,
    Linger,
}
impl From<TimeType> for SocketOptTimeType {
    fn from(value: TimeType) -> Self {
        match value {
            TimeType::ReadTimeout => Self::ReadTimeout,
            TimeType::WriteTimeout => Self::WriteTimeout,
            TimeType::AcceptTimeout => Self::AcceptTimeout,
            TimeType::ConnectTimeout => Self::ConnectTimeout,
            TimeType::BindTimeout => Self::BindTimeout,
            TimeType::Linger => Self::Linger,
        }
    }
}
impl From<SocketOptTimeType> for TimeType {
    fn from(value: SocketOptTimeType) -> Self {
        match value {
            SocketOptTimeType::ReadTimeout => Self::ReadTimeout,
            SocketOptTimeType::WriteTimeout => Self::WriteTimeout,
            SocketOptTimeType::AcceptTimeout => Self::AcceptTimeout,
            SocketOptTimeType::ConnectTimeout => Self::ConnectTimeout,
            SocketOptTimeType::BindTimeout => Self::BindTimeout,
            SocketOptTimeType::Linger => Self::Linger,
        }
    }
}

/// Represents a log entry in a snapshot log stream that represents the total
/// state of a WASM process at a point in time.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum JournalEntry<'a> {
    InitModule {
        wasm_hash: [u8; 32],
    },
    UpdateMemoryRegion {
        region: Range<u64>,
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
    },
    ProcessExit {
        exit_code: Option<ExitCode>,
    },
    SetThread {
        id: WasiThreadId,
        #[serde(with = "base64")]
        call_stack: Cow<'a, [u8]>,
        #[serde(with = "base64")]
        memory_stack: Cow<'a, [u8]>,
        #[serde(with = "base64")]
        store_data: Cow<'a, [u8]>,
        is_64bit: bool,
    },
    CloseThread {
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    },
    FileDescriptorSeek {
        fd: Fd,
        offset: FileDelta,
        whence: Whence,
    },
    FileDescriptorWrite {
        fd: Fd,
        offset: u64,
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        is_64bit: bool,
    },
    SetClockTime {
        clock_id: Snapshot0Clockid,
        time: Timestamp,
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
    CreateEvent {
        initial_val: u64,
        flags: EventFdFlags,
        fd: Fd,
    },
    PortAddAddr {
        cidr: IpCidr,
    },
    PortDelAddr {
        addr: IpAddr,
    },
    PortAddrClear,
    PortBridge {
        network: Cow<'a, str>,
        token: Cow<'a, str>,
        security: StreamSecurity,
    },
    PortUnbridge,
    PortDhcpAcquire,
    PortGatewaySet {
        ip: IpAddr,
    },
    PortRouteAdd {
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    },
    PortRouteClear,
    PortRouteDel {
        ip: IpAddr,
    },
    SocketOpen {
        af: Addressfamily,
        ty: Socktype,
        pt: SockProto,
        fd: Fd,
    },
    SocketListen {
        fd: Fd,
        backlog: u32,
    },
    SocketBind {
        fd: Fd,
        addr: SocketAddr,
    },
    SocketConnected {
        fd: Fd,
        addr: SocketAddr,
    },
    SocketAccepted {
        listen_fd: Fd,
        fd: Fd,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        nonblocking: bool,
    },
    SocketJoinIpv4Multicast {
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketJoinIpv6Multicast {
        fd: Fd,
        multiaddr: Ipv6Addr,
        iface: u32,
    },
    SocketLeaveIpv4Multicast {
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketLeaveIpv6Multicast {
        fd: Fd,
        multiaddr: Ipv6Addr,
        iface: u32,
    },
    SocketSendFile {
        socket_fd: Fd,
        file_fd: Fd,
        offset: Filesize,
        count: Filesize,
    },
    SocketSendTo {
        fd: Fd,
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        flags: SiFlags,
        addr: SocketAddr,
        is_64bit: bool,
    },
    SocketSend {
        fd: Fd,
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        flags: SiFlags,
        is_64bit: bool,
    },
    SocketSetOptFlag {
        fd: Fd,
        opt: Sockoption,
        flag: bool,
    },
    SocketSetOptSize {
        fd: Fd,
        opt: Sockoption,
        size: u64,
    },
    SocketSetOptTime {
        fd: Fd,
        ty: SocketOptTimeType,
        time: Option<Duration>,
    },
    SocketShutdown {
        fd: Fd,
        how: SocketShutdownHow,
    },
    /// Represents the marker for the end of a snapshot
    Snapshot {
        when: SystemTime,
        trigger: SnapshotTrigger,
    },
}

impl<'a> JournalEntry<'a> {
    pub fn into_owned(self) -> JournalEntry<'static> {
        match self {
            Self::InitModule { wasm_hash } => JournalEntry::InitModule { wasm_hash },
            Self::UpdateMemoryRegion { region, data } => JournalEntry::UpdateMemoryRegion {
                region,
                data: data.into_owned().into(),
            },
            Self::ProcessExit { exit_code } => JournalEntry::ProcessExit { exit_code },
            Self::SetThread {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => JournalEntry::SetThread {
                id,
                call_stack: call_stack.into_owned().into(),
                memory_stack: memory_stack.into_owned().into(),
                store_data: store_data.into_owned().into(),
                is_64bit,
            },
            Self::CloseThread { id, exit_code } => JournalEntry::CloseThread { id, exit_code },
            Self::FileDescriptorSeek { fd, offset, whence } => {
                JournalEntry::FileDescriptorSeek { fd, offset, whence }
            }
            Self::FileDescriptorWrite {
                fd,
                offset,
                data,
                is_64bit,
            } => JournalEntry::FileDescriptorWrite {
                fd,
                offset,
                data: data.into_owned().into(),
                is_64bit,
            },
            Self::SetClockTime { clock_id, time } => JournalEntry::SetClockTime { clock_id, time },
            Self::CloseFileDescriptor { fd } => JournalEntry::CloseFileDescriptor { fd },
            Self::OpenFileDescriptor {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => JournalEntry::OpenFileDescriptor {
                fd,
                dirfd,
                dirflags,
                path: path.into_owned().into(),
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            },
            Self::RenumberFileDescriptor { old_fd, new_fd } => {
                JournalEntry::RenumberFileDescriptor { old_fd, new_fd }
            }
            Self::DuplicateFileDescriptor {
                original_fd,
                copied_fd,
            } => JournalEntry::DuplicateFileDescriptor {
                original_fd,
                copied_fd,
            },
            Self::CreateDirectory { fd, path } => JournalEntry::CreateDirectory {
                fd,
                path: path.into_owned().into(),
            },
            Self::RemoveDirectory { fd, path } => JournalEntry::RemoveDirectory {
                fd,
                path: path.into_owned().into(),
            },
            Self::PathSetTimes {
                fd,
                flags,
                path,
                st_atim,
                st_mtim,
                fst_flags,
            } => JournalEntry::PathSetTimes {
                fd,
                flags,
                path: path.into_owned().into(),
                st_atim,
                st_mtim,
                fst_flags,
            },
            Self::FileDescriptorSetTimes {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => JournalEntry::FileDescriptorSetTimes {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            },
            Self::FileDescriptorSetFlags { fd, flags } => {
                JournalEntry::FileDescriptorSetFlags { fd, flags }
            }
            Self::FileDescriptorSetRights {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => JournalEntry::FileDescriptorSetRights {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            },
            Self::FileDescriptorSetSize { fd, st_size } => {
                JournalEntry::FileDescriptorSetSize { fd, st_size }
            }
            Self::FileDescriptorAdvise {
                fd,
                offset,
                len,
                advice,
            } => JournalEntry::FileDescriptorAdvise {
                fd,
                offset,
                len,
                advice,
            },
            Self::FileDescriptorAllocate { fd, offset, len } => {
                JournalEntry::FileDescriptorAllocate { fd, offset, len }
            }
            Self::CreateHardLink {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => JournalEntry::CreateHardLink {
                old_fd,
                old_path: old_path.into_owned().into(),
                old_flags,
                new_fd,
                new_path: new_path.into_owned().into(),
            },
            Self::CreateSymbolicLink {
                old_path,
                fd,
                new_path,
            } => JournalEntry::CreateSymbolicLink {
                old_path: old_path.into_owned().into(),
                fd,
                new_path: new_path.into_owned().into(),
            },
            Self::UnlinkFile { fd, path } => JournalEntry::UnlinkFile {
                fd,
                path: path.into_owned().into(),
            },
            Self::PathRename {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => JournalEntry::PathRename {
                old_fd,
                old_path: old_path.into_owned().into(),
                new_fd,
                new_path: new_path.into_owned().into(),
            },
            Self::ChangeDirectory { path } => JournalEntry::ChangeDirectory {
                path: path.into_owned().into(),
            },
            Self::EpollCreate { fd } => JournalEntry::EpollCreate { fd },
            Self::EpollCtl {
                epfd,
                op,
                fd,
                event,
            } => JournalEntry::EpollCtl {
                epfd,
                op,
                fd,
                event,
            },
            Self::TtySet { tty, line_feeds } => JournalEntry::TtySet { tty, line_feeds },
            Self::CreatePipe { fd1, fd2 } => JournalEntry::CreatePipe { fd1, fd2 },
            Self::CreateEvent {
                initial_val,
                flags,
                fd,
            } => JournalEntry::CreateEvent {
                initial_val,
                flags,
                fd,
            },
            Self::PortAddAddr { cidr } => JournalEntry::PortAddAddr { cidr },
            Self::PortDelAddr { addr } => JournalEntry::PortDelAddr { addr },
            Self::PortAddrClear => JournalEntry::PortAddrClear,
            Self::PortBridge {
                network,
                token,
                security,
            } => JournalEntry::PortBridge {
                network: network.into_owned().into(),
                token: token.into_owned().into(),
                security,
            },
            Self::PortUnbridge => JournalEntry::PortUnbridge,
            Self::PortDhcpAcquire => JournalEntry::PortDhcpAcquire,
            Self::PortGatewaySet { ip } => JournalEntry::PortGatewaySet { ip },
            Self::PortRouteAdd {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => JournalEntry::PortRouteAdd {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            },
            Self::PortRouteClear => JournalEntry::PortRouteClear,
            Self::PortRouteDel { ip } => JournalEntry::PortRouteDel { ip },
            Self::SocketOpen { af, ty, pt, fd } => JournalEntry::SocketOpen { af, ty, pt, fd },
            Self::SocketListen { fd, backlog } => JournalEntry::SocketListen { fd, backlog },
            Self::SocketBind { fd, addr } => JournalEntry::SocketBind { fd, addr },
            Self::SocketConnected { fd, addr } => JournalEntry::SocketConnected { fd, addr },
            Self::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            } => JournalEntry::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            },
            Self::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => JournalEntry::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            Self::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => JournalEntry::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            Self::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => JournalEntry::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            },
            Self::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => JournalEntry::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            },
            Self::SocketSendFile {
                socket_fd,
                file_fd,
                offset,
                count,
            } => JournalEntry::SocketSendFile {
                socket_fd,
                file_fd,
                offset,
                count,
            },
            Self::SocketSendTo {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => JournalEntry::SocketSendTo {
                fd,
                data: data.into_owned().into(),
                flags,
                addr,
                is_64bit,
            },
            Self::SocketSend {
                fd,
                data,
                flags,
                is_64bit,
            } => JournalEntry::SocketSend {
                fd,
                data: data.into_owned().into(),
                flags,
                is_64bit,
            },
            Self::SocketSetOptFlag { fd, opt, flag } => {
                JournalEntry::SocketSetOptFlag { fd, opt, flag }
            }
            Self::SocketSetOptSize { fd, opt, size } => {
                JournalEntry::SocketSetOptSize { fd, opt, size }
            }
            Self::SocketSetOptTime { fd, ty, time } => {
                JournalEntry::SocketSetOptTime { fd, ty, time }
            }
            Self::SocketShutdown { fd, how } => JournalEntry::SocketShutdown { fd, how },
            Self::Snapshot { when, trigger } => JournalEntry::Snapshot { when, trigger },
        }
    }

    pub fn estimate_size(&self) -> usize {
        let base_size = std::mem::size_of_val(self);
        match self {
            JournalEntry::InitModule { .. } => base_size,
            JournalEntry::UpdateMemoryRegion { data, .. } => base_size + data.len(),
            JournalEntry::ProcessExit { .. } => base_size,
            JournalEntry::SetThread {
                call_stack,
                memory_stack,
                store_data,
                ..
            } => base_size + call_stack.len() + memory_stack.len() + store_data.len(),
            JournalEntry::CloseThread { .. } => base_size,
            JournalEntry::FileDescriptorSeek { .. } => base_size,
            JournalEntry::FileDescriptorWrite { data, .. } => base_size + data.len(),
            JournalEntry::SetClockTime { .. } => base_size,
            JournalEntry::CloseFileDescriptor { .. } => base_size,
            JournalEntry::OpenFileDescriptor { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::RenumberFileDescriptor { .. } => base_size,
            JournalEntry::DuplicateFileDescriptor { .. } => base_size,
            JournalEntry::CreateDirectory { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::RemoveDirectory { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::PathSetTimes { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::FileDescriptorSetTimes { .. } => base_size,
            JournalEntry::FileDescriptorSetFlags { .. } => base_size,
            JournalEntry::FileDescriptorSetRights { .. } => base_size,
            JournalEntry::FileDescriptorSetSize { .. } => base_size,
            JournalEntry::FileDescriptorAdvise { .. } => base_size,
            JournalEntry::FileDescriptorAllocate { .. } => base_size,
            JournalEntry::CreateHardLink {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::CreateSymbolicLink {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::UnlinkFile { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::PathRename {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::ChangeDirectory { path } => base_size + path.as_bytes().len(),
            JournalEntry::EpollCreate { .. } => base_size,
            JournalEntry::EpollCtl { .. } => base_size,
            JournalEntry::TtySet { .. } => base_size,
            JournalEntry::CreatePipe { .. } => base_size,
            JournalEntry::CreateEvent { .. } => base_size,
            JournalEntry::PortAddAddr { .. } => base_size,
            JournalEntry::PortDelAddr { .. } => base_size,
            JournalEntry::PortAddrClear => base_size,
            JournalEntry::PortBridge { network, token, .. } => {
                base_size + network.as_bytes().len() + token.as_bytes().len()
            }
            JournalEntry::PortUnbridge => base_size,
            JournalEntry::PortDhcpAcquire => base_size,
            JournalEntry::PortGatewaySet { .. } => base_size,
            JournalEntry::PortRouteAdd { .. } => base_size,
            JournalEntry::PortRouteClear => base_size,
            JournalEntry::PortRouteDel { .. } => base_size,
            JournalEntry::SocketOpen { .. } => base_size,
            JournalEntry::SocketListen { .. } => base_size,
            JournalEntry::SocketBind { .. } => base_size,
            JournalEntry::SocketConnected { .. } => base_size,
            JournalEntry::SocketAccepted { .. } => base_size,
            JournalEntry::SocketJoinIpv4Multicast { .. } => base_size,
            JournalEntry::SocketJoinIpv6Multicast { .. } => base_size,
            JournalEntry::SocketLeaveIpv4Multicast { .. } => base_size,
            JournalEntry::SocketLeaveIpv6Multicast { .. } => base_size,
            JournalEntry::SocketSendFile { .. } => base_size,
            JournalEntry::SocketSendTo { data, .. } => base_size + data.len(),
            JournalEntry::SocketSend { data, .. } => base_size + data.len(),
            JournalEntry::SocketSetOptFlag { .. } => base_size,
            JournalEntry::SocketSetOptSize { .. } => base_size,
            JournalEntry::SocketSetOptTime { .. } => base_size,
            JournalEntry::SocketShutdown { .. } => base_size,
            JournalEntry::Snapshot { .. } => base_size,
        }
    }
}
