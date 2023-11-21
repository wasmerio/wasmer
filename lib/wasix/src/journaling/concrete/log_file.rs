use bytes::Buf;
use shared_buffer::OwnedBuffer;
use std::{
    io::{Seek, SeekFrom},
    path::Path,
};
use tokio::runtime::Handle;
use virtual_fs::AsyncWriteExt;

use futures::future::LocalBoxFuture;

use super::*;

struct State {
    file: tokio::fs::File,
    buffer_pos: usize,
    record_pos: usize,
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
pub struct LogFileJournal {
    state: std::sync::Mutex<State>,
    handle: Handle,
    buffer: OwnedBuffer,
}

impl LogFileJournal {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let buffer = OwnedBuffer::from_file(&file)?;

        // Move the file to the end
        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            state: std::sync::Mutex::new(State {
                file: tokio::fs::File::from_std(file),
                buffer_pos: 0,
                record_pos: 0,
            }),
            handle: Handle::current(),
            buffer,
        })
    }
}

impl JournalBatch {}

#[async_trait::async_trait]
impl Journal for LogFileJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        tracing::debug!("journal event: {:?}", entry);
        Box::pin(async {
            // Create the batch
            let batch = JournalBatch {
                records: vec![entry.into()],
            };

            let data = rkyv::to_bytes::<_, 128>(&batch).unwrap();
            let data_len = data.len() as u64;
            let data_len = data_len.to_be_bytes();

            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut state = self.state.lock().unwrap();
            state.file.write_all(&data_len).await?;
            state.file.write_all(&data).await?;
            Ok(())
        })
    }

    /// UNSAFE: This method uses unsafe operations to remove the need to zero
    /// the buffer before its read the log entries into it
    fn read<'a>(&'a self) -> anyhow::Result<Option<JournalEntry<'a>>> {
        let mut state = self.state.lock().unwrap();

        // Get a memory reference to the data on the disk at
        // the current read location
        let mut buffer_ptr = self.buffer.as_ref();

        // First we read the magic number for the archive
        if state.buffer_pos == 0 {
            if buffer_ptr.len() >= 8 {
                let magic = u64::from_be_bytes(buffer_ptr[0..8].try_into().unwrap());
                if magic != JOURNAL_MAGIC_NUMBER {
                    return Err(anyhow::format_err!(
                        "invalid magic number of journal ({} vs {})",
                        magic,
                        JOURNAL_MAGIC_NUMBER
                    ));
                }
                state.buffer_pos += 8;
            } else {
                return Ok(None);
            }
        }
        buffer_ptr.advance(state.buffer_pos);

        loop {
            // Next we read the length of the current batch
            if buffer_ptr.len() < 8 {
                return Ok(None);
            }
            let batch_len = u64::from_be_bytes(buffer_ptr[0..8].try_into().unwrap()) as usize;
            buffer_ptr.advance(8);
            if batch_len == 0 || batch_len > buffer_ptr.len() {
                return Ok(None);
            }

            // Read the batch data itself
            let batch = &buffer_ptr[..batch_len];
            let batch: &ArchivedJournalBatch =
                unsafe { rkyv::archived_unsized_root::<JournalBatch>(batch) };

            // If we have reached the end then move onto the next batch
            if state.record_pos >= batch.records.len() {
                buffer_ptr.advance(batch_len);
                state.buffer_pos += batch_len;
                state.record_pos = 0;
                continue;
            }
            let record = &batch.records[state.record_pos];

            // Otherwise we return the record and advance
            state.record_pos += 1;
            return Ok(Some(record.into()));
        }
    }
}
