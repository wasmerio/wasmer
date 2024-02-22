use std::fmt;

use super::*;
use wasmer_wasix_types::wasi;

/// Type of printing mode to use
#[derive(Debug)]
pub enum JournalPrintingMode {
    Text,
    Json,
}
impl Default for JournalPrintingMode {
    fn default() -> Self {
        Self::Text
    }
}

/// The default for runtime is to use the unsupported journal
/// which will fail to write journal entries if one attempts to do so.
#[derive(Debug, Default)]
pub struct PrintingJournal {
    mode: JournalPrintingMode,
}

impl PrintingJournal {
    pub fn new(mode: JournalPrintingMode) -> Self {
        Self { mode }
    }
}

impl ReadableJournal for PrintingJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        Ok(None)
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::<PrintingJournal>::default())
    }
}

impl WritableJournal for PrintingJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        match self.mode {
            JournalPrintingMode::Text => println!("{}", entry),
            JournalPrintingMode::Json => {
                println!("{}", serde_json::to_string_pretty(&entry)?)
            }
        }
        Ok(LogWriteResult {
            record_start: 0,
            record_end: entry.estimate_size() as u64,
        })
    }
}

impl Journal for PrintingJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (
            Box::<PrintingJournal>::default(),
            Box::<PrintingJournal>::default(),
        )
    }
}

