use futures::future::LocalBoxFuture;

use super::*;

/// Filters out a specific set of journal events and drops the rest, this
/// journal can be useful for restoring to a previous call point but
/// retaining the memory changes (e.g. WCGI runner).
pub struct FilteredJournal {
    inner: Box<DynJournal>,
    filter_memory: bool,
    filter_threads: bool,
    filter_fs: bool,
    filter_core: bool,
    filter_snapshots: bool,
    filter_net: bool,
}

impl FilteredJournal {
    pub fn new(inner: Box<DynJournal>) -> Self {
        Self {
            inner,
            filter_memory: false,
            filter_threads: false,
            filter_fs: false,
            filter_core: false,
            filter_snapshots: false,
            filter_net: false,
        }
    }

    pub fn with_ignore_memory(mut self, val: bool) -> Self {
        self.filter_memory = val;
        self
    }

    pub fn with_ignore_threads(mut self, val: bool) -> Self {
        self.filter_threads = val;
        self
    }

    pub fn with_ignore_fs(mut self, val: bool) -> Self {
        self.filter_fs = val;
        self
    }

    pub fn with_ignore_core(mut self, val: bool) -> Self {
        self.filter_core = val;
        self
    }

    pub fn with_ignore_snapshots(mut self, val: bool) -> Self {
        self.filter_snapshots = val;
        self
    }

    pub fn with_ignore_networking(mut self, val: bool) -> Self {
        self.filter_net = val;
        self
    }
}

impl Journal for FilteredJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            let evt = match entry {
                JournalEntry::SetClockTime { .. }
                | JournalEntry::InitModule { .. }
                | JournalEntry::ProcessExit { .. }
                | JournalEntry::EpollCreate { .. }
                | JournalEntry::EpollCtl { .. }
                | JournalEntry::TtySet { .. } => {
                    if self.filter_core {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::SetThread { .. } | JournalEntry::CloseThread { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::UpdateMemoryRegion { .. } => {
                    if self.filter_memory {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorSeek { .. }
                | JournalEntry::FileDescriptorWrite { .. }
                | JournalEntry::OpenFileDescriptor { .. }
                | JournalEntry::CloseFileDescriptor { .. }
                | JournalEntry::RemoveDirectory { .. }
                | JournalEntry::UnlinkFile { .. }
                | JournalEntry::PathRename { .. }
                | JournalEntry::RenumberFileDescriptor { .. }
                | JournalEntry::DuplicateFileDescriptor { .. }
                | JournalEntry::CreateDirectory { .. }
                | JournalEntry::PathSetTimes { .. }
                | JournalEntry::FileDescriptorSetFlags { .. }
                | JournalEntry::FileDescriptorAdvise { .. }
                | JournalEntry::FileDescriptorAllocate { .. }
                | JournalEntry::FileDescriptorSetRights { .. }
                | JournalEntry::FileDescriptorSetTimes { .. }
                | JournalEntry::FileDescriptorSetSize { .. }
                | JournalEntry::CreateHardLink { .. }
                | JournalEntry::CreateSymbolicLink { .. }
                | JournalEntry::ChangeDirectory { .. }
                | JournalEntry::CreatePipe { .. }
                | JournalEntry::CreateEvent { .. } => {
                    if self.filter_fs {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::Snapshot { .. } => {
                    if self.filter_snapshots {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::PortAddAddr { .. }
                | JournalEntry::PortDelAddr { .. }
                | JournalEntry::PortAddrClear
                | JournalEntry::PortBridge { .. }
                | JournalEntry::PortUnbridge
                | JournalEntry::PortDhcpAcquire
                | JournalEntry::PortGatewaySet { .. }
                | JournalEntry::PortRouteAdd { .. }
                | JournalEntry::PortRouteClear
                | JournalEntry::PortRouteDel { .. }
                | JournalEntry::SocketOpen { .. }
                | JournalEntry::SocketListen { .. }
                | JournalEntry::SocketBind { .. }
                | JournalEntry::SocketConnected { .. }
                | JournalEntry::SocketAccepted { .. }
                | JournalEntry::SocketJoinIpv4Multicast { .. }
                | JournalEntry::SocketJoinIpv6Multicast { .. }
                | JournalEntry::SocketLeaveIpv4Multicast { .. }
                | JournalEntry::SocketLeaveIpv6Multicast { .. }
                | JournalEntry::SocketSendFile { .. }
                | JournalEntry::SocketSendTo { .. }
                | JournalEntry::SocketSend { .. }
                | JournalEntry::SocketSetOptFlag { .. }
                | JournalEntry::SocketSetOptSize { .. }
                | JournalEntry::SocketSetOptTime { .. }
                | JournalEntry::SocketShutdown { .. } => {
                    if self.filter_net {
                        return Ok(());
                    }
                    entry
                }
            };
            self.inner.write(evt).await
        })
    }

    fn read(&self) -> LocalBoxFuture<'_, anyhow::Result<Option<JournalEntry<'_>>>> {
        Box::pin(async { self.inner.read().await })
    }
}
