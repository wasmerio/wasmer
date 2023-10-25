use serde::{Deserialize, Serialize};
use std::{
    io::{self, ErrorKind, SeekFrom},
    mem::MaybeUninit,
    path::Path,
    time::SystemTime,
};
use tokio::runtime::Handle;
use wasmer_wasix_types::wasi::ExitCode;

use futures::future::LocalBoxFuture;
use virtual_fs::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, Fd};

use crate::WasiThreadId;

use super::*;

/// The snapshot log entries are serializable which
/// allows them to be written directly to a file
///
/// Note: This structure is versioned which allows for
/// changes to the log entry types without having to
/// worry about backward and forward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotLogEntry {
    InitV1 {
        wasm_hash: [u8; 32],
    },
    TerminalDataV1 {
        data: Vec<u8>,
    },
    UpdateMemoryRegionV1 {
        start: u64,
        end: u64,
        data: Vec<u8>,
    },
    CloseThreadV1 {
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    },
    SetThreadV1 {
        id: WasiThreadId,
        call_stack: Vec<u8>,
        memory_stack: Vec<u8>,
        store_data: Vec<u8>,
        is_64bit: bool,
    },
    CloseFileDescriptorV1 {
        fd: Fd,
    },
    OpenFileDescriptorV1 {
        fd: Fd,
        state: FdSnapshot<'static>,
    },
    RemoveFileSystemEntryV1 {
        path: String,
    },
    UpdateFileSystemEntryV1 {
        path: String,
        ft: FileEntryType,
        accessed: u64,
        created: u64,
        modified: u64,
        len: u64,
        data: Vec<u8>,
    },
    SnapshotV1 {
        when: SystemTime,
        trigger: SnapshotTrigger,
    },
}

impl<'a> From<SnapshotLog<'a>> for SnapshotLogEntry {
    fn from(value: SnapshotLog<'a>) -> Self {
        match value {
            SnapshotLog::Init { wasm_hash } => Self::InitV1 { wasm_hash },
            SnapshotLog::TerminalData { data } => Self::TerminalDataV1 {
                data: data.into_owned(),
            },
            SnapshotLog::UpdateMemoryRegion { region, data } => Self::UpdateMemoryRegionV1 {
                start: region.start,
                end: region.end,
                data: data.into_owned(),
            },
            SnapshotLog::CloseThread { id, exit_code } => Self::CloseThreadV1 { id, exit_code },
            SnapshotLog::SetThread {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThreadV1 {
                id,
                call_stack: call_stack.into_owned(),
                memory_stack: memory_stack.into_owned(),
                store_data: store_data.into_owned(),
                is_64bit,
            },
            SnapshotLog::CloseFileDescriptor { fd } => Self::CloseFileDescriptorV1 { fd },
            SnapshotLog::OpenFileDescriptor { fd, state } => Self::OpenFileDescriptorV1 {
                fd,
                state: state.into_owned(),
            },
            SnapshotLog::RemoveFileSystemEntry { path } => Self::RemoveFileSystemEntryV1 {
                path: path.into_owned(),
            },
            SnapshotLog::UpdateFileSystemEntry {
                path,
                ft,
                accessed,
                created,
                modified,
                len,
                data,
            } => Self::UpdateFileSystemEntryV1 {
                path: path.into_owned(),
                ft,
                accessed,
                created,
                modified,
                len,
                data: data.into_owned(),
            },
            SnapshotLog::Snapshot { when, trigger } => Self::SnapshotV1 { when, trigger },
        }
    }
}

impl<'a> From<SnapshotLogEntry> for SnapshotLog<'a> {
    fn from(value: SnapshotLogEntry) -> Self {
        match value {
            SnapshotLogEntry::InitV1 { wasm_hash } => Self::Init { wasm_hash },
            SnapshotLogEntry::TerminalDataV1 { data } => Self::TerminalData { data: data.into() },
            SnapshotLogEntry::UpdateMemoryRegionV1 { start, end, data } => {
                Self::UpdateMemoryRegion {
                    region: start..end,
                    data: data.into(),
                }
            }
            SnapshotLogEntry::CloseThreadV1 { id, exit_code } => {
                Self::CloseThread { id, exit_code }
            }
            SnapshotLogEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => Self::SetThread {
                id: id,
                call_stack: call_stack.into(),
                memory_stack: memory_stack.into(),
                store_data: store_data.into(),
                is_64bit,
            },
            SnapshotLogEntry::CloseFileDescriptorV1 { fd } => Self::CloseFileDescriptor { fd },
            SnapshotLogEntry::OpenFileDescriptorV1 { fd, state } => Self::OpenFileDescriptor {
                fd,
                state: state.clone(),
            },
            SnapshotLogEntry::RemoveFileSystemEntryV1 { path } => {
                Self::RemoveFileSystemEntry { path: path.into() }
            }
            SnapshotLogEntry::UpdateFileSystemEntryV1 {
                path,
                ft,
                accessed,
                created,
                modified,
                len,
                data,
            } => Self::UpdateFileSystemEntry {
                path: path.into(),
                ft: ft.clone(),
                accessed,
                created,
                modified,
                len,
                data: data.into(),
            },
            SnapshotLogEntry::SnapshotV1 { when, trigger } => Self::Snapshot { when, trigger },
        }
    }
}

struct State {
    file: tokio::fs::File,
    at_end: bool,
}

/// The LogFile snapshot capturer will write its snapshots to a linear journal
/// and read them when restoring. It uses the `bincode` serializer which
/// means that forwards and backwards compatibility must be dealt with
/// carefully.
///
/// When opening an existing journal file that was previously saved
/// then new entries will be added to the end regardless of if
/// its been read.
///
/// The logfile snapshot capturer uses a 64bit number as a entry encoding
/// delimiter.
pub struct LogFileSnapshotCapturer {
    state: tokio::sync::Mutex<State>,
    handle: Handle,
}

impl LogFileSnapshotCapturer {
    pub async fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let state = State {
            file: tokio::fs::File::options()
                .read(true)
                .write(true)
                .create(true)
                .open(path)
                .await?,
            at_end: false,
        };
        Ok(Self {
            state: tokio::sync::Mutex::new(state),
            handle: Handle::current(),
        })
    }