impl<'a> fmt::Display for JournalEntry<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalEntry::InitModuleV1 { wasm_hash } => {
                write!(f, "init-module (hash={:x?})", wasm_hash)
            }
            JournalEntry::ClearEtherealV1 => {
                write!(f, "clear-ethereal")
            }
            JournalEntry::UpdateMemoryRegionV1 { region, data } => write!(
                f,
                "memory-update (start={}, end={}, data.len={})",
                region.start,
                region.end,
                data.len()
            ),
            JournalEntry::ProcessExitV1 { exit_code } => {
                write!(f, "process-exit (code={:?})", exit_code)
            }
            JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                ..
            } => write!(
                f,
                "thread-update (id={}, call-stack.len={}, mem-stack.len={}, store-size={}",
                id,
                call_stack.len(),
                memory_stack.len(),
                store_data.len(),
            ),
            JournalEntry::CloseThreadV1 { id, exit_code } => {
                write!(f, "thread-close (id={}, code={:?})", id, exit_code)
            }
            JournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => write!(
                f,
                "fd-seek (fd={}, offset={}, whence={:?})",
                fd, offset, whence
            ),
            JournalEntry::FileDescriptorWriteV1 {
                fd, offset, data, ..
            } => write!(
                f,
                "fd-write (fd={}, offset={}, data.len={})",
                fd,
                offset,
                data.len()
            ),
            JournalEntry::SetClockTimeV1 { clock_id, time } => {
                write!(f, "set-clock-time (id={:?}, time={})", clock_id, time)
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => write!(f, "fd-close (fd={})", fd),
            JournalEntry::OpenFileDescriptorV1 {
                fd, path, o_flags, ..
            } => {
                if o_flags.contains(wasi::Oflags::CREATE) {
                    if o_flags.contains(wasi::Oflags::TRUNC) {
                        write!(f, "fd-create-new (fd={}, path={})", fd, path)
                    } else {
                        write!(f, "fd-create (fd={}, path={})", fd, path)
                    }
                } else if o_flags.contains(wasi::Oflags::TRUNC) {
                    write!(f, "fd-open-new (fd={}, path={})", fd, path)
                } else {
                    write!(f, "fd-open (fd={}, path={})", fd, path)
                }
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                write!(f, "fd-renumber (old={}, new={})", old_fd, new_fd)
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => write!(
                f,
                "fd-duplicate (original={}, copied={})",
                original_fd, copied_fd
            ),
            JournalEntry::CreateDirectoryV1 { fd, path } => {
                write!(f, "path-create-dir (fd={}, path={})", fd, path)
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                write!(f, "path-remove-dir (fd={}, path={})", fd, path)
            }
            JournalEntry::PathSetTimesV1 {
                path,
                st_atim,
                st_mtim,
                ..
            } => write!(
                f,
                "path-set-times (path={}, atime={}, mtime={}))",
                path, st_atim, st_mtim
            ),
            JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                ..
            } => write!(
                f,
                "fd-set-times (fd={}, atime={}, mtime={})",
                fd, st_atim, st_mtim
            ),
            JournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                write!(f, "fd-set-flags (fd={}, flags={:?})", fd, flags)
            }
            JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => write!(
                f,
                "fd-set-rights (fd={}, base={:?}, inherited={:?})",
                fd, fs_rights_base, fs_rights_inheriting
            ),
            JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                write!(f, "fd-set-size (fd={}, size={})", fd, st_size)
            }
            JournalEntry::FileDescriptorAdviseV1 {
                fd, offset, len, ..
            } => write!(f, "fd-advise (fd={}, offset={}, len={})", fd, offset, len),
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                write!(f, "fd-allocate (fd={}, offset={}, len={})", fd, offset, len)
            }
            JournalEntry::CreateHardLinkV1 {
                old_path, new_path, ..
            } => write!(f, "path-link (from={}, to={})", old_path, new_path),
            JournalEntry::CreateSymbolicLinkV1 {
                old_path, new_path, ..
            } => write!(f, "path-symlink (from={}, to={})", old_path, new_path),
            JournalEntry::UnlinkFileV1 { path, .. } => write!(f, "path-unlink (path={})", path),
            JournalEntry::PathRenameV1 {
                old_path, new_path, ..
            } => write!(
                f,
                "path-rename (old-path={}, new-path={})",
                old_path, new_path
            ),
            JournalEntry::ChangeDirectoryV1 { path } => write!(f, "chdir (path={})", path),
            JournalEntry::EpollCreateV1 { fd } => write!(f, "epoll-create (fd={})", fd),
            JournalEntry::EpollCtlV1 { epfd, op, fd, .. } => {
                write!(f, "epoll-ctl (epfd={}, op={:?}, fd={})", epfd, op, fd)
            }
            JournalEntry::TtySetV1 { tty, line_feeds } => write!(
                f,
                "tty-set (echo={}, buffering={}, feeds={})",
                tty.echo, tty.line_buffered, line_feeds
            ),
            JournalEntry::CreatePipeV1 { fd1, fd2 } => {
                write!(f, "fd-pipe (fd1={}, fd2={})", fd1, fd2)
            }
            JournalEntry::CreateEventV1 {
                initial_val, fd, ..
            } => write!(f, "fd-event (fd={}, initial={})", fd, initial_val),
            JournalEntry::PortAddAddrV1 { cidr } => {
                write!(f, "port-addr-add (ip={}, prefix={})", cidr.ip, cidr.prefix)
            }
            JournalEntry::PortDelAddrV1 { addr } => write!(f, "port-addr-del (addr={})", addr),
            JournalEntry::PortAddrClearV1 => write!(f, "port-addr-clear"),
            JournalEntry::PortBridgeV1 { network, .. } => {
                write!(f, "port-bridge (network={})", network)
            }
            JournalEntry::PortUnbridgeV1 => write!(f, "port-unbridge"),
            JournalEntry::PortDhcpAcquireV1 => write!(f, "port-dhcp-acquire"),
            JournalEntry::PortGatewaySetV1 { ip } => write!(f, "port-gateway-set (ip={})", ip),
            JournalEntry::PortRouteAddV1 {
                cidr, via_router, ..
            } => write!(
                f,
                "port-route-add (ip={}, prefix={}, via_router={})",
                cidr.ip, cidr.prefix, via_router
            ),
            JournalEntry::PortRouteClearV1 => write!(f, "port-route-clear"),
            JournalEntry::PortRouteDelV1 { ip } => write!(f, "port-route-del (ip={})", ip),
            JournalEntry::SocketOpenV1 { af, ty, pt, fd } => {
                write!(
                    f,
                    "sock-open (fd={}, af={:?}, ty={:?}, pt={:?})",
                    fd, af, ty, pt
                )
            }
            JournalEntry::SocketListenV1 { fd, backlog } => {
                write!(f, "sock-listen (fd={}, backlog={})", fd, backlog)
            }
            JournalEntry::SocketBindV1 { fd, addr } => {
                write!(f, "sock-bind (fd={}, addr={})", fd, addr)
            }
            JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => {
                write!(
                    f,
                    "sock-connect (fd={}, addr={}, peer={})",
                    fd, local_addr, peer_addr
                )
            }
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr,
                peer_addr,
                ..
            } => write!(
                f,
                "sock-accept (listen-fd={}, sock_fd={}, addr={}, peer={})",
                listen_fd, fd, local_addr, peer_addr
            ),
            JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => write!(
                f,
                "sock-join-mcast-ipv4 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(
                f,
                "sock-join-mcast-ipv6 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(
                f,
                "sock-leave-mcast-ipv4 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(
                f,
                "sock-leave-mcast-ipv6 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => write!(
                f,
                "sock-send-file (sock-fd={}, file-fd={}, offset={}, count={})",
                socket_fd, file_fd, offset, count
            ),
            JournalEntry::SocketSendToV1 { fd, data, addr, .. } => write!(
                f,
                "sock-send-to (fd={}, data.len={}, addr={})",
                fd,
                data.len(),
                addr
            ),
            JournalEntry::SocketSendV1 { fd, data, .. } => {
                write!(f, "sock-send (fd={}, data.len={}", fd, data.len())
            }
            JournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, flag={})", fd, opt, flag)
            }
            JournalEntry::SocketSetOptSizeV1 { fd, opt, size } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, size={})", fd, opt, size)
            }
            JournalEntry::SocketSetOptTimeV1 { fd, ty, time } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, time={:?})", fd, ty, time)
            }
            JournalEntry::SocketShutdownV1 { fd, how } => {
                write!(f, "sock-shutdown (fd={}, how={:?})", fd, how)
            }
            JournalEntry::SnapshotV1 { when, trigger } => {
                write!(f, "snapshot (when={:?}, trigger={:?})", when, trigger)
            }
        }
    }
}
