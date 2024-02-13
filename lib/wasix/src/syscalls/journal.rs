use super::*;

#[allow(clippy::extra_unused_type_parameters)]
#[cfg(not(feature = "journal"))]
pub fn maybe_snapshot_once<'a, M: MemorySize>(
    ctx: FunctionEnvMut<'a, WasiEnv>,
    _trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    Ok(Ok(ctx))
}

#[cfg(feature = "journal")]
pub fn maybe_snapshot_once<'a, M: MemorySize>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
    trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    use crate::os::task::process::{WasiProcessCheckpoint, WasiProcessInner};

    if unsafe { handle_rewind_ext_with_default::<M, ()>(&mut ctx, HandleRewindType::ResultLess) }
        .is_some()
    {
        return Ok(Ok(ctx));
    }

    if !ctx.data().enable_journal {
        return Ok(Ok(ctx));
    }

    if ctx.data_mut().pop_snapshot_trigger(trigger) {
        let inner = ctx.data().process.inner.clone();
        let res = wasi_try_ok_ok!(WasiProcessInner::checkpoint::<M>(
            inner,
            ctx,
            WasiProcessCheckpoint::Snapshot { trigger },
        )?);
        match res {
            MaybeCheckpointResult::Unwinding => return Ok(Err(Errno::Success)),
            MaybeCheckpointResult::NotThisTime(c) => {
                ctx = c;
            }
        }
    }
    Ok(Ok(ctx))
}

#[allow(clippy::extra_unused_type_parameters)]
#[cfg(not(feature = "journal"))]
pub fn maybe_snapshot_many<'a, M: MemorySize>(
    ctx: FunctionEnvMut<'a, WasiEnv>,
    _trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    Ok(Ok(ctx))
}

#[cfg(feature = "journal")]
pub fn maybe_snapshot_many<'a, M: MemorySize>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
    trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    use crate::os::task::process::{WasiProcessCheckpoint, WasiProcessInner};

    if unsafe { handle_rewind_ext_with_default::<M, ()>(&mut ctx, HandleRewindType::ResultLess) }
        .is_some()
    {
        return Ok(Ok(ctx));
    }

    if !ctx.data().enable_journal {
        return Ok(Ok(ctx));
    }

    if ctx.data_mut().has_snapshot_trigger(trigger) {
        let inner = ctx.data().process.inner.clone();
        let res = wasi_try_ok_ok!(WasiProcessInner::checkpoint::<M>(
            inner,
            ctx,
            WasiProcessCheckpoint::Snapshot { trigger },
        )?);
        match res {
            MaybeCheckpointResult::Unwinding => return Ok(Err(Errno::Success)),
            MaybeCheckpointResult::NotThisTime(c) => {
                ctx = c;
            }
        }
    }
    Ok(Ok(ctx))
}

#[allow(clippy::extra_unused_type_parameters)]
#[cfg(not(feature = "journal"))]
pub fn maybe_snapshot<'a, M: MemorySize>(
    ctx: FunctionEnvMut<'a, WasiEnv>,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    Ok(Ok(ctx))
}

#[cfg(feature = "journal")]
pub fn maybe_snapshot<'a, M: MemorySize>(
    mut ctx: FunctionEnvMut<'a, WasiEnv>,
) -> WasiResult<FunctionEnvMut<'a, WasiEnv>> {
    use crate::os::task::process::{WasiProcessCheckpoint, WasiProcessInner};

    if !ctx.data().enable_journal {
        return Ok(Ok(ctx));
    }

    let inner = ctx.data().process.inner.clone();
    let res = wasi_try_ok_ok!(WasiProcessInner::maybe_checkpoint::<M>(inner, ctx)?);
    match res {
        MaybeCheckpointResult::Unwinding => return Ok(Err(Errno::Success)),
        MaybeCheckpointResult::NotThisTime(c) => {
            ctx = c;
        }
    }
    Ok(Ok(ctx))
}

#[cfg(not(feature = "journal"))]
pub fn wait_for_snapshot(_env: &WasiEnv) -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(std::future::pending())
}

#[cfg(feature = "journal")]
pub fn wait_for_snapshot(env: &WasiEnv) -> Pin<Box<dyn Future<Output = ()>>> {
    use crate::os::task::process::{LockableWasiProcessInner, WasiProcessCheckpoint};

    struct Poller {
        inner: LockableWasiProcessInner,
    }
    impl Future for Poller {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut guard = self.inner.0.lock().unwrap();
            if !matches!(guard.checkpoint, WasiProcessCheckpoint::Execute) {
                return Poll::Ready(());
            }
            if !guard.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                guard.wakers.push(cx.waker().clone());
            }
            Poll::Pending
        }
    }
    Box::pin(Poller {
        inner: env.process.inner.clone(),
    })
}

