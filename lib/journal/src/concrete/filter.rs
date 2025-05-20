use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

/// Filters out a specific set of journal events and drops the rest, this
/// journal can be useful for restoring to a previous call point but
/// retaining the memory changes (e.g. WCGI runner).
#[derive(Debug)]
pub struct FilteredJournal<W: WritableJournal, R: ReadableJournal> {
    tx: FilteredJournalTx<W>,
    rx: FilteredJournalRx<R>,
}

/// Represents what will be filtered by the filtering process
#[derive(Debug)]
struct FilteredJournalConfig {
    filter_memory: bool,
    filter_threads: bool,
    filter_fs: bool,
    filter_stdio: bool,
    filter_core: bool,
    filter_snapshots: bool,
    filter_net: bool,
    filter_events: Option<HashSet<usize>>,
    event_index: AtomicUsize,
}

impl Default for FilteredJournalConfig {
    fn default() -> Self {
        Self {
            filter_memory: false,
            filter_threads: false,
            filter_fs: false,
            filter_stdio: false,
            filter_core: false,
            filter_snapshots: false,
            filter_net: false,
            filter_events: None,
            event_index: AtomicUsize::new(0),
        }
    }
}

impl Clone for FilteredJournalConfig {
    fn clone(&self) -> Self {
        Self {
            filter_memory: self.filter_memory,
            filter_threads: self.filter_threads,
            filter_fs: self.filter_fs,
            filter_stdio: self.filter_stdio,
            filter_core: self.filter_core,
            filter_snapshots: self.filter_snapshots,
            filter_net: self.filter_net,
            filter_events: self.filter_events.clone(),
            event_index: AtomicUsize::new(self.event_index.load(Ordering::SeqCst)),
        }
    }
}

#[derive(Debug)]
pub struct FilteredJournalTx<W: WritableJournal> {
    inner: W,
    config: FilteredJournalConfig,
}

#[derive(Debug)]
pub struct FilteredJournalRx<R: ReadableJournal> {
    inner: R,
}

/// Constructs a filter with a set of parameters that will be filtered on
#[derive(Debug, Default)]
pub struct FilteredJournalBuilder {
    config: FilteredJournalConfig,
}

impl FilteredJournalBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build<J>(
        self,
        inner: J,
    ) -> FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>>
    where
        J: Journal,
    {
        FilteredJournal::new(inner, self.config)
    }

    pub fn build_split<W, R>(self, writer: W, reader: R) -> FilteredJournal<W, R>
    where
        W: WritableJournal,
        R: ReadableJournal,
    {
        FilteredJournal::new_split(writer, reader, self.config)
    }

    pub fn with_ignore_memory(mut self, val: bool) -> Self {
        self.config.filter_memory = val;
        self
    }

    pub fn with_ignore_threads(mut self, val: bool) -> Self {
        self.config.filter_threads = val;
        self
    }

    pub fn with_ignore_fs(mut self, val: bool) -> Self {
        self.config.filter_fs = val;
        self
    }

    pub fn with_ignore_stdio(mut self, val: bool) -> Self {
        self.config.filter_stdio = val;
        self
    }

    pub fn with_ignore_core(mut self, val: bool) -> Self {
        self.config.filter_core = val;
        self
    }

    pub fn with_ignore_snapshots(mut self, val: bool) -> Self {
        self.config.filter_snapshots = val;
        self
    }

    pub fn with_ignore_networking(mut self, val: bool) -> Self {
        self.config.filter_net = val;
        self
    }

    pub fn with_filter_events(mut self, events: HashSet<usize>) -> Self {
        self.config.filter_events = Some(events);
        self
    }

    pub fn add_event_to_whitelist(&mut self, event_index: usize) {
        if let Some(filter) = self.config.filter_events.as_mut() {
            filter.insert(event_index);
        }
    }

    pub fn set_ignore_memory(&mut self, val: bool) -> &mut Self {
        self.config.filter_memory = val;
        self
    }

    pub fn set_ignore_threads(&mut self, val: bool) -> &mut Self {
        self.config.filter_threads = val;
        self
    }

    pub fn set_ignore_fs(&mut self, val: bool) -> &mut Self {
        self.config.filter_fs = val;
        self
    }

    pub fn set_ignore_stdio(&mut self, val: bool) -> &mut Self {
        self.config.filter_stdio = val;
        self
    }

    pub fn set_ignore_core(&mut self, val: bool) -> &mut Self {
        self.config.filter_core = val;
        self
    }

    pub fn set_ignore_snapshots(&mut self, val: bool) -> &mut Self {
        self.config.filter_snapshots = val;
        self
    }

    pub fn set_ignore_networking(&mut self, val: bool) -> &mut Self {
        self.config.filter_net = val;
        self
    }
}

impl FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
    fn new<J>(inner: J, config: FilteredJournalConfig) -> Self
    where
        J: Journal,
    {
        let (tx, rx) = inner.split();
        Self {
            tx: FilteredJournalTx { inner: tx, config },
            rx: FilteredJournalRx { inner: rx },
        }
    }

    pub fn clone_with_inner<J>(&self, inner: J) -> Self
    where
        J: Journal,
    {
        let config = self.tx.config.clone();
        let (tx, rx) = inner.split();
        Self {
            tx: FilteredJournalTx { config, inner: tx },
            rx: FilteredJournalRx { inner: rx },
        }
    }
}

impl<W: WritableJournal, R: ReadableJournal> FilteredJournal<W, R> {
    pub fn into_inner(self) -> RecombinedJournal<W, R> {
        RecombinedJournal::new(self.tx.inner, self.rx.inner)
    }

