use bytes::Buf;
use rkyv::{
    api::high::HighSerializer,
    rancor::Strategy,
    ser::{
        allocator::{Arena, ArenaHandle},
        sharing::Share,
        writer::IoWriter,
        Positional, Serializer, Writer,
    },
};
use shared_buffer::OwnedBuffer;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use virtual_fs::mem_fs::OffloadBackingStore;

use super::*;

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
#[derive(Debug)]
pub struct LogFileJournal {
    tx: LogFileJournalTx,
    rx: LogFileJournalRx,
}

struct TxState {
    /// The original handle to the file
    underlying_file: File,

    /// A modified handle to the original underlying file
    file: File,

    /// The arena necessary for serialization
    arena: Arena,

    /// The latest position in the file the serializator got to
    pos: usize,
}

impl TxState {
    fn get_serializer(&mut self) -> Serializer<IoWriter<&File>, ArenaHandle<'_>, Share> {
        self.get_serializer_with_pos(self.pos)
    }

    fn get_serializer_with_pos(
        &mut self,
        pos: usize,
    ) -> Serializer<IoWriter<&File>, ArenaHandle<'_>, Share> {
        Serializer::new(
            IoWriter::with_pos(&self.file, pos),
            self.arena.acquire(),
            Share::new(),
        )
    }

    fn to_high<'a>(
        serializer: &'a mut Serializer<IoWriter<&'a File>, ArenaHandle<'a>, Share>,
    ) -> &'a mut HighSerializer<IoWriter<&'a File>, ArenaHandle<'a>, rkyv::rancor::Error> {
        Strategy::wrap(serializer)
    }
}

impl std::fmt::Debug for TxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TxState")
            .field("file", &self.underlying_file)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct LogFileJournalTx {
    state: Arc<Mutex<TxState>>,
}

#[derive(Debug)]
pub struct LogFileJournalRx {
    tx: Option<LogFileJournalTx>,
    buffer_pos: Mutex<usize>,
    buffer: OwnedBuffer,
    store: OffloadBackingStore,
}

impl LogFileJournalRx {
    pub fn owned_buffer(&self) -> OwnedBuffer {
        self.store.owned_buffer().clone()
    }

    pub fn backing_store(&self) -> OffloadBackingStore {
        self.store.clone()
    }
}

impl LogFileJournalTx {
    pub fn as_rx(&self) -> anyhow::Result<LogFileJournalRx> {
        let state = self.state.lock().unwrap();
        let file = state.underlying_file.try_clone()?;

        let store = OffloadBackingStore::from_file(&file);
        let buffer = store.owned_buffer();

        // If the buffer exists we valid the magic number
        let mut buffer_pos = 0;
        let mut buffer_ptr = buffer.as_ref();
        if buffer_ptr.len() >= 8 {
            let magic = u64::from_be_bytes(buffer_ptr[0..8].try_into().unwrap());
            if magic != JOURNAL_MAGIC_NUMBER {
                return Err(anyhow::format_err!(
                    "invalid magic number of journal ({} vs {})",
                    magic,
                    JOURNAL_MAGIC_NUMBER
                ));
            }
            buffer_ptr.advance(8);
            buffer_pos += 8;
        } else {
            tracing::trace!("journal has no magic (could be empty?)");
        }

        Ok(LogFileJournalRx {
            tx: Some(self.clone()),
            buffer_pos: Mutex::new(buffer_pos),
            buffer,
            store,
        })
    }
}

impl LogFileJournal {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Self::from_file(file)
    }

    pub fn new_readonly(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file = std::fs::File::options().read(true).open(path)?;
        Self::from_file(file)
    }

    pub fn owned_buffer(&self) -> OwnedBuffer {
        self.rx.owned_buffer()
    }

    pub fn backing_store(&self) -> OffloadBackingStore {
        self.rx.backing_store()
    }

    /// Create a new journal from a file
    pub fn from_file(mut file: std::fs::File) -> anyhow::Result<Self> {
        // Move to the end of the file and write the
        // magic if one is needed
        let underlying_file = file.try_clone()?;
        let arena = Arena::new();

        let end_pos = file.seek(SeekFrom::End(0))?;

        let mut tx = TxState {
            underlying_file,
            arena,
            file,
            pos: end_pos as usize,
        };

        let mut serializer = tx.get_serializer();
        let serializer = TxState::to_high(&mut serializer);

        if serializer.pos() == 0 {
            let magic = JOURNAL_MAGIC_NUMBER;
            let magic = magic.to_be_bytes();
            serializer.write(&magic)?;
        }

        let last_pos = serializer.pos();
        let _ = serializer;

        tx.arena.shrink();
        tx.pos = last_pos;

        // Create the tx
        let tx = LogFileJournalTx {
            state: Arc::new(Mutex::new(tx)),
        };

        // First we create the readable journal
        let rx = tx.as_rx()?;

        Ok(Self { rx, tx })
    }

    /// Create a new journal from a buffer
    pub fn from_buffer(
        buffer: OwnedBuffer,
    ) -> RecombinedJournal<UnsupportedJournal, LogFileJournalRx> {
        // Create the rx
        let rx = LogFileJournalRx {
            tx: None,
            buffer_pos: Mutex::new(0),
            buffer: buffer.clone(),
            store: OffloadBackingStore::from_buffer(buffer),
        };

        // Create an unsupported write journal
        let tx = UnsupportedJournal::default();

        // Now recombine
        RecombinedJournal::new(tx, rx)
    }
}

