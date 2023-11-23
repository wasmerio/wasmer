use std::fmt;

use super::*;

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
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        Ok(None)
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::<PrintingJournal>::default())
    }
}

impl WritableJournal for PrintingJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        match self.mode {
            JournalPrintingMode::Text => println!("{}", entry),
            JournalPrintingMode::Json => {
                println!("{}", serde_json::to_string_pretty(&entry)?)
            }
        }
        Ok(entry.estimate_size() as u64)
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
            JournalEntry::InitModule { wasm_hash } => {
                write!(f, "init-module (hash={:x?})", wasm_hash)
            }
            JournalEntry::UpdateMemoryRegion { region, data } => write!(
                f,
                "memory-update (start={}, end={}, data.len={})",
                region.start,
                region.end,
                data.len()
            ),
            JournalEntry::ProcessExit { exit_code } => {
                write!(f, "process-exit (code={:?})", exit_code)
            }
            JournalEntry::SetThread {
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
            JournalEntry::CloseThread { id, exit_code } => {
                write!(f, "thread-close (id={}, code={:?})", id, exit_code)
            }
            JournalEntry::FileDescriptorSeek { fd, offset, whence } => write!(
                f,
                "fd-seek (fd={}, offset={}, whence={:?})",
                fd, offset, whence
            ),
            JournalEntry::FileDescriptorWrite {
                fd, offset, data, ..
            } => write!(
                f,
                "fd-write (fd={}, offset={}, data.len={})",
                fd,
                offset,
                data.len()
            ),
            JournalEntry::SetClockTime { clock_id, time } => {
                write!(f, "set-clock-time (id={:?}, time={})", clock_id, time)
            }
            JournalEntry::CloseFileDescriptor { fd } => write!(f, "fd-close (fd={})", fd),
            JournalEntry::OpenFileDescriptor { fd, path, .. } => {
                write!(f, "fd-open (path={}, fd={})", fd, path)
            }
            JournalEntry::RenumberFileDescriptor { old_fd, new_fd } => {
                write!(f, "fd-renumber (old={}, new={})", old_fd, new_fd)
            }
            JournalEntry::DuplicateFileDescriptor {
                original_fd,
                copied_fd,
            } => write!(
                f,
                "fd-duplicate (original={}, copied={})",
                original_fd, copied_fd
            ),
            JournalEntry::CreateDirectory { path, .. } => {
                write!(f, "path-create-dir (path={})", path)
            }
            JournalEntry::RemoveDirectory { path, .. } => {
                write!(f, "path-remove-dir (path={})", path)
            }
            JournalEntry::PathSetTimes {
                path,
                st_atim,
                st_mtim,
                ..
            } => write!(
                f,
                "path-set-times (path={}, atime={}, mtime={}))",
                path, st_atim, st_mtim
            ),
            JournalEntry::FileDescriptorSetTimes {
                fd,
                st_atim,
                st_mtim,
                ..
            } => write!(
                f,
                "fd-set-times (fd={}, atime={}, mtime={})",
                fd, st_atim, st_mtim
            ),
            JournalEntry::FileDescriptorSetFlags { fd, flags } => {
                write!(f, "fd-set-flags (fd={}, flags={:?})", fd, flags)
            }
            JournalEntry::FileDescriptorSetRights {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => write!(
                f,
                "fd-set-rights (fd={}, base={:?}, inherited={:?})",
                fd, fs_rights_base, fs_rights_inheriting
            ),
            JournalEntry::FileDescriptorSetSize { fd, st_size } => {
                write!(f, "fd-set-size (fd={}, size={})", fd, st_size)
            }
            JournalEntry::FileDescriptorAdvise {
                fd, offset, len, ..
            } => write!(f, "fd-advise (fd={}, offset={}, len={})", fd, offset, len),
            JournalEntry::FileDescriptorAllocate { fd, offset, len } => {
                write!(f, "fd-allocate (fd={}, offset={}, len={})", fd, offset, len)
            }
            JournalEntry::CreateHardLink {
                old_path, new_path, ..
            } => write!(f, "path-link (from={}, to={})", old_path, new_path),
            JournalEntry::CreateSymbolicLink {
                old_path, new_path, ..
            } => write!(f, "path-symlink (from={}, to={})", old_path, new_path),
            JournalEntry::UnlinkFile { path, .. } => write!(f, "path-unlink (path={})", path),
            JournalEntry::PathRename {
                old_path, new_path, ..
            } => write!(
                f,
                "path-rename (old-path={}, new-path={})",
                old_path, new_path
            ),
            JournalEntry::ChangeDirectory { path } => write!(f, "chdir (path={})", path),
            JournalEntry::EpollCreate { fd } => write!(f, "epoll-create (fd={})", fd),
            JournalEntry::EpollCtl { epfd, op, fd, .. } => {
                write!(f, "epoll-ctl (epfd={}, op={:?}, fd={})", epfd, op, fd)
            }
            JournalEntry::TtySet { tty, line_feeds } => write!(
                f,
                "tty-set (echo={}, buffering={}, feeds={})",
                tty.echo, tty.line_buffered, line_feeds
            ),
            JournalEntry::CreatePipe { fd1, fd2 } => {
                write!(f, "fd-pipe (fd1={}, fd2={})", fd1, fd2)
            }
            JournalEntry::CreateEvent {
                initial_val, fd, ..
            } => write!(f, "fd-event (fd={}, initial={})", fd, initial_val),
            JournalEntry::PortAddAddr { cidr } => {
                write!(f, "port-addr-add (ip={}, prefix={})", cidr.ip, cidr.prefix)
            }
            JournalEntry::PortDelAddr { addr } => write!(f, "port-addr-del (addr={})", addr),
            JournalEntry::PortAddrClear => write!(f, "port-addr-clear"),
            JournalEntry::PortBridge { network, .. } => {
                write!(f, "port-bridge (network={})", network)
            }
            JournalEntry::PortUnbridge => write!(f, "port-unbridge"),
            JournalEntry::PortDhcpAcquire => write!(f, "port-dhcp-acquire"),
            JournalEntry::PortGatewaySet { ip } => write!(f, "port-gateway-set (ip={})", ip),
            JournalEntry::PortRouteAdd {
                cidr, via_router, ..
            } => write!(
                f,
                "port-route-add (ip={}, prefix={}, via_router={})",
                cidr.ip, cidr.prefix, via_router
            ),
            JournalEntry::PortRouteClear => write!(f, "port-route-clear"),
            JournalEntry::PortRouteDel { ip } => write!(f, "port-route-del (ip={})", ip),
            JournalEntry::SocketOpen { af, ty, pt, fd } => {
                write!(
                    f,
                    "sock-open (fd={}, af={:?}, ty={:?}, pt={:?})",
                    fd, af, ty, pt
                )
            }
            JournalEntry::SocketListen { fd, backlog } => {
                write!(f, "sock-listen (fd={}, backlog={})", fd, backlog)
            }
            JournalEntry::SocketBind { fd, addr } => {
                write!(f, "sock-bind (fd={}, addr={})", fd, addr)
            }
            JournalEntry::SocketConnected { fd, addr } => {
                write!(f, "sock-connect (fd={}, addr={})", fd, addr)
            }
            JournalEntry::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                ..
            } => write!(
                f,
                "sock-accept (listen-fd={}, sock_fd={}, peer={})",
                listen_fd, fd, peer_addr
            ),
            JournalEntry::SocketJoinIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => write!(
                f,
                "sock-join-mcast-ipv4 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketJoinIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => write!(
                f,
                "sock-join-mcast-ipv6 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketLeaveIpv4Multicast {
                fd,
                multiaddr,
                iface,
            } => write!(
                f,
                "sock-leave-mcast-ipv4 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketLeaveIpv6Multicast {
                fd,
                multiaddr,
                iface,
            } => write!(
                f,
                "sock-leave-mcast-ipv6 (fd={}, addr={}, iface={})",
                fd, multiaddr, iface
            ),
            JournalEntry::SocketSendFile {
                socket_fd,
                file_fd,
                offset,
                count,
            } => write!(
                f,
                "sock-send-file (sock-fd={}, file-fd={}, offset={}, count={})",
                socket_fd, file_fd, offset, count
            ),
            JournalEntry::SocketSendTo { fd, data, addr, .. } => write!(
                f,
                "sock-send-to (fd={}, data.len={}, addr={})",
                fd,
                data.len(),
                addr
            ),
            JournalEntry::SocketSend { fd, data, .. } => {
                write!(f, "sock-send (fd={}, data.len={}", fd, data.len())
            }
            JournalEntry::SocketSetOptFlag { fd, opt, flag } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, flag={})", fd, opt, flag)
            }
            JournalEntry::SocketSetOptSize { fd, opt, size } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, size={})", fd, opt, size)
            }
            JournalEntry::SocketSetOptTime { fd, ty, time } => {
                write!(f, "sock-set-opt (fd={}, opt={:?}, time={:?})", fd, ty, time)
            }
            JournalEntry::SocketShutdown { fd, how } => {
                write!(f, "sock-shutdown (fd={}, how={:?})", fd, how)
            }
            JournalEntry::Snapshot { when, trigger } => {
                write!(f, "snapshot (when={:?}, trigger={:?})", when, trigger)
            }
        }
    }
}
