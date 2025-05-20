use std::ops::Range;

use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(super) unsafe fn play_event(
        &mut self,
        next: JournalEntry<'a>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        match next {
            JournalEntry::InitModuleV1 { wasm_hash } => {
                self.action_init_module(wasm_hash, differ_ethereal)?;
            }
            JournalEntry::ClearEtherealV1 => {
                self.clear_ethereal(differ_ethereal);
            }
            JournalEntry::ProcessExitV1 { exit_code } => {
                self.action_process_exit(exit_code, differ_ethereal)?;
            }
            JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_write(fd, offset, data, is_64bit)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %offset, "Differ(ether) journal - FdWrite");
                    differ_ethereal.push(JournalEntry::FileDescriptorWriteV1 {
                        fd,
                        offset,
                        data,
                        is_64bit,
                    });
                } else {
                    self.action_fd_write(fd, offset, data, is_64bit)?;
                }
            }
            JournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_seek(fd, offset, whence)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %offset, ?whence, "Differ(ether) journal - FdSeek");
                    differ_ethereal.push(JournalEntry::FileDescriptorSeekV1 { fd, offset, whence });
                } else {
                    self.action_fd_seek(fd, offset, whence)?;
                }
            }
            JournalEntry::UpdateMemoryRegionV1 {
                region,
                compressed_data,
            } => {
                self.action_update_compressed_memory(region, compressed_data, differ_ethereal)?;
            }
            JournalEntry::CloseThreadV1 { id, exit_code } => {
                self.action_close_thread(id, exit_code, differ_ethereal)?;
            }
            JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
                start,
                layout,
            } => {
                self.action_set_thread(
                    id,
                    call_stack,
                    memory_stack,
                    store_data,
                    is_64bit,
                    start,
                    layout,
                    differ_ethereal,
                )?;
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_close(fd)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - FdClose");
                    differ_ethereal.push(JournalEntry::CloseFileDescriptorV1 { fd });
                } else {
                    self.action_fd_close(fd)?;
                }
            }
            JournalEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => {
                self.real_fd.insert(fd);
                self.action_fd_open(
                    fd,
                    dirfd,
                    dirflags,
                    path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    Fdflagsext::empty(),
                )?;
            }
            JournalEntry::OpenFileDescriptorV2 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
                fd_flags,
            } => {
                self.real_fd.insert(fd);
                self.action_fd_open(
                    fd,
                    dirfd,
                    dirflags,
                    path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    fd_flags,
                )?;
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                tracing::trace!("Replay journal - RemoveDirectory {}", path);
                JournalEffector::apply_path_remove_directory(&mut self.ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::UnlinkFileV1 { fd, path } => {
                tracing::trace!("Replay journal - UnlinkFile {}", path);
                JournalEffector::apply_path_unlink(&mut self.ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => {
                tracing::trace!("Replay journal - PathRename {}->{}", old_path, new_path);
                JournalEffector::apply_path_rename(
                    &mut self.ctx,
                    old_fd,
                    &old_path,
                    new_fd,
                    &new_path,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::SnapshotV1 { when, trigger } => {
                self.action_snapshot(when, trigger, differ_ethereal)?;
            }
            JournalEntry::SetClockTimeV1 { clock_id, time } => {
                tracing::trace!(?clock_id, %time, "Replay journal - ClockTimeSet");
                JournalEffector::apply_clock_time_set(&mut self.ctx, clock_id, time)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                if self.real_fd.remove(&old_fd) {
                    self.action_fd_renumber(old_fd, new_fd)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%old_fd, %new_fd, "Differ(ether) journal - FdRenumber");
                    differ_ethereal.push(JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd });
                } else {
                    self.action_fd_renumber(old_fd, new_fd)?;
                }
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                if self.real_fd.contains(&original_fd) {
                    self.action_fd_dup(original_fd, copied_fd, false)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%original_fd, %copied_fd, "Differ(ether) journal - FdDuplicate");
                    differ_ethereal.push(JournalEntry::DuplicateFileDescriptorV1 {
                        original_fd,
                        copied_fd,
                    });
                } else {
                    self.action_fd_dup(original_fd, copied_fd, false)?;
                }
            }
            JournalEntry::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                cloexec,
            } => {
                if self.real_fd.contains(&original_fd) {
                    self.action_fd_dup(original_fd, copied_fd, cloexec)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%original_fd, %copied_fd, %cloexec, "Differ(ether) journal - FdDuplicate");
                    differ_ethereal.push(JournalEntry::DuplicateFileDescriptorV2 {
                        original_fd,
                        copied_fd,
                        cloexec,
                    });
                } else {
                    self.action_fd_dup(original_fd, copied_fd, cloexec)?;
                }
            }
            JournalEntry::CreateDirectoryV1 { fd, path } => {
                tracing::trace!(%fd, %path, "Replay journal - CreateDirectory");
                JournalEffector::apply_path_create_directory(&mut self.ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::PathSetTimesV1 {
                fd,
                flags,
                path,
                st_atim,
                st_mtim,
                fst_flags,
            } => {
                if self.real_fd.contains(&fd) {
                    self.action_path_set_times(fd, flags, path, st_atim, st_mtim, fst_flags)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - PathSetTimes");
                    differ_ethereal.push(JournalEntry::PathSetTimesV1 {
                        fd,
                        flags,
                        path,
                        st_atim,
                        st_mtim,
                        fst_flags,
                    });
                } else {
                    self.action_path_set_times(fd, flags, path, st_atim, st_mtim, fst_flags)?;
                }
            }
            JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_set_times(fd, st_atim, st_mtim, fst_flags)?
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %st_atim, %st_mtim, ?fst_flags, "Differ(ether) journal - FdSetTimes");
                    differ_ethereal.push(JournalEntry::FileDescriptorSetTimesV1 {
                        fd,
                        st_atim,
                        st_mtim,
                        fst_flags,
                    });
                } else {
                    self.action_fd_set_times(fd, st_atim, st_mtim, fst_flags)?
                }
            }
            JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_set_size(fd, st_size)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %st_size, "Differ(ether) journal - FdSetSize");
                    differ_ethereal.push(JournalEntry::FileDescriptorSetSizeV1 { fd, st_size });
                } else {
                    self.action_fd_set_size(fd, st_size)?;
                }
            }
            JournalEntry::FileDescriptorSetFdFlagsV1 { fd, flags } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_set_fdflags(fd, flags)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?flags, "Differ(ether) journal - FdSetFdFlags");
                    differ_ethereal.push(JournalEntry::FileDescriptorSetFdFlagsV1 { fd, flags });
                } else {
                    self.action_fd_set_fdflags(fd, flags)?;
                }
            }
            JournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_set_flags(fd, flags)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?flags, "Differ(ether) journal - FdSetFlags");
                    differ_ethereal.push(JournalEntry::FileDescriptorSetFlagsV1 { fd, flags });
                } else {
                    self.action_fd_set_flags(fd, flags)?;
                }
            }
            JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_set_rights(fd, fs_rights_base, fs_rights_inheriting)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - FdSetRights");
                    differ_ethereal.push(JournalEntry::FileDescriptorSetRightsV1 {
                        fd,
                        fs_rights_base,
                        fs_rights_inheriting,
                    });
                } else {
                    self.action_fd_set_rights(fd, fs_rights_base, fs_rights_inheriting)?;
                }
            }
            JournalEntry::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_advise(fd, offset, len, advice)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %offset, %len, ?advice, "Differ(ether) journal - FdAdvise");
                    differ_ethereal.push(JournalEntry::FileDescriptorAdviseV1 {
                        fd,
                        offset,
                        len,
                        advice,
                    });
                } else {
                    self.action_fd_advise(fd, offset, len, advice)?;
                }
            }
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                if self.real_fd.contains(&fd) {
                    self.action_fd_allocate(fd, offset, len)?;
                } else if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %offset, %len, "Differ(ether) journal - FdAllocate");
                    differ_ethereal.push(JournalEntry::FileDescriptorAllocateV1 {
                        fd,
                        offset,
                        len,
                    });
                } else {
                    self.action_fd_allocate(fd, offset, len)?;
                }
            }
            JournalEntry::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => {
                tracing::trace!("Replay journal - PathLink {}->{}", old_path, new_path);
                JournalEffector::apply_path_link(
                    &mut self.ctx,
                    old_fd,
                    old_flags,
                    &old_path,
                    new_fd,
                    &new_path,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => {
                tracing::trace!("Replay journal - PathSymlink {}->{}", old_path, new_path);
                JournalEffector::apply_path_symlink(&mut self.ctx, &old_path, fd, &new_path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::ChangeDirectoryV1 { path } => {
                tracing::trace!("Replay journal - ChangeDirection {}", path);
                JournalEffector::apply_chdir(&mut self.ctx, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            JournalEntry::CreatePipeV1 { read_fd, write_fd } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%read_fd, %write_fd,  "Differ(ether) journal - CreatePipe");
                    differ_ethereal.push(JournalEntry::CreatePipeV1 { read_fd, write_fd });
                } else {
                    tracing::trace!(%read_fd, %write_fd,  "Replay journal - CreatePipe");
                    JournalEffector::apply_fd_pipe(&mut self.ctx, read_fd, write_fd)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            JournalEntry::EpollCreateV1 { fd } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - EpollCreate");
                    differ_ethereal.push(JournalEntry::EpollCreateV1 { fd });
                } else {
                    tracing::trace!(%fd, "Replay journal - EpollCreate");
                    JournalEffector::apply_epoll_create(&mut self.ctx, fd)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%epfd, %fd, ?op, "Differ(ether) journal - EpollCtl");
                    differ_ethereal.push(JournalEntry::EpollCtlV1 {
                        epfd,
                        op,
                        fd,
                        event,
                    });
                } else {
                    tracing::trace!(%epfd, %fd, ?op, "Replay journal - EpollCtl");
                    JournalEffector::apply_epoll_ctl(&mut self.ctx, epfd, op, fd, event)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            JournalEntry::TtySetV1 { tty, line_feeds } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!("Differ(ether) journal - TtySet");
                    differ_ethereal.push(JournalEntry::TtySetV1 { tty, line_feeds });
                } else {
                    self.action_tty_set(tty, line_feeds)?;
                }
            }
            JournalEntry::PortAddAddrV1 { cidr } => {
                tracing::trace!(?cidr, "Replay journal - PortAddAddr");
                JournalEffector::apply_port_addr_add(&mut self.ctx, cidr)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortDelAddrV1 { addr } => {
                tracing::trace!(?addr, "Replay journal - PortDelAddr");
                JournalEffector::apply_port_addr_remove(&mut self.ctx, addr)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortAddrClearV1 => {
                tracing::trace!("Replay journal - PortAddrClear");
                JournalEffector::apply_port_addr_clear(&mut self.ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => {
                tracing::trace!("Replay journal - PortBridge");
                JournalEffector::apply_port_bridge(&mut self.ctx, &network, &token, security)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortUnbridgeV1 => {
                tracing::trace!("Replay journal - PortUnBridge");
                JournalEffector::apply_port_unbridge(&mut self.ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortDhcpAcquireV1 => {
                tracing::trace!("Replay journal - PortDhcpAcquire");
                JournalEffector::apply_port_dhcp_acquire(&mut self.ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortGatewaySetV1 { ip } => {
                tracing::trace!(?ip, "Replay journal - PortGatewaySet");
                JournalEffector::apply_port_gateway_set(&mut self.ctx, ip)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => {
                tracing::trace!(?cidr, "Replay journal - PortRouteAdd");
                JournalEffector::apply_port_route_add(
                    &mut self.ctx,
                    cidr,
                    via_router,
                    preferred_until,
                    expires_at,
                )
                .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortRouteClearV1 => {
                tracing::trace!("Replay journal - PortRouteClear");
                JournalEffector::apply_port_route_clear(&mut self.ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::PortRouteDelV1 { ip } => {
                tracing::trace!(?ip, "Replay journal - PortRouteDel");
                JournalEffector::apply_port_route_remove(&mut self.ctx, ip)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            JournalEntry::SocketOpenV1 { af, ty, pt, fd } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(?af, ?ty, ?pt, %fd, "Differ(ether) journal - SocketOpen");
                    differ_ethereal.push(JournalEntry::SocketOpenV1 { af, ty, pt, fd });
                } else {
                    tracing::trace!(?af, ?ty, ?pt, %fd, "Replay journal - SocketOpen");
                    JournalEffector::apply_sock_open(&mut self.ctx, af, ty, pt, fd)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketPairV1 { fd1, fd2 } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd1, %fd2, "Differ(ether) journal - SocketPair");
                    differ_ethereal.push(JournalEntry::SocketPairV1 { fd1, fd2 });
                } else {
                    tracing::trace!(%fd1, %fd2, "Replay journal - SocketOpen");
                    JournalEffector::apply_sock_pair(&mut self.ctx, fd1, fd2)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketListenV1 { fd, backlog } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - SocketListen");
                    differ_ethereal.push(JournalEntry::SocketListenV1 { fd, backlog });
                } else {
                    tracing::trace!(%fd, "Replay journal - SocketListen");
                    JournalEffector::apply_sock_listen(&mut self.ctx, fd, backlog as usize)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketBindV1 { fd, addr } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?addr, "Differ(ether) journal - SocketBind");
                    differ_ethereal.push(JournalEntry::SocketBindV1 { fd, addr });
                } else {
                    tracing::trace!(%fd, ?addr, "Replay journal - SocketBind");
                    JournalEffector::apply_sock_bind(&mut self.ctx, fd, addr)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?peer_addr, "Differ(ether) journal - SockConnect");
                    differ_ethereal.push(JournalEntry::SocketConnectedV1 {
                        fd,
                        local_addr,
                        peer_addr,
                    });
                } else {
                    let connected_sockets_are_dead = self.connected_sockets_are_dead;
                    tracing::trace!(%fd, ?peer_addr, "Replay journal - SockConnect");
                    JournalEffector::apply_sock_connect(
                        &mut self.ctx,
                        fd,
                        local_addr,
                        peer_addr,
                        connected_sockets_are_dead,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr: addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%listen_fd, %fd, ?peer_addr, "Differ(ether) journal - SocketAccept");
                    differ_ethereal.push(JournalEntry::SocketAcceptedV1 {
                        listen_fd,
                        fd,
                        local_addr: addr,
                        peer_addr,
                        fd_flags,
                        non_blocking: nonblocking,
                    });
                } else {
                    tracing::trace!(%listen_fd, %fd, ?peer_addr, "Replay journal - SocketAccept");
                    JournalEffector::apply_sock_accepted(
                        &mut self.ctx,
                        listen_fd,
                        fd,
                        addr,
                        peer_addr,
                        fd_flags,
                        nonblocking,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?multiaddr, "Differ(ether) journal - JoinIpv4Multicast");
                    differ_ethereal.push(JournalEntry::SocketJoinIpv4MulticastV1 {
                        fd,
                        multiaddr,
                        iface,
                    });
                } else {
                    tracing::trace!(%fd, ?multiaddr, "Replay journal - JoinIpv4Multicast");
                    JournalEffector::apply_sock_join_ipv4_multicast(
                        &mut self.ctx,
                        fd,
                        multiaddr,
                        iface,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?multiaddr, "Differ(ether) journal - JoinIpv6Multicast");
                    differ_ethereal.push(JournalEntry::SocketJoinIpv6MulticastV1 {
                        fd,
                        multi_addr: multiaddr,
                        iface,
                    });
                } else {
                    tracing::trace!(%fd, ?multiaddr, "Replay journal - JoinIpv6Multicast");
                    JournalEffector::apply_sock_join_ipv6_multicast(
                        &mut self.ctx,
                        fd,
                        multiaddr,
                        iface,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?multiaddr, "Differ(ether) journal - LeaveIpv4Multicast");
                    differ_ethereal.push(JournalEntry::SocketLeaveIpv4MulticastV1 {
                        fd,
                        multi_addr: multiaddr,
                        iface,
                    });
                } else {
                    tracing::trace!(%fd, ?multiaddr, "Replay journal - LeaveIpv4Multicast");
                    JournalEffector::apply_sock_leave_ipv4_multicast(
                        &mut self.ctx,
                        fd,
                        multiaddr,
                        iface,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?multiaddr, "Differ(ether) journal - LeaveIpv6Multicast");
                    differ_ethereal.push(JournalEntry::SocketLeaveIpv6MulticastV1 {
                        fd,
                        multi_addr: multiaddr,
                        iface,
                    });
                } else {
                    tracing::trace!(%fd, ?multiaddr, "Replay journal - LeaveIpv6Multicast");
                    JournalEffector::apply_sock_leave_ipv6_multicast(
                        &mut self.ctx,
                        fd,
                        multiaddr,
                        iface,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => {
                if self.connected_sockets_are_dead {
                    return Ok(());
                }
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%socket_fd, %file_fd, %offset, %count, "Differ(ether) journal - SockSendFile");
                    differ_ethereal.push(JournalEntry::SocketSendFileV1 {
                        socket_fd,
                        file_fd,
                        offset,
                        count,
                    });
                } else {
                    tracing::trace!(%socket_fd, %file_fd, %offset, %count, "Replay journal - SockSendFile");
                    JournalEffector::apply_sock_send_file(
                        &mut self.ctx,
                        socket_fd,
                        file_fd,
                        offset,
                        count,
                    )
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => {
                if self.connected_sockets_are_dead {
                    return Ok(());
                }
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - SocketSendTo data={} bytes", data.len());
                    differ_ethereal.push(JournalEntry::SocketSendToV1 {
                        fd,
                        data,
                        flags,
                        addr,
                        is_64bit,
                    });
                } else {
                    tracing::trace!(%fd, "Replay journal - SocketSendTo data={} bytes", data.len());
                    if is_64bit {
                        JournalEffector::apply_sock_send_to::<Memory64>(
                            &self.ctx, fd, data, flags, addr,
                        )
                    } else {
                        JournalEffector::apply_sock_send_to::<Memory32>(
                            &self.ctx, fd, data, flags, addr,
                        )
                    }
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => {
                if self.connected_sockets_are_dead {
                    return Ok(());
                }
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, "Differ(ether) journal - SocketSend data={} bytes", data.len());
                    differ_ethereal.push(JournalEntry::SocketSendV1 {
                        fd,
                        data,
                        flags,
                        is_64bit,
                    });
                } else {
                    tracing::trace!(%fd, "Replay journal - SocketSend data={} bytes", data.len());
                    if is_64bit {
                        JournalEffector::apply_sock_send::<Memory64>(&self.ctx, fd, data, flags)
                    } else {
                        JournalEffector::apply_sock_send::<Memory32>(&self.ctx, fd, data, flags)
                    }
                    .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?opt, %flag, "Differ(ether) journal - SocketSetOptFlag");
                    differ_ethereal.push(JournalEntry::SocketSetOptFlagV1 { fd, opt, flag });
                } else {
                    tracing::trace!(%fd, ?opt, %flag, "Replay journal - SocketSetOptFlag");
                    JournalEffector::apply_sock_set_opt_flag(&mut self.ctx, fd, opt, flag)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSetOptSizeV1 { fd, opt, size } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?opt, %size, "Differ(ether) journal - SocketSetOptSize");
                    differ_ethereal.push(JournalEntry::SocketSetOptSizeV1 { fd, opt, size });
                } else {
                    tracing::trace!(%fd, ?opt, %size, "Replay journal - SocketSetOptSize");
                    JournalEffector::apply_sock_set_opt_size(&mut self.ctx, fd, opt, size)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketSetOptTimeV1 { fd, ty, time } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?ty, ?time, "Differ(ether) journal - SocketSetOptTime");
                    differ_ethereal.push(JournalEntry::SocketSetOptTimeV1 { fd, ty, time });
                } else {
                    tracing::trace!(%fd, ?ty, ?time, "Replay journal - SocketSetOptTime");
                    JournalEffector::apply_sock_set_opt_time(&mut self.ctx, fd, ty.into(), time)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::SocketShutdownV1 { fd, how } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, ?how, "Differ(ether) journal - SocketShutdown");
                    differ_ethereal.push(JournalEntry::SocketShutdownV1 { fd, how });
                } else {
                    tracing::trace!(%fd, ?how, "Replay journal - SocketShutdown");
                    JournalEffector::apply_sock_shutdown(&mut self.ctx, fd, how.into())
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
            JournalEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => {
                if let Some(differ_ethereal) = differ_ethereal {
                    tracing::trace!(%fd, %flags, "Differ(ether) journal - CreateEvent");
                    differ_ethereal.push(JournalEntry::CreateEventV1 {
                        initial_val,
                        flags,
                        fd,
                    });
                } else {
                    tracing::trace!(%fd, %flags, "Replay journal - CreateEvent");
                    JournalEffector::apply_fd_event(&mut self.ctx, initial_val, flags, fd)
                        .map_err(anyhow_err_to_runtime_err)?
                }
            }
        }
        Ok(())
    }
}