impl WritableJournal for LogFileJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        tracing::debug!("journal event: {:?}", entry);

        let mut state = self.state.lock().unwrap();

        // Write the header (with a record size of zero)
        let record_type: JournalEntryRecordType = entry.archive_record_type();
        let mut serializer = state.get_serializer();
        let serializer = TxState::to_high(&mut serializer);
        let offset_header = serializer.pos() as u64;
        tracing::trace!("serpos is {offset_header}");
        serializer.write(&[0u8; 8])?;

        // Now serialize the actual data to the log
        let offset_start = serializer.pos() as u64;
        entry.serialize_archive(serializer)?;
        let offset_end = serializer.pos() as u64;
        let record_size = offset_end - offset_start;
        tracing::trace!(
            "delimiter header={offset_header},start={offset_start},record_size={record_size}"
        );

        let last_pos = serializer.pos();
        let _ = serializer;

        // Write the record and then move back to the end again
        state.underlying_file.seek(SeekFrom::Start(offset_header))?;
        let header_bytes = {
            let a = (record_type as u16).to_be_bytes();
            let b = &record_size.to_be_bytes()[2..8];
            [a[0], a[1], b[0], b[1], b[2], b[3], b[4], b[5]]
        };
        state.underlying_file.write_all(&header_bytes)?;
        state.underlying_file.seek(SeekFrom::Start(offset_end))?;

        state.arena.shrink();
        state.pos = last_pos;

        // Now write the actual data and update the offsets
        Ok(LogWriteResult {
            record_start: offset_start,
            record_end: offset_end,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.underlying_file.flush()?;
        Ok(())
    }
}

impl ReadableJournal for LogFileJournalRx {
    /// UNSAFE: This method uses unsafe operations to remove the need to zero
    /// the buffer before its read the log entries into it
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        let mut buffer_pos = self.buffer_pos.lock().unwrap();

        // Get a memory reference to the data on the disk at
        // the current read location
        let mut buffer_ptr = self.buffer.as_ref();
        buffer_ptr.advance(*buffer_pos);
        loop {
            // Read the headers and advance
            if buffer_ptr.len() < 8 {
                return Ok(None);
            }

            let record_type: JournalEntryRecordType;
            let header = {
                let b = buffer_ptr;

                // If the next header is the magic itself then skip it.
                // You may be wondering how a magic could appear later
                // in the journal itself. This can happen if someone
                // concat's multiple journals together to make a combined
                // journal
                if b[0..8] == JOURNAL_MAGIC_NUMBER_BYTES[0..8] {
                    buffer_ptr.advance(8);
                    *buffer_pos += 8;
                    continue;
                }

                // Otherwise we decode the header
                let header = JournalEntryHeader {
                    record_type: u16::from_be_bytes([b[0], b[1]]),
                    record_size: u64::from_be_bytes([0u8, 0u8, b[2], b[3], b[4], b[5], b[6], b[7]]),
                };

                // Now we read the entry
                record_type = match header.record_type.try_into() {
                    Ok(t) => t,
                    Err(_) => {
                        tracing::debug!(
                            "unknown journal entry type ({}) - the journal stops here",
                            header.record_type
                        );
                        return Ok(None);
                    }
                };

                buffer_ptr.advance(8);
                *buffer_pos += 8;
                header
            };
            let record_start = *buffer_pos as u64;

            // Move the buffer position forward past the record
            let entry = &buffer_ptr[..(header.record_size as usize)];
            buffer_ptr.advance(header.record_size as usize);
            *buffer_pos += header.record_size as usize;

            let record = unsafe { record_type.deserialize_archive(entry)? };
            return Ok(Some(LogReadResult {
                record_start,
                record_end: *buffer_pos as u64,
                record,
            }));
        }
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        if let Some(tx) = &self.tx {
            let ret = tx.as_rx()?;
            Ok(Box::new(ret))
        } else {
            Ok(Box::new(LogFileJournalRx {
                tx: None,
                buffer_pos: Mutex::new(0),
                buffer: self.buffer.clone(),
                store: self.store.clone(),
            }))
        }
    }
}

impl WritableJournal for LogFileJournal {
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

impl ReadableJournal for LogFileJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for LogFileJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}