    fn new_split(writer: W, reader: R, config: FilteredJournalConfig) -> Self {
        Self {
            tx: FilteredJournalTx {
                inner: writer,
                config,
            },
            rx: FilteredJournalRx { inner: reader },
        }
    }
}

impl<W: WritableJournal> WritableJournal for FilteredJournalTx<W> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let event_index = self.config.event_index.fetch_add(1, Ordering::SeqCst);
        if let Some(events) = self.config.filter_events.as_ref() {
            if !events.contains(&event_index) {
                return Ok(LogWriteResult {
                    record_start: 0,
                    record_end: 0,
                });
            }
        }

        let evt = match entry {
            JournalEntry::SetClockTimeV1 { .. }
            | JournalEntry::InitModuleV1 { .. }
            | JournalEntry::ProcessExitV1 { .. }
            | JournalEntry::EpollCreateV1 { .. }
            | JournalEntry::EpollCtlV1 { .. }
            | JournalEntry::TtySetV1 { .. } => {
                if self.config.filter_core {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::ClearEtherealV1 => entry,
            JournalEntry::SetThreadV1 { .. } | JournalEntry::CloseThreadV1 { .. } => {
                if self.config.filter_threads {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::UpdateMemoryRegionV1 { .. } => {
                if self.config.filter_memory {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::FileDescriptorSeekV1 { fd, .. }
            | JournalEntry::FileDescriptorWriteV1 { fd, .. }
            | JournalEntry::OpenFileDescriptorV1 { fd, .. }
            | JournalEntry::OpenFileDescriptorV2 { fd, .. }
            | JournalEntry::CloseFileDescriptorV1 { fd, .. }
            | JournalEntry::RenumberFileDescriptorV1 { old_fd: fd, .. }
            | JournalEntry::DuplicateFileDescriptorV1 {
                original_fd: fd, ..
            }
            | JournalEntry::DuplicateFileDescriptorV2 {
                original_fd: fd, ..
            }
            | JournalEntry::FileDescriptorSetFdFlagsV1 { fd, .. }
            | JournalEntry::FileDescriptorSetFlagsV1 { fd, .. }
            | JournalEntry::FileDescriptorAdviseV1 { fd, .. }
            | JournalEntry::FileDescriptorAllocateV1 { fd, .. }
            | JournalEntry::FileDescriptorSetRightsV1 { fd, .. }
            | JournalEntry::FileDescriptorSetTimesV1 { fd, .. }
            | JournalEntry::FileDescriptorSetSizeV1 { fd, .. } => {
                if self.config.filter_stdio && fd <= 2 {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                if self.config.filter_fs {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::RemoveDirectoryV1 { .. }
            | JournalEntry::UnlinkFileV1 { .. }
            | JournalEntry::PathRenameV1 { .. }
            | JournalEntry::CreateDirectoryV1 { .. }
            | JournalEntry::PathSetTimesV1 { .. }
            | JournalEntry::CreateHardLinkV1 { .. }
            | JournalEntry::CreateSymbolicLinkV1 { .. }
            | JournalEntry::ChangeDirectoryV1 { .. }
            | JournalEntry::CreatePipeV1 { .. }
            | JournalEntry::CreateEventV1 { .. } => {
                if self.config.filter_fs {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::SnapshotV1 { .. } => {
                if self.config.filter_snapshots {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
            JournalEntry::PortAddAddrV1 { .. }
            | JournalEntry::PortDelAddrV1 { .. }
            | JournalEntry::PortAddrClearV1
            | JournalEntry::PortBridgeV1 { .. }
            | JournalEntry::PortUnbridgeV1
            | JournalEntry::PortDhcpAcquireV1
            | JournalEntry::PortGatewaySetV1 { .. }
            | JournalEntry::PortRouteAddV1 { .. }
            | JournalEntry::PortRouteClearV1
            | JournalEntry::PortRouteDelV1 { .. }
            | JournalEntry::SocketOpenV1 { .. }
            | JournalEntry::SocketPairV1 { .. }
            | JournalEntry::SocketListenV1 { .. }
            | JournalEntry::SocketBindV1 { .. }
            | JournalEntry::SocketConnectedV1 { .. }
            | JournalEntry::SocketAcceptedV1 { .. }
            | JournalEntry::SocketJoinIpv4MulticastV1 { .. }
            | JournalEntry::SocketJoinIpv6MulticastV1 { .. }
            | JournalEntry::SocketLeaveIpv4MulticastV1 { .. }
            | JournalEntry::SocketLeaveIpv6MulticastV1 { .. }
            | JournalEntry::SocketSendFileV1 { .. }
            | JournalEntry::SocketSendToV1 { .. }
            | JournalEntry::SocketSendV1 { .. }
            | JournalEntry::SocketSetOptFlagV1 { .. }
            | JournalEntry::SocketSetOptSizeV1 { .. }
            | JournalEntry::SocketSetOptTimeV1 { .. }
            | JournalEntry::SocketShutdownV1 { .. } => {
                if self.config.filter_net {
                    return Ok(LogWriteResult {
                        record_start: 0,
                        record_end: 0,
                    });
                }
                entry
            }
        };
        self.inner.write(evt)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.inner.flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        self.inner.commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.inner.rollback()
    }
}

impl<R: ReadableJournal> ReadableJournal for FilteredJournalRx<R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(FilteredJournalRx {
            inner: self.inner.as_restarted()?,
        }))
    }
}

impl<W: WritableJournal, R: ReadableJournal> WritableJournal for FilteredJournal<W, R> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        self.tx.commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.tx.rollback()
    }
}

impl<W: WritableJournal, R: ReadableJournal> ReadableJournal for FilteredJournal<W, R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
