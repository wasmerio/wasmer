use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
};

use derivative::Derivative;

use super::*;

/// Filters out a specific set of journal events and drops the rest, this
/// journal can be useful for restoring to a previous call point but
/// retaining the memory changes (e.g. WCGI runner).
#[derive(Debug)]
pub struct FilteredJournal {
    tx: FilteredJournalTx,
    rx: FilteredJournalRx,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct FilteredJournalTx {
    #[derivative(Debug = "ignore")]
    inner: Box<DynWritableJournal>,
    filter_memory: bool,
    filter_threads: bool,
    filter_fs: bool,
    filter_core: bool,
    filter_snapshots: bool,
    filter_net: bool,
    filter_events: Option<HashSet<usize>>,
    event_index: AtomicUsize,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct FilteredJournalRx {
    #[derivative(Debug = "ignore")]
    inner: Box<DynReadableJournal>,
}

impl FilteredJournal {
    pub fn new<J>(inner: J) -> Self
    where
        J: Journal,
    {
        let (tx, rx) = inner.split();
        Self {
            tx: FilteredJournalTx {
                inner: tx,
                filter_memory: false,
                filter_threads: false,
                filter_fs: false,
                filter_core: false,
                filter_snapshots: false,
                filter_net: false,
                filter_events: None,
                event_index: AtomicUsize::new(0),
            },
            rx: FilteredJournalRx { inner: rx },
        }
    }

    pub fn with_ignore_memory(mut self, val: bool) -> Self {
        self.tx.filter_memory = val;
        self
    }

    pub fn with_ignore_threads(mut self, val: bool) -> Self {
        self.tx.filter_threads = val;
        self
    }

    pub fn with_ignore_fs(mut self, val: bool) -> Self {
        self.tx.filter_fs = val;
        self
    }

    pub fn with_ignore_core(mut self, val: bool) -> Self {
        self.tx.filter_core = val;
        self
    }

    pub fn with_ignore_snapshots(mut self, val: bool) -> Self {
        self.tx.filter_snapshots = val;
        self
    }

    pub fn with_ignore_networking(mut self, val: bool) -> Self {
        self.tx.filter_net = val;
        self
    }

    pub fn with_filter_events(mut self, events: HashSet<usize>) -> Self {
        self.tx.filter_events = Some(events);
        self
    }

    pub fn add_event_to_whitelist(&mut self, event_index: usize) {
        if let Some(filter) = self.tx.filter_events.as_mut() {
            filter.insert(event_index);
        }
    }

    pub fn into_inner(self) -> RecombinedJournal {
        RecombinedJournal::new(self.tx.inner, self.rx.inner)
    }
}

impl WritableJournal for FilteredJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        let event_index = self.event_index.fetch_add(1, Ordering::SeqCst);
        if let Some(events) = self.filter_events.as_ref() {
            if !events.contains(&event_index) {
                return Ok(0);
            }
        }

        let evt = match entry {
            JournalEntry::SetClockTime { .. }
            | JournalEntry::InitModule { .. }
            | JournalEntry::ProcessExit { .. }
            | JournalEntry::EpollCreate { .. }
            | JournalEntry::EpollCtl { .. }
            | JournalEntry::TtySet { .. } => {
                if self.filter_core {
                    return Ok(0);
                }
                entry
            }
            JournalEntry::SetThread { .. } | JournalEntry::CloseThread { .. } => {
                if self.filter_threads {
                    return Ok(0);
                }
                entry
            }
            JournalEntry::UpdateMemoryRegion { .. } => {
                if self.filter_memory {
                    return Ok(0);
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
                    return Ok(0);
                }
                entry
            }
            JournalEntry::Snapshot { .. } => {
                if self.filter_snapshots {
                    return Ok(0);
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
                    return Ok(0);
                }
                entry
            }
        };
        self.inner.write(evt)
    }
}

impl ReadableJournal for FilteredJournalRx {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(FilteredJournalRx {
            inner: self.inner.as_restarted()?,
        }))
    }
}

impl WritableJournal for FilteredJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for FilteredJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for FilteredJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
