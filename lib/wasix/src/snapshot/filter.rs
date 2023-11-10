use futures::future::LocalBoxFuture;

use super::*;

/// Filters out a specific set of log events and drops the rest, this
/// capturer can be useful for restoring to a previous call point but
/// retaining the memory changes (e.g. WCGI runner).
pub struct FilteredSnapshotCapturer {
    inner: Box<DynSnapshotCapturer>,
    filter_memory: bool,
    filter_threads: bool,
    filter_files: bool,
    filter_terminal: bool,
    filter_snapshots: bool,
    filter_descriptors: bool,
}

impl FilteredSnapshotCapturer {
    pub fn new(inner: Box<DynSnapshotCapturer>) -> Self {
        Self {
            inner,
            filter_memory: false,
            filter_threads: false,
            filter_files: false,
            filter_terminal: false,
            filter_snapshots: false,
            filter_descriptors: false,
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

impl SnapshotCapturer for FilteredSnapshotCapturer {
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            let evt = match entry {
                SnapshotLog::Init { wasm_hash } => SnapshotLog::Init { wasm_hash },
                SnapshotLog::FileDescriptorWrite { .. } => {
                    if self.filter_terminal {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::UpdateMemoryRegion { .. } => {
                    if self.filter_memory {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::CloseThread { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::SetThread { .. } => {
                    if self.filter_threads {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::CloseFileDescriptor { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::OpenFileDescriptor { .. } => {
                    if self.filter_descriptors {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::RemoveDirectory { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::UnlinkFile { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::PathRename { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::UpdateFileSystemEntry { .. } => {
                    if self.filter_files {
                        return Ok(());
                    }
                    entry
                }
                SnapshotLog::Snapshot { .. } => {
                    if self.filter_snapshots {
                        return Ok(());
                    }
                    entry
                }
            };
            self.inner.write(evt).await
        })
    }

    fn read<'a>(&'a self) -> LocalBoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>> {
        Box::pin(async { self.inner.read().await })
    }
}
