use std::fmt;

use super::*;
use lz4_flex::block::uncompressed_size;
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

/// The printing journal writes all the journal entries to the console
/// as either text or json.
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
            JournalPrintingMode::Text => println!("{entry}"),
            JournalPrintingMode::Json => {
                println!("{}", serde_json::to_string_pretty(&entry)?)
            }
        }
        Ok(LogWriteResult {
            record_start: 0,
            record_end: entry.estimate_size() as u64,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        Ok(())
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
                write!(f, "init-module (hash={wasm_hash:x?})")
            }
            JournalEntry::ClearEtherealV1 => {
                write!(f, "clear-ethereal")
            }
            JournalEntry::UpdateMemoryRegionV1 {
                region,
                compressed_data,
            } => write!(
                f,
                "memory-update (start={}, end={}, data.len={}, compressed.len={})",
                region.start,
                region.end,
                uncompressed_size(compressed_data.as_ref())
                    .map(|a| a.0)
                    .unwrap_or_else(|_| compressed_data.as_ref().len()),
                compressed_data.len()
            ),
            JournalEntry::ProcessExitV1 { exit_code } => {
                write!(f, "process-exit (code={exit_code:?})")
            }
            JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                ..
            } => write!(
                f,
                "thread-update (id={}, call-stack.len={}, mem-stack.len={}, store-size={})",
                id,
                call_stack.len(),
                memory_stack.len(),
                store_data.len(),
            ),
            JournalEntry::CloseThreadV1 { id, exit_code } => {
                write!(f, "thread-close (id={id}, code={exit_code:?})")
            }
            JournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => {
                write!(f, "fd-seek (fd={fd}, offset={offset}, whence={whence:?})")
            },
            JournalEntry::FileDescriptorWriteV1 {
                fd, offset, data, ..
            } => write!(f, "fd-write (fd={fd}, offset={offset}, data.len={})", data.len()),
            JournalEntry::SetClockTimeV1 { clock_id, time } => {
                write!(f, "set-clock-time (id={clock_id:?}, time={time})")
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => write!(f, "fd-close (fd={fd})"),
            JournalEntry::OpenFileDescriptorV1 {
                fd, path, o_flags, ..
            }
            | JournalEntry::OpenFileDescriptorV2 {
                fd, path, o_flags, ..
            }=> {
                if o_flags.contains(wasi::Oflags::CREATE) {
                    if o_flags.contains(wasi::Oflags::TRUNC) {
                        write!(f, "fd-create-new (fd={fd}, path={path})")
                    } else if o_flags.contains(wasi::Oflags::EXCL) {
                        write!(f, "fd-create-excl (fd={fd}, path={path})")
                    } else {
                        write!(f, "fd-create (fd={fd}, path={path})")
                    }
                } else if o_flags.contains(wasi::Oflags::TRUNC) {
                    write!(f, "fd-open-new (fd={fd}, path={path})")
                } else {
                    write!(f, "fd-open (fd={fd}, path={path})")
                }
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                write!(f, "fd-renumber (old={old_fd}, new={new_fd})")
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => write!(f, "fd-duplicate (original={original_fd}, copied={copied_fd})"),
            JournalEntry::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                cloexec
            } => write!(f, "fd-duplicate (original={original_fd}, copied={copied_fd}, cloexec={cloexec})"),
            JournalEntry::CreateDirectoryV1 { fd, path } => {
                write!(f, "path-create-dir (fd={fd}, path={path})")
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                write!(f, "path-remove-dir (fd={fd}, path={path})")
            }
            JournalEntry::PathSetTimesV1 {
                path,
                st_atim,
                st_mtim,
                ..
            } => write!(f, "path-set-times (path={path}, atime={st_atim}, mtime={st_mtim}))"),
            JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                ..
            } => write!(f, "fd-set-times (fd={fd}, atime={st_atim}, mtime={st_mtim})"),
            JournalEntry::FileDescriptorSetFdFlagsV1 { fd, flags } => {
                write!(f, "fd-set-fd-flags (fd={fd}, flags={flags:?})")
            }
            JournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                write!(f, "fd-set-flags (fd={fd}, flags={flags:?})")
            }
            JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => write!(f, "fd-set-rights (fd={fd}, base={fs_rights_base:?}, inherited={fs_rights_inheriting:?})"),
            JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                write!(f, "fd-set-size (fd={fd}, size={st_size})")
            }
            JournalEntry::FileDescriptorAdviseV1 {
                fd, offset, len, ..
            } => write!(f, "fd-advise (fd={fd}, offset={offset}, len={len})"),
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                write!(f, "fd-allocate (fd={fd}, offset={offset}, len={len})")
            }
            JournalEntry::CreateHardLinkV1 {
                old_path, new_path, ..
            } => write!(f, "path-link (from={old_path}, to={new_path})"),
            JournalEntry::CreateSymbolicLinkV1 {
                old_path, new_path, ..
            } => write!(f, "path-symlink (from={old_path}, to={new_path})"),
            JournalEntry::UnlinkFileV1 { path, .. } => write!(f, "path-unlink (path={path})"),
            JournalEntry::PathRenameV1 {
                old_path, new_path, ..
            } => write!(f, "path-rename (old-path={old_path}, new-path={new_path})"),
            JournalEntry::ChangeDirectoryV1 { path } => write!(f, "chdir (path={path})"),
            JournalEntry::EpollCreateV1 { fd } => write!(f, "epoll-create (fd={fd})"),
            JournalEntry::EpollCtlV1 { epfd, op, fd, .. } => {
                write!(f, "epoll-ctl (epfd={epfd}, op={op:?}, fd={fd})")
            }
            JournalEntry::TtySetV1 { tty, line_feeds } => write!(f, "tty-set (echo={}, buffering={}, feeds={})", tty.echo, tty.line_buffered, line_feeds),
            JournalEntry::CreatePipeV1 { read_fd, write_fd } => {
                write!(f, "fd-pipe (read_fd={read_fd}, write_fd={write_fd})")
            }
            JournalEntry::CreateEventV1 {
                initial_val, fd, ..
            } => write!(f, "fd-event (fd={fd}, initial={initial_val})"),
            JournalEntry::PortAddAddrV1 { cidr } => {
                write!(f, "port-addr-add (ip={}, prefix={})", cidr.ip, cidr.prefix)
            }
            JournalEntry::PortDelAddrV1 { addr } => write!(f, "port-addr-del (addr={addr})"),
            JournalEntry::PortAddrClearV1 => write!(f, "port-addr-clear"),
            JournalEntry::PortBridgeV1 { network, .. } => {
                write!(f, "port-bridge (network={network})")
            }
            JournalEntry::PortUnbridgeV1 => write!(f, "port-unbridge"),
            JournalEntry::PortDhcpAcquireV1 => write!(f, "port-dhcp-acquire"),
            JournalEntry::PortGatewaySetV1 { ip } => write!(f, "port-gateway-set (ip={ip})"),
            JournalEntry::PortRouteAddV1 {
                cidr, via_router, ..
            } => write!(
                f,
                "port-route-add (ip={}, prefix={}, via_router={})",
                cidr.ip, cidr.prefix, via_router
            ),
            JournalEntry::PortRouteClearV1 => write!(f, "port-route-clear"),
            JournalEntry::PortRouteDelV1 { ip } => write!(f, "port-route-del (ip={ip})"),
            JournalEntry::SocketOpenV1 { af, ty, pt, fd } => {
                write!(f, "sock-open (fd={fd}, af={af:?}, ty={ty:?}, pt={pt:?})")
            }
            JournalEntry::SocketPairV1 { fd1, fd2 } => {
                write!(f, "sock-pair (fd1={fd1}, fd2={fd2})")
            }
            JournalEntry::SocketListenV1 { fd, backlog } => {
                write!(f, "sock-listen (fd={fd}, backlog={backlog})")
            }
            JournalEntry::SocketBindV1 { fd, addr } => {
                write!(f, "sock-bind (fd={fd}, addr={addr})")
            }
            JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => {
                write!(f, "sock-connect (fd={fd}, addr={local_addr}, peer={peer_addr})")
            }
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr,
                peer_addr,
                ..
            } => write!(f, "sock-accept (listen-fd={listen_fd}, sock_fd={fd}, addr={local_addr}, peer={peer_addr})"),
            JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => write!(f, "sock-join-mcast-ipv4 (fd={fd}, addr={multiaddr}, iface={iface})"),
            JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(f, "sock-join-mcast-ipv6 (fd={fd}, addr={multiaddr}, iface={iface})"),
            JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(f, "sock-leave-mcast-ipv4 (fd={fd}, addr={multiaddr}, iface={iface})"),
            JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => write!(f, "sock-leave-mcast-ipv6 (fd={fd}, addr={multiaddr}, iface={iface})"),
            JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => write!(f, "sock-send-file (sock-fd={socket_fd}, file-fd={file_fd}, offset={offset}, count={count})"),
            JournalEntry::SocketSendToV1 { fd, data, addr, .. } => {
                write!(f, "sock-send-to (fd={}, data.len={}, addr={})", fd, data.len(), addr)
            }
            JournalEntry::SocketSendV1 { fd, data, .. } => {
                write!(f, "sock-send (fd={}, data.len={})", fd, data.len())
            }
            JournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => {
                write!(f, "sock-set-opt (fd={fd}, opt={opt:?}, flag={flag})")
            }
            JournalEntry::SocketSetOptSizeV1 { fd, opt, size } => {
                write!(f, "sock-set-opt (fd={fd}, opt={opt:?}, size={size})")
            }
            JournalEntry::SocketSetOptTimeV1 { fd, ty, time } => {
                write!(f, "sock-set-opt (fd={fd}, opt={ty:?}, time={time:?})")
            }
            JournalEntry::SocketShutdownV1 { fd, how } => {
                write!(f, "sock-shutdown (fd={fd}, how={how:?})")
            }
            JournalEntry::SnapshotV1 { when, trigger } => {
                write!(f, "snapshot (when={when:?}, trigger={trigger:?})")
            }
        }
    }
}