/// Safety: This function manipulates the memory of the process and thus must
/// be executed by the WASM process thread itself.
///
#[allow(clippy::result_large_err)]
#[cfg(feature = "journal")]
pub unsafe fn restore_snapshot(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    journal: Arc<DynJournal>,
    bootstrapping: bool,
) -> Result<Option<RewindState>, WasiRuntimeError> {
    use std::{collections::BTreeMap, ops::Range};

    use crate::{journal::Journal, os::task::process::MemorySnapshotRegion};

    // We delay the spawning of threads until the end as its
    // possible that the threads will be cancelled before all the
    // events finished the streaming process
    let mut spawn_threads: BTreeMap<WasiThreadId, RewindState> = Default::default();
    let mut staging_spawn_threads: BTreeMap<WasiThreadId, RewindState> = Default::default();
    let mut staging_kill_thread: HashSet<WasiThreadId> = Default::default();

    // We delay the memory updates until the end as its possible the
    // memory will be cleared before all the events finished the
    // streaming process
    let mut update_memory: BTreeMap<MemorySnapshotRegion, Cow<'_, [u8]>> = Default::default();
    let mut staging_update_memory: BTreeMap<MemorySnapshotRegion, Cow<'_, [u8]>> =
        Default::default();
    let mut update_tty = None;

    // We capture the stdout and stderr while we replay
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_fds = HashSet::new();
    let mut stderr_fds = HashSet::new();
    stdout_fds.insert(1 as WasiFd);
    stderr_fds.insert(2 as WasiFd);

    // Loop through all the events and process them
    let cur_module_hash = ctx.data().process.module_hash.as_bytes();
    let mut journal_module_hash = None;
    let mut rewind = None;
    let mut staging_rewind = None;
    while let Some(next) = journal.read().map_err(anyhow_err_to_runtime_err)? {
        tracing::trace!("Restoring snapshot event - {next:?}");
        match next.into_inner() {
            crate::journal::JournalEntry::InitModuleV1 { wasm_hash } => {
                journal_module_hash.replace(wasm_hash);
            }
            crate::journal::JournalEntry::ProcessExitV1 { exit_code } => {
                if bootstrapping {
                    rewind = None;
                    staging_rewind = None;
                    spawn_threads.clear();
                    staging_spawn_threads.clear();
                    update_memory.clear();
                    staging_update_memory.clear();
                    update_tty.take();
                    stdout.clear();
                    stderr.clear();
                    stdout_fds.clear();
                    stderr_fds.clear();
                    stdout_fds.insert(1 as WasiFd);
                    stderr_fds.insert(2 as WasiFd);
                } else {
                    JournalEffector::apply_process_exit(&mut ctx, exit_code)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            crate::journal::JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => {
                if stdout_fds.contains(&fd) {
                    stdout.push((offset, data, is_64bit));
                    continue;
                }
                if stderr_fds.contains(&fd) {
                    stderr.push((offset, data, is_64bit));
                    continue;
                }

                if is_64bit {
                    JournalEffector::apply_fd_write::<Memory64>(&ctx, fd, offset, data)
                } else {
                    JournalEffector::apply_fd_write::<Memory32>(&ctx, fd, offset, data)
                }
                .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => {
                JournalEffector::apply_fd_seek(&mut ctx, fd, offset, whence)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::UpdateMemoryRegionV1 { region, data } => {
                if Some(cur_module_hash) != journal_module_hash {
                    continue;
                }

                if bootstrapping {
                    staging_update_memory.insert(region.into(), data.clone());
                } else {
                    JournalEffector::apply_memory(&mut ctx, region, &data)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            crate::journal::JournalEntry::CloseThreadV1 { id, exit_code } => {
                if id == ctx.data().tid().raw() {
                    if bootstrapping {
                        rewind = None;
                        staging_rewind = None;
                        spawn_threads.clear();
                        staging_spawn_threads.clear();
                        update_memory.clear();
                        staging_update_memory.clear();
                        update_tty.take();
                        stdout.clear();
                        stderr.clear();
                        stdout_fds.clear();
                        stderr_fds.clear();
                        stdout_fds.insert(1 as WasiFd);
                        stderr_fds.insert(2 as WasiFd);
                    } else {
                        JournalEffector::apply_process_exit(&mut ctx, exit_code)
                            .map_err(anyhow_err_to_runtime_err)?;
                    }
                } else if bootstrapping {
                    staging_spawn_threads.remove(&Into::<WasiThreadId>::into(id));
                    staging_kill_thread.insert(id.into());
                } else {
                    JournalEffector::apply_thread_exit(
                        &mut ctx,
                        Into::<WasiThreadId>::into(id),
                        exit_code,
                    )
                    .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            crate::journal::JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
                start,
                layout,
            } => {
                if Some(cur_module_hash) != journal_module_hash {
                    continue;
                }

                let state = RewindState {
                    memory_stack: memory_stack.to_vec().into(),
                    rewind_stack: call_stack.to_vec().into(),
                    store_data: store_data.to_vec().into(),
                    start,
                    layout,
                    is_64bit,
                };

                let id = Into::<WasiThreadId>::into(id);
                if id == ctx.data().tid() {
                    staging_rewind.replace(state);
                } else if bootstrapping {
                    staging_kill_thread.remove(&id);
                    staging_spawn_threads.insert(id, state);
                } else {
                    return Err(WasiRuntimeError::Runtime(RuntimeError::user(
                        anyhow::format_err!(
                            "Snapshot restoration does not currently support live updates of running threads."
                        )
                        .into(),
                    )));
                }
            }
            crate::journal::JournalEntry::CloseFileDescriptorV1 { fd } => {
                stdout_fds.remove(&fd);
                stderr_fds.remove(&fd);
                JournalEffector::apply_fd_close(&mut ctx, fd).map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => {
                JournalEffector::apply_path_open(
                    &mut ctx,
                    fd,
                    dirfd,
                    dirflags,
                    &path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::RemoveDirectoryV1 { fd, path } => {
                JournalEffector::apply_path_remove_directory(&mut ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::UnlinkFileV1 { fd, path } => {
                JournalEffector::apply_path_unlink(&mut ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => {
                JournalEffector::apply_path_rename(&mut ctx, old_fd, &old_path, new_fd, &new_path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::SnapshotV1 { when: _, trigger } => {
                if Some(cur_module_hash) != journal_module_hash {
                    continue;
                }

                if let Some(new_rewind) = staging_rewind.take() {
                    rewind.replace(new_rewind);
                }
                for thread_id in staging_kill_thread.drain() {
                    spawn_threads.remove(&thread_id);
                }
                while let Some(thread) = staging_spawn_threads.pop_first() {
                    spawn_threads.insert(thread.0, thread.1);
                }
                while let Some(mem) = staging_update_memory.pop_first() {
                    update_memory.insert(mem.0, mem.1);
                }

                ctx.data_mut().pop_snapshot_trigger(trigger);
            }
            crate::journal::JournalEntry::SetClockTimeV1 { clock_id, time } => {
                JournalEffector::apply_clock_time_set(&mut ctx, clock_id, time)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                if old_fd != new_fd {
                    stdout_fds.remove(&new_fd);
                    stderr_fds.remove(&new_fd);
                }
                if stdout_fds.remove(&old_fd) {
                    stdout_fds.insert(new_fd);
                }
                if stderr_fds.remove(&old_fd) {
                    stderr_fds.insert(new_fd);
                }
                JournalEffector::apply_fd_renumber(&mut ctx, old_fd, new_fd)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                if original_fd != copied_fd {
                    stdout_fds.remove(&copied_fd);
                    stderr_fds.remove(&copied_fd);
                }
                if stdout_fds.contains(&original_fd) {
                    stdout_fds.insert(copied_fd);
                }
                if stderr_fds.contains(&original_fd) {
                    stderr_fds.insert(copied_fd);
                }
                JournalEffector::apply_fd_duplicate(&mut ctx, original_fd, copied_fd)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::CreateDirectoryV1 { fd, path } => {
                JournalEffector::apply_path_create_directory(&mut ctx, fd, &path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::PathSetTimesV1 {
                fd,
                flags,
                path,
                st_atim,
                st_mtim,
                fst_flags,
            } => {
                JournalEffector::apply_path_set_times(
                    &mut ctx, fd, flags, &path, st_atim, st_mtim, fst_flags,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => {
                JournalEffector::apply_fd_set_times(&mut ctx, fd, st_atim, st_mtim, fst_flags)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                JournalEffector::apply_fd_set_size(&mut ctx, fd, st_size)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                JournalEffector::apply_fd_set_flags(&mut ctx, fd, flags)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => {
                JournalEffector::apply_fd_set_rights(
                    &mut ctx,
                    fd,
                    fs_rights_base,
                    fs_rights_inheriting,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => {
                JournalEffector::apply_fd_advise(&mut ctx, fd, offset, len, advice)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => {
                JournalEffector::apply_fd_allocate(&mut ctx, fd, offset, len)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => {
                JournalEffector::apply_path_link(
                    &mut ctx, old_fd, old_flags, &old_path, new_fd, &new_path,
                )
                .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => {
                JournalEffector::apply_path_symlink(&mut ctx, &old_path, fd, &new_path)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::ChangeDirectoryV1 { path } => {
                JournalEffector::apply_chdir(&mut ctx, &path).map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::CreatePipeV1 { fd1, fd2 } => {
                JournalEffector::apply_fd_pipe(&mut ctx, fd1, fd2)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::EpollCreateV1 { fd } => {
                JournalEffector::apply_epoll_create(&mut ctx, fd)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => {
                JournalEffector::apply_epoll_ctl(&mut ctx, epfd, op, fd, event)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
            crate::journal::JournalEntry::TtySetV1 { tty, line_feeds } => {
                let state = crate::WasiTtyState {
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
                };

                if bootstrapping {
                    update_tty.replace(state);
                } else {
                    JournalEffector::apply_tty_set(&mut ctx, state)
                        .map_err(anyhow_err_to_runtime_err)?;
                }
            }
            crate::journal::JournalEntry::PortAddAddrV1 { cidr } => {
                JournalEffector::apply_port_addr_add(&mut ctx, cidr)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortDelAddrV1 { addr } => {
                JournalEffector::apply_port_addr_remove(&mut ctx, addr)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortAddrClearV1 => {
                JournalEffector::apply_port_addr_clear(&mut ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => JournalEffector::apply_port_bridge(&mut ctx, &network, &token, security)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::PortUnbridgeV1 => {
                JournalEffector::apply_port_unbridge(&mut ctx).map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortDhcpAcquireV1 => {
                JournalEffector::apply_port_dhcp_acquire(&mut ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortGatewaySetV1 { ip } => {
                JournalEffector::apply_port_gateway_set(&mut ctx, ip)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => JournalEffector::apply_port_route_add(
                &mut ctx,
                cidr,
                via_router,
                preferred_until,
                expires_at,
            )
            .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::PortRouteClearV1 => {
                JournalEffector::apply_port_route_clear(&mut ctx)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::PortRouteDelV1 { ip } => {
                JournalEffector::apply_port_route_remove(&mut ctx, ip)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketOpenV1 { af, ty, pt, fd } => {
                JournalEffector::apply_sock_open(&mut ctx, af, ty, pt, fd)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketListenV1 { fd, backlog } => {
                JournalEffector::apply_sock_listen(&mut ctx, fd, backlog as usize)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketBindV1 { fd, addr } => {
                JournalEffector::apply_sock_bind(&mut ctx, fd, addr)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => JournalEffector::apply_sock_connect(&mut ctx, fd, local_addr, peer_addr)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr: addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            } => JournalEffector::apply_sock_accepted(
                &mut ctx,
                listen_fd,
                fd,
                addr,
                peer_addr,
                fd_flags,
                nonblocking,
            )
            .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => JournalEffector::apply_sock_join_ipv4_multicast(&mut ctx, fd, multiaddr, iface)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEffector::apply_sock_join_ipv6_multicast(&mut ctx, fd, multiaddr, iface)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEffector::apply_sock_leave_ipv4_multicast(&mut ctx, fd, multiaddr, iface)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => JournalEffector::apply_sock_leave_ipv6_multicast(&mut ctx, fd, multiaddr, iface)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => JournalEffector::apply_sock_send_file(&mut ctx, socket_fd, file_fd, offset, count)
                .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => if is_64bit {
                JournalEffector::apply_sock_send_to::<Memory64>(&ctx, fd, data, flags, addr)
            } else {
                JournalEffector::apply_sock_send_to::<Memory32>(&ctx, fd, data, flags, addr)
            }
            .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => if is_64bit {
                JournalEffector::apply_sock_send::<Memory64>(&ctx, fd, data, flags)
            } else {
                JournalEffector::apply_sock_send::<Memory32>(&ctx, fd, data, flags)
            }
            .map_err(anyhow_err_to_runtime_err)?,
            crate::journal::JournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => {
                JournalEffector::apply_sock_set_opt_flag(&mut ctx, fd, opt, flag)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketSetOptSizeV1 { fd, opt, size } => {
                JournalEffector::apply_sock_set_opt_size(&mut ctx, fd, opt, size)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketSetOptTimeV1 { fd, ty, time } => {
                JournalEffector::apply_sock_set_opt_time(&mut ctx, fd, ty.into(), time)
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::SocketShutdownV1 { fd, how } => {
                JournalEffector::apply_sock_shutdown(&mut ctx, fd, how.into())
                    .map_err(anyhow_err_to_runtime_err)?
            }
            crate::journal::JournalEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => JournalEffector::apply_fd_event(&mut ctx, initial_val, flags, fd)
                .map_err(anyhow_err_to_runtime_err)?,
        }
    }

    // Check for events that are orphaned
    if !staging_kill_thread.is_empty() {
        tracing::debug!("Orphaned thread exit (missing snapshot) - it will be ignored");
    }
    if !staging_update_memory.is_empty() {
        tracing::debug!("Orphaned memory update (missing snapshot) - it will be ignored");
    }
    if !staging_spawn_threads.is_empty() {
        tracing::debug!("Orphaned thread spawn (missing snapshot) - it will be ignored");
    }
    if staging_rewind.is_some() {
        tracing::debug!("Orphaned main rewind (missing snapshot) - it will be ignored");
    }

    // If we are not in the same module then we fire off an exit
    // that simulates closing the process (hence keeps everything
    // in a clean state)
    if journal_module_hash.is_some() && Some(cur_module_hash) != journal_module_hash {
        tracing::error!(
            "The WASM module hash does not match the journal module hash (journal_hash={:x?} vs module_hash{:x?}) - forcing a restart",
            journal_module_hash.unwrap(),
            cur_module_hash
        );
        if bootstrapping {
            rewind = None;
            staging_rewind = None;
            spawn_threads.clear();
            staging_spawn_threads.clear();
            update_memory.clear();
            staging_update_memory.clear();
            update_tty.take();
            stdout.clear();
            stderr.clear();
            stdout_fds.clear();
            stderr_fds.clear();
            stdout_fds.insert(1 as WasiFd);
            stderr_fds.insert(2 as WasiFd);
        } else {
            JournalEffector::apply_process_exit(&mut ctx, None)
                .map_err(anyhow_err_to_runtime_err)?;
        }
    }

    // Now output the stdout and stderr
    for (offset, data, is_64bit) in stdout {
        if is_64bit {
            JournalEffector::apply_fd_write::<Memory64>(&ctx, 1, offset, data)
        } else {
            JournalEffector::apply_fd_write::<Memory32>(&ctx, 1, offset, data)
        }
        .map_err(anyhow_err_to_runtime_err)?;
    }

    for (offset, data, is_64bit) in stderr {
        if is_64bit {
            JournalEffector::apply_fd_write::<Memory64>(&ctx, 2, offset, data)
        } else {
            JournalEffector::apply_fd_write::<Memory32>(&ctx, 2, offset, data)
        }
        .map_err(anyhow_err_to_runtime_err)?;
    }

    // Next we apply all the memory updates that were delayed while the logs
    // were processed to completion.
    for (region, data) in update_memory {
        JournalEffector::apply_memory(&mut ctx, region.into(), &data)
            .map_err(anyhow_err_to_runtime_err)?;
    }

    if let Some(state) = update_tty {
        JournalEffector::apply_tty_set(&mut ctx, state).map_err(anyhow_err_to_runtime_err)?;
    }

    // If the main thread is not being restored then don't bother
    // attempting to restore the spawned threads either as the
    // main process is effectively in an exited state
    if rewind.is_none() {
        spawn_threads.clear();
    }

    // Spawn all the threads
    for (thread_id, thread_state) in spawn_threads {
        if thread_state.is_64bit {
            JournalEffector::apply_thread_state::<Memory64>(
                &mut ctx,
                thread_id,
                thread_state.memory_stack,
                thread_state.rewind_stack,
                thread_state.store_data,
                thread_state.start,
                thread_state.layout,
            )
            .map_err(anyhow_err_to_runtime_err)?;
        } else {
            JournalEffector::apply_thread_state::<Memory32>(
                &mut ctx,
                thread_id,
                thread_state.memory_stack,
                thread_state.rewind_stack,
                thread_state.store_data,
                thread_state.start,
                thread_state.layout,
            )
            .map_err(anyhow_err_to_runtime_err)?;
        }
    }

    Ok(rewind)
}