    pub fn new_std(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let state = State {
            file: tokio::fs::File::from_std(file),
            at_end: false,
        };
        Ok(Self {
            state: tokio::sync::Mutex::new(state),
            handle: Handle::current(),
        })
    }
}

#[async_trait::async_trait]
impl SnapshotCapturer for LogFileSnapshotCapturer {
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        tracing::debug!("snapshot event: {:?}", entry);
        Box::pin(async {
            let entry: SnapshotLogEntry = entry.into();

            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut state = self.state.lock().await;
            if state.at_end == false {
                state.file.seek(SeekFrom::End(0)).await?;
                state.at_end = true;
            }

            let data = bincode::serialize(&entry)?;
            let data_len = data.len() as u64;
            let data_len = data_len.to_ne_bytes();

            state.file.write_all(&data_len).await?;
            state.file.write_all(&data).await?;
            Ok(())
        })
    }

    /// UNSAFE: This method uses unsafe operations to remove the need to zero
    /// the buffer before its read the log entries into it
    fn read<'a>(&'a self) -> LocalBoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>> {
        Box::pin(async {
            let mut state = self.state.lock().await;

            let mut data_len = [0u8; 8];
            match state.file.read_exact(&mut data_len).await {
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
                Err(err) => return Err(err.into()),
                Ok(_) => {}
            }

            let data_len = u64::from_ne_bytes(data_len);
            let mut data = Vec::with_capacity(data_len as usize);
            let data_unsafe: &mut [MaybeUninit<u8>] = data.spare_capacity_mut();
            let data_unsafe: &mut [u8] = unsafe { std::mem::transmute(data_unsafe) };
            state.file.read_exact(data_unsafe).await?;
            unsafe {
                data.set_len(data_len as usize);
            }

            let entry: SnapshotLogEntry = bincode::deserialize(&data)?;
            Ok(Some(entry.into()))
        })
    }
}
