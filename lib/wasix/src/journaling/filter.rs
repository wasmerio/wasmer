use futures::future::LocalBoxFuture;

use super::*;

/// Filters out a specific set of journal events and drops the rest, this
/// journal can be useful for restoring to a previous call point but
/// retaining the memory changes (e.g. WCGI runner).
pub struct FilteredJournal {
    inner: Box<DynJournal>,
    filter_memory: bool,
    filter_threads: bool,
    filter_files: bool,
    filter_chdir: bool,
    filter_clock: bool,
    filter_terminal: bool,
    filter_snapshots: bool,
    filter_descriptors: bool,
    filter_epoll: bool,
}

impl FilteredJournal {
    pub fn new(inner: Box<DynJournal>) -> Self {
        Self {
            inner,
            filter_memory: false,
            filter_threads: false,
            filter_files: false,
            filter_chdir: false,
            filter_clock: false,
            filter_terminal: false,
            filter_snapshots: false,
            filter_descriptors: false,
            filter_epoll: false,
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

    pub fn with_ignore_files(mut self, val: bool) -> Self {
        self.filter_files = val;
        self
    }

    pub fn with_ignore_chdir(mut self, val: bool) -> Self {
        self.filter_chdir = val;
        self
    }

    pub fn with_ignore_clock(mut self, val: bool) -> Self {
        self.filter_clock = val;
        self
    }

    pub fn with_ignore_epoll(mut self, val: bool) -> Self {
        self.filter_epoll = val;
        self
    }

    pub fn with_ignore_terminal(mut self, val: bool) -> Self {
        self.filter_terminal = val;
        self
    }

    pub fn with_ignore_snapshots(mut self, val: bool) -> Self {
        self.filter_snapshots = val;
        self
    }

    pub fn with_ignore_descriptors(mut self, val: bool) -> Self {
        self.filter_descriptors = val;
        self
    }
}

impl Journal for FilteredJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            let evt = match entry {
                JournalEntry::InitModule { .. } => {
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
                JournalEntry::ProcessExit { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::SetThread { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CloseThread { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorSeek { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorWrite { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::OpenFileDescriptor { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CloseFileDescriptor { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::RemoveDirectory { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::UnlinkFile { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::PathRename { .. } => {
                    if self.filter_files {
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
                JournalEntry::SetClockTime { .. } => {
                    if self.filter_clock {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::RenumberFileDescriptor { .. } => {
                    if self.filter_terminal {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::DuplicateFileDescriptor { .. } => {
                    if self.filter_terminal {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CreateDirectory { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::PathSetTimes { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorSetFlags { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorAdvise { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorAllocate { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }

                JournalEntry::FileDescriptorSetRights { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorSetTimes { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::FileDescriptorSetSize { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CreateHardLink { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CreateSymbolicLink { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::ChangeDirectory { .. } => {
                    if self.filter_chdir {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::EpollCreate { .. } => {
                    if self.filter_memory {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::EpollCtl { .. } => {
                    if self.filter_memory {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::TtySet { .. } => {
                    if self.filter_terminal {
                        return Ok(());
                    }
                    entry
                }
                JournalEntry::CreatePipe { .. } => {
                    if self.filter_files {
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
