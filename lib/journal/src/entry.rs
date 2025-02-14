use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::net::{Shutdown, SocketAddr};
use std::time::{Duration, SystemTime};
use std::{borrow::Cow, ops::Range};
use virtual_net::{IpCidr, StreamSecurity};
use wasmer_wasix_types::wasi::{
    Addressfamily, Advice, EpollCtl, EpollEventCtl, EventFdFlags, ExitCode, Fdflags, Fdflagsext,
    FileDelta, Filesize, Fstflags, LookupFlags, Oflags, Rights, SiFlags, Snapshot0Clockid,
    SockProto, Sockoption, Socktype, Timestamp, Tty, Whence,
};
use wasmer_wasix_types::wasix::{ThreadStartType, WasiMemoryLayout};

use crate::{base64, SnapshotTrigger};

type Fd = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub enum SocketOptTimeType {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    BindTimeout,
    Linger,
}

/// Represents a log entry in a snapshot log stream that represents the total
/// state of a WASM process at a point in time.
#[allow(clippy::large_enum_variant)]
#[derive(derive_more::Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalEntry<'a> {
    InitModuleV1 {
        wasm_hash: Box<[u8]>,
    },
    ClearEtherealV1,
    UpdateMemoryRegionV1 {
        region: Range<u64>,
        #[debug(ignore)]
        #[serde(with = "base64")]
        compressed_data: Cow<'a, [u8]>,
    },
    ProcessExitV1 {
        exit_code: Option<ExitCode>,
    },
    SetThreadV1 {
        id: u32,
        #[debug(ignore)]
        #[serde(with = "base64")]
        call_stack: Cow<'a, [u8]>,
        #[debug(ignore)]
        #[serde(with = "base64")]
        memory_stack: Cow<'a, [u8]>,
        #[debug(ignore)]
        #[serde(with = "base64")]
        store_data: Cow<'a, [u8]>,
        start: ThreadStartType,
        layout: WasiMemoryLayout,
        is_64bit: bool,
    },
    CloseThreadV1 {
        id: u32,
        exit_code: Option<ExitCode>,
    },
    FileDescriptorSeekV1 {
        fd: Fd,
        offset: FileDelta,
        whence: Whence,
    },
    FileDescriptorWriteV1 {
        fd: Fd,
        offset: u64,
        #[debug(ignore)]
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        is_64bit: bool,
    },
    SetClockTimeV1 {
        clock_id: Snapshot0Clockid,
        time: Timestamp,
    },
    CloseFileDescriptorV1 {
        fd: Fd,
    },
    OpenFileDescriptorV1 {
        fd: Fd,
        dirfd: Fd,
        dirflags: LookupFlags,
        path: Cow<'a, str>,
        o_flags: Oflags,
        #[debug(ignore)]
        fs_rights_base: Rights,
        #[debug(ignore)]
        fs_rights_inheriting: Rights,
        fs_flags: Fdflags,
    },
    OpenFileDescriptorV2 {
        fd: Fd,
        dirfd: Fd,
        dirflags: LookupFlags,
        path: Cow<'a, str>,
        o_flags: Oflags,
        #[debug(ignore)]
        fs_rights_base: Rights,
        #[debug(ignore)]
        fs_rights_inheriting: Rights,
        fs_flags: Fdflags,
        fd_flags: Fdflagsext,
    },
    RenumberFileDescriptorV1 {
        old_fd: Fd,
        new_fd: Fd,
    },
    DuplicateFileDescriptorV1 {
        original_fd: Fd,
        copied_fd: Fd,
    },
    DuplicateFileDescriptorV2 {
        original_fd: Fd,
        copied_fd: Fd,
        cloexec: bool,
    },
    CreateDirectoryV1 {
        fd: Fd,
        path: Cow<'a, str>,
    },
    RemoveDirectoryV1 {
        fd: Fd,
        path: Cow<'a, str>,
    },
    PathSetTimesV1 {
        fd: Fd,
        flags: LookupFlags,
        path: Cow<'a, str>,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    },
    FileDescriptorSetTimesV1 {
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    },
    FileDescriptorSetFdFlagsV1 {
        fd: Fd,
        flags: Fdflagsext,
    },
    FileDescriptorSetFlagsV1 {
        fd: Fd,
        flags: Fdflags,
    },
    FileDescriptorSetRightsV1 {
        fd: Fd,
        fs_rights_base: Rights,
        fs_rights_inheriting: Rights,
    },
    FileDescriptorSetSizeV1 {
        fd: Fd,
        st_size: Filesize,
    },
    FileDescriptorAdviseV1 {
        fd: Fd,
        offset: Filesize,
        len: Filesize,
        advice: Advice,
    },
    FileDescriptorAllocateV1 {
        fd: Fd,
        offset: Filesize,
        len: Filesize,
    },
    CreateHardLinkV1 {
        old_fd: Fd,
        old_path: Cow<'a, str>,
        old_flags: LookupFlags,
        new_fd: Fd,
        new_path: Cow<'a, str>,
    },
    CreateSymbolicLinkV1 {
        old_path: Cow<'a, str>,
        fd: Fd,
        new_path: Cow<'a, str>,
    },
    UnlinkFileV1 {
        fd: Fd,
        path: Cow<'a, str>,
    },
    PathRenameV1 {
        old_fd: Fd,
        old_path: Cow<'a, str>,
        new_fd: Fd,
        new_path: Cow<'a, str>,
    },
    ChangeDirectoryV1 {
        path: Cow<'a, str>,
    },
    EpollCreateV1 {
        fd: Fd,
    },
    EpollCtlV1 {
        epfd: Fd,
        op: EpollCtl,
        fd: Fd,
        event: Option<EpollEventCtl>,
    },
    TtySetV1 {
        tty: Tty,
        line_feeds: bool,
    },
    CreatePipeV1 {
        read_fd: Fd,
        write_fd: Fd,
    },
    CreateEventV1 {
        initial_val: u64,
        flags: EventFdFlags,
        fd: Fd,
    },
    PortAddAddrV1 {
        cidr: IpCidr,
    },
    PortDelAddrV1 {
        addr: IpAddr,
    },
    PortAddrClearV1,
    PortBridgeV1 {
        network: Cow<'a, str>,
        token: Cow<'a, str>,
        security: StreamSecurity,
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
        af: Addressfamily,
        ty: Socktype,
        pt: SockProto,
        fd: Fd,
    },
    SocketPairV1 {
        fd1: Fd,
        fd2: Fd,
    },
    SocketListenV1 {
        fd: Fd,
        backlog: u32,
    },
    SocketBindV1 {
        fd: Fd,
        addr: SocketAddr,
    },
    SocketConnectedV1 {
        fd: Fd,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    },
    SocketAcceptedV1 {
        listen_fd: Fd,
        fd: Fd,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        non_blocking: bool,
    },
    SocketJoinIpv4MulticastV1 {
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketJoinIpv6MulticastV1 {
        fd: Fd,
        multi_addr: Ipv6Addr,
        iface: u32,
    },
    SocketLeaveIpv4MulticastV1 {
        fd: Fd,
        multi_addr: Ipv4Addr,
        iface: Ipv4Addr,
    },
    SocketLeaveIpv6MulticastV1 {
        fd: Fd,
        multi_addr: Ipv6Addr,
        iface: u32,
    },
    SocketSendFileV1 {
        socket_fd: Fd,
        file_fd: Fd,
        offset: Filesize,
        count: Filesize,
    },
    SocketSendToV1 {
        fd: Fd,
        #[debug(ignore)]
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        flags: SiFlags,
        addr: SocketAddr,
        is_64bit: bool,
    },
    SocketSendV1 {
        fd: Fd,
        #[debug(ignore)]
        #[serde(with = "base64")]
        data: Cow<'a, [u8]>,
        flags: SiFlags,
        is_64bit: bool,
    },
    SocketSetOptFlagV1 {
        fd: Fd,
        opt: Sockoption,
        flag: bool,
    },
    SocketSetOptSizeV1 {
        fd: Fd,
        opt: Sockoption,
        size: u64,
    },
    SocketSetOptTimeV1 {
        fd: Fd,
        ty: SocketOptTimeType,
        time: Option<Duration>,
    },
    SocketShutdownV1 {
        fd: Fd,
        how: SocketShutdownHow,
    },
    /// Represents the marker for the end of a snapshot
    SnapshotV1 {
        when: SystemTime,
        trigger: SnapshotTrigger,
    },
}

impl<'a> JournalEntry<'a> {
    pub fn into_owned(self) -> JournalEntry<'static> {
        match self {
            Self::InitModuleV1 { wasm_hash } => JournalEntry::InitModuleV1 { wasm_hash },
            Self::ClearEtherealV1 => JournalEntry::ClearEtherealV1,
            Self::UpdateMemoryRegionV1 {
                region,
                compressed_data,
            } => JournalEntry::UpdateMemoryRegionV1 {
                region,
                compressed_data: compressed_data.into_owned().into(),
            },
            Self::ProcessExitV1 { exit_code } => JournalEntry::ProcessExitV1 { exit_code },
            Self::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
                start,
                layout,
            } => JournalEntry::SetThreadV1 {
                id,
                call_stack: call_stack.into_owned().into(),
                memory_stack: memory_stack.into_owned().into(),
                store_data: store_data.into_owned().into(),
                start,
                layout,
                is_64bit,
            },
            Self::CloseThreadV1 { id, exit_code } => JournalEntry::CloseThreadV1 { id, exit_code },
            Self::FileDescriptorSeekV1 { fd, offset, whence } => {
                JournalEntry::FileDescriptorSeekV1 { fd, offset, whence }
            }
            Self::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data: data.into_owned().into(),
                is_64bit,
            },
            Self::SetClockTimeV1 { clock_id, time } => {
                JournalEntry::SetClockTimeV1 { clock_id, time }
            }
            Self::CloseFileDescriptorV1 { fd } => JournalEntry::CloseFileDescriptorV1 { fd },
            Self::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => JournalEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path: path.into_owned().into(),
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            },
            Self::OpenFileDescriptorV2 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
                fd_flags,
            } => JournalEntry::OpenFileDescriptorV2 {
                fd,
                dirfd,
                dirflags,
                path: path.into_owned().into(),
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
                fd_flags,
            },
            Self::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd }
            }
            Self::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            },
            Self::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                cloexec,
            } => JournalEntry::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                cloexec,
            },
            Self::CreateDirectoryV1 { fd, path } => JournalEntry::CreateDirectoryV1 {
                fd,
                path: path.into_owned().into(),
            },
            Self::RemoveDirectoryV1 { fd, path } => JournalEntry::RemoveDirectoryV1 {
                fd,
                path: path.into_owned().into(),
            },
            Self::PathSetTimesV1 {
                fd,
                flags,
                path,
                st_atim,
                st_mtim,
                fst_flags,
            } => JournalEntry::PathSetTimesV1 {
                fd,
                flags,
                path: path.into_owned().into(),
                st_atim,
                st_mtim,
                fst_flags,
            },
            Self::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            },
            Self::FileDescriptorSetFdFlagsV1 { fd, flags } => {
                JournalEntry::FileDescriptorSetFdFlagsV1 { fd, flags }
            }
            Self::FileDescriptorSetFlagsV1 { fd, flags } => {
                JournalEntry::FileDescriptorSetFlagsV1 { fd, flags }
            }
            Self::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            },
            Self::FileDescriptorSetSizeV1 { fd, st_size } => {
                JournalEntry::FileDescriptorSetSizeV1 { fd, st_size }
            }
            Self::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => JournalEntry::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            },
            Self::FileDescriptorAllocateV1 { fd, offset, len } => {
                JournalEntry::FileDescriptorAllocateV1 { fd, offset, len }
            }
            Self::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => JournalEntry::CreateHardLinkV1 {
                old_fd,
                old_path: old_path.into_owned().into(),
                old_flags,
                new_fd,
                new_path: new_path.into_owned().into(),
            },
            Self::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => JournalEntry::CreateSymbolicLinkV1 {
                old_path: old_path.into_owned().into(),
                fd,
                new_path: new_path.into_owned().into(),
            },
            Self::UnlinkFileV1 { fd, path } => JournalEntry::UnlinkFileV1 {
                fd,
                path: path.into_owned().into(),
            },
            Self::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => JournalEntry::PathRenameV1 {
                old_fd,
                old_path: old_path.into_owned().into(),
                new_fd,
                new_path: new_path.into_owned().into(),
            },
            Self::ChangeDirectoryV1 { path } => JournalEntry::ChangeDirectoryV1 {
                path: path.into_owned().into(),
            },
            Self::EpollCreateV1 { fd } => JournalEntry::EpollCreateV1 { fd },
            Self::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            },
            Self::TtySetV1 { tty, line_feeds } => JournalEntry::TtySetV1 { tty, line_feeds },
            Self::CreatePipeV1 { read_fd, write_fd } => {
                JournalEntry::CreatePipeV1 { read_fd, write_fd }
            }
            Self::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => JournalEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            },
            Self::PortAddAddrV1 { cidr } => JournalEntry::PortAddAddrV1 { cidr },
            Self::PortDelAddrV1 { addr } => JournalEntry::PortDelAddrV1 { addr },
            Self::PortAddrClearV1 => JournalEntry::PortAddrClearV1,
            Self::PortBridgeV1 {
                network,
                token,
                security,
            } => JournalEntry::PortBridgeV1 {
                network: network.into_owned().into(),
                token: token.into_owned().into(),
                security,
            },
            Self::PortUnbridgeV1 => JournalEntry::PortUnbridgeV1,
            Self::PortDhcpAcquireV1 => JournalEntry::PortDhcpAcquireV1,
            Self::PortGatewaySetV1 { ip } => JournalEntry::PortGatewaySetV1 { ip },
            Self::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            },
            Self::PortRouteClearV1 => JournalEntry::PortRouteClearV1,
            Self::PortRouteDelV1 { ip } => JournalEntry::PortRouteDelV1 { ip },
            Self::SocketOpenV1 { af, ty, pt, fd } => JournalEntry::SocketOpenV1 { af, ty, pt, fd },
            Self::SocketPairV1 { fd1, fd2 } => JournalEntry::SocketPairV1 { fd1, fd2 },
            Self::SocketListenV1 { fd, backlog } => JournalEntry::SocketListenV1 { fd, backlog },
            Self::SocketBindV1 { fd, addr } => JournalEntry::SocketBindV1 { fd, addr },
            Self::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            },
            Self::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            } => JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            },
            Self::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
            Self::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            },
            Self::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            },
            Self::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            },
            Self::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            },
            Self::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => JournalEntry::SocketSendToV1 {
                fd,
                data: data.into_owned().into(),
                flags,
                addr,
                is_64bit,
            },
            Self::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => JournalEntry::SocketSendV1 {
                fd,
                data: data.into_owned().into(),
                flags,
                is_64bit,
            },
            Self::SocketSetOptFlagV1 { fd, opt, flag } => {
                JournalEntry::SocketSetOptFlagV1 { fd, opt, flag }
            }
            Self::SocketSetOptSizeV1 { fd, opt, size } => {
                JournalEntry::SocketSetOptSizeV1 { fd, opt, size }
            }
            Self::SocketSetOptTimeV1 { fd, ty, time } => {
                JournalEntry::SocketSetOptTimeV1 { fd, ty, time }
            }
            Self::SocketShutdownV1 { fd, how } => JournalEntry::SocketShutdownV1 { fd, how },
            Self::SnapshotV1 { when, trigger } => JournalEntry::SnapshotV1 { when, trigger },
        }
    }

    pub fn estimate_size(&self) -> usize {
        let base_size = std::mem::size_of_val(self);
        match self {
            JournalEntry::InitModuleV1 { .. } => base_size,
            JournalEntry::ClearEtherealV1 => base_size,
            JournalEntry::UpdateMemoryRegionV1 {
                compressed_data, ..
            } => base_size + compressed_data.len(),
            JournalEntry::ProcessExitV1 { .. } => base_size,
            JournalEntry::SetThreadV1 {
                call_stack,
                memory_stack,
                store_data,
                ..
            } => base_size + call_stack.len() + memory_stack.len() + store_data.len(),
            JournalEntry::CloseThreadV1 { .. } => base_size,
            JournalEntry::FileDescriptorSeekV1 { .. } => base_size,
            JournalEntry::FileDescriptorWriteV1 { data, .. } => base_size + data.len(),
            JournalEntry::SetClockTimeV1 { .. } => base_size,
            JournalEntry::CloseFileDescriptorV1 { .. } => base_size,
            JournalEntry::OpenFileDescriptorV1 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::OpenFileDescriptorV2 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::RenumberFileDescriptorV1 { .. } => base_size,
            JournalEntry::DuplicateFileDescriptorV1 { .. } => base_size,
            JournalEntry::DuplicateFileDescriptorV2 { .. } => base_size,
            JournalEntry::CreateDirectoryV1 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::RemoveDirectoryV1 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::PathSetTimesV1 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::FileDescriptorSetTimesV1 { .. } => base_size,
            JournalEntry::FileDescriptorSetFdFlagsV1 { .. } => base_size,
            JournalEntry::FileDescriptorSetFlagsV1 { .. } => base_size,
            JournalEntry::FileDescriptorSetRightsV1 { .. } => base_size,
            JournalEntry::FileDescriptorSetSizeV1 { .. } => base_size,
            JournalEntry::FileDescriptorAdviseV1 { .. } => base_size,
            JournalEntry::FileDescriptorAllocateV1 { .. } => base_size,
            JournalEntry::CreateHardLinkV1 {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::CreateSymbolicLinkV1 {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::UnlinkFileV1 { path, .. } => base_size + path.as_bytes().len(),
            JournalEntry::PathRenameV1 {
                old_path, new_path, ..
            } => base_size + old_path.as_bytes().len() + new_path.as_bytes().len(),
            JournalEntry::ChangeDirectoryV1 { path } => base_size + path.as_bytes().len(),
            JournalEntry::EpollCreateV1 { .. } => base_size,
            JournalEntry::EpollCtlV1 { .. } => base_size,
            JournalEntry::TtySetV1 { .. } => base_size,
            JournalEntry::CreatePipeV1 { .. } => base_size,
            JournalEntry::CreateEventV1 { .. } => base_size,
            JournalEntry::PortAddAddrV1 { .. } => base_size,
            JournalEntry::PortDelAddrV1 { .. } => base_size,
            JournalEntry::PortAddrClearV1 => base_size,
            JournalEntry::PortBridgeV1 { network, token, .. } => {
                base_size + network.as_bytes().len() + token.as_bytes().len()
            }
            JournalEntry::PortUnbridgeV1 => base_size,
            JournalEntry::PortDhcpAcquireV1 => base_size,
            JournalEntry::PortGatewaySetV1 { .. } => base_size,
            JournalEntry::PortRouteAddV1 { .. } => base_size,
            JournalEntry::PortRouteClearV1 => base_size,
            JournalEntry::PortRouteDelV1 { .. } => base_size,
            JournalEntry::SocketOpenV1 { .. } => base_size,
            JournalEntry::SocketPairV1 { .. } => base_size,
            JournalEntry::SocketListenV1 { .. } => base_size,
            JournalEntry::SocketBindV1 { .. } => base_size,
            JournalEntry::SocketConnectedV1 { .. } => base_size,
            JournalEntry::SocketAcceptedV1 { .. } => base_size,
            JournalEntry::SocketJoinIpv4MulticastV1 { .. } => base_size,
            JournalEntry::SocketJoinIpv6MulticastV1 { .. } => base_size,
            JournalEntry::SocketLeaveIpv4MulticastV1 { .. } => base_size,
            JournalEntry::SocketLeaveIpv6MulticastV1 { .. } => base_size,
            JournalEntry::SocketSendFileV1 { .. } => base_size,
            JournalEntry::SocketSendToV1 { data, .. } => base_size + data.len(),
            JournalEntry::SocketSendV1 { data, .. } => base_size + data.len(),
            JournalEntry::SocketSetOptFlagV1 { .. } => base_size,
            JournalEntry::SocketSetOptSizeV1 { .. } => base_size,
            JournalEntry::SocketSetOptTimeV1 { .. } => base_size,
            JournalEntry::SocketShutdownV1 { .. } => base_size,
            JournalEntry::SnapshotV1 { .. } => base_size,
        }
    }
}