#[cfg(test)]
mod tests {
    use wasmer_wasix_types::wasix::WasiMemoryLayout;

    use super::*;

    #[tracing_test::traced_test]
    #[test]
    pub fn test_save_and_load_journal_events() {
        // Get a random file path
        let file = tempfile::NamedTempFile::new().unwrap();

        // Write some events to it
        let journal = LogFileJournal::from_file(file.as_file().try_clone().unwrap()).unwrap();
        journal
            .write(JournalEntry::CreatePipeV1 {
                read_fd: 1,
                write_fd: 2,
            })
            .unwrap();
        journal
            .write(JournalEntry::SetThreadV1 {
                id: 1,
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
                layout: WasiMemoryLayout {
                    stack_upper: 0,
                    stack_lower: 1024,
                    guard_size: 16,
                    stack_size: 1024,
                },
                start: wasmer_wasix_types::wasix::ThreadStartType::MainThread,
            })
            .unwrap();
        journal.write(JournalEntry::PortAddrClearV1).unwrap();
        drop(journal);

        // Read the events and validate
        let journal = LogFileJournal::new(file.path()).unwrap();
        let event1 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event2 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event3 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event4 = journal.read().unwrap().map(LogReadResult::into_inner);

        // Check the events
        assert_eq!(
            event1,
            Some(JournalEntry::CreatePipeV1 {
                read_fd: 1,
                write_fd: 2
            })
        );
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1,
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
                layout: WasiMemoryLayout {
                    stack_upper: 0,
                    stack_lower: 1024,
                    guard_size: 16,
                    stack_size: 1024,
                },
                start: wasmer_wasix_types::wasix::ThreadStartType::MainThread,
            })
        );
        assert_eq!(event3, Some(JournalEntry::PortAddrClearV1));
        assert_eq!(event4, None);

        // Now write another event
        journal
            .write(JournalEntry::SocketSendV1 {
                fd: 1234,
                data: [12; 1024].to_vec().into(),
                flags: 123,
                is_64bit: true,
            })
            .unwrap();

        // The event should not be visible yet unless we reload the log file
        assert_eq!(journal.read().unwrap().map(LogReadResult::into_inner), None);

        // Reload the load file
        let journal = LogFileJournal::new(file.path()).unwrap();

        // Before we read it, we will throw in another event
        journal
            .write(JournalEntry::CreatePipeV1 {
                read_fd: 1234,
                write_fd: 5432,
            })
            .unwrap();

        let event1 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event2 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event3 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event4 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event5 = journal.read().unwrap().map(LogReadResult::into_inner);
        assert_eq!(
            event1,
            Some(JournalEntry::CreatePipeV1 {
                read_fd: 1,
                write_fd: 2
            })
        );
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1,
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
                layout: WasiMemoryLayout {
                    stack_upper: 0,
                    stack_lower: 1024,
                    guard_size: 16,
                    stack_size: 1024,
                },
                start: wasmer_wasix_types::wasix::ThreadStartType::MainThread,
            })
        );
        assert_eq!(event3, Some(JournalEntry::PortAddrClearV1));
        assert_eq!(
            event4,
            Some(JournalEntry::SocketSendV1 {
                fd: 1234,
                data: [12; 1024].to_vec().into(),
                flags: 123,
                is_64bit: true,
            })
        );
        assert_eq!(event5, None);

        // Load it again
        let journal = LogFileJournal::new(file.path()).unwrap();

        let event1 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event2 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event3 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event4 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event5 = journal.read().unwrap().map(LogReadResult::into_inner);
        let event6 = journal.read().unwrap().map(LogReadResult::into_inner);

        tracing::info!("event1 {:?}", event1);
        tracing::info!("event2 {:?}", event2);
        tracing::info!("event3 {:?}", event3);
        tracing::info!("event4 {:?}", event4);
        tracing::info!("event5 {:?}", event5);
        tracing::info!("event6 {:?}", event6);

        assert_eq!(
            event1,
            Some(JournalEntry::CreatePipeV1 {
                read_fd: 1,
                write_fd: 2
            })
        );
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1,
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
                layout: WasiMemoryLayout {
                    stack_upper: 0,
                    stack_lower: 1024,
                    guard_size: 16,
                    stack_size: 1024,
                },
                start: wasmer_wasix_types::wasix::ThreadStartType::MainThread,
            })
        );
        assert_eq!(event3, Some(JournalEntry::PortAddrClearV1));
        assert_eq!(
            event4,
            Some(JournalEntry::SocketSendV1 {
                fd: 1234,
                data: [12; 1024].to_vec().into(),
                flags: 123,
                is_64bit: true,
            })
        );
        assert_eq!(
            event5,
            Some(JournalEntry::CreatePipeV1 {
                read_fd: 1234,
                write_fd: 5432,
            })
        );
        assert_eq!(event6, None);
    }
}
