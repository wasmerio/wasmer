use bytes::Buf;
use rkyv::ser::serializers::{
    AllocScratch, CompositeSerializer, SharedSerializeMap, WriteSerializer,
};
use shared_buffer::OwnedBuffer;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};

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
pub struct LogFileJournal {
    tx: LogFileJournalTx,
    rx: LogFileJournalRx,
}

#[derive(Debug)]
struct TxState {
    file: File,
    serializer: CompositeSerializer<WriteSerializer<File>, AllocScratch, SharedSerializeMap>,
}

#[derive(Debug, Clone)]
pub struct LogFileJournalTx {
    state: Arc<Mutex<TxState>>,
}

#[derive(Debug)]
pub struct LogFileJournalRx {
    tx: LogFileJournalTx,
    buffer_pos: Mutex<usize>,
    buffer: OwnedBuffer,
}

impl LogFileJournalTx {
    pub fn as_rx(&self) -> anyhow::Result<LogFileJournalRx> {
        let state = self.state.lock().unwrap();
        let file = state.file.try_clone()?;

        let buffer = OwnedBuffer::from_file(&file)?;

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
            tx: self.clone(),
            buffer_pos: Mutex::new(buffer_pos),
            buffer,
        })
    }
}

impl LogFileJournal {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        Self::from_file(file)
    }

    pub fn from_file(mut file: std::fs::File) -> anyhow::Result<Self> {
        // Move to the end of the file and write the
        // magic if one is needed
        if file.seek(SeekFrom::End(0)).unwrap() == 0 {
            let magic = JOURNAL_MAGIC_NUMBER;
            let magic = magic.to_be_bytes();
            file.write_all(&magic)?;
        }

        // Create the tx
        let tx = LogFileJournalTx {
            state: Arc::new(Mutex::new(TxState {
                file: file.try_clone()?,
                serializer: CompositeSerializer::new(
                    WriteSerializer::new(file),
                    AllocScratch::default(),
                    SharedSerializeMap::default(),
                ),
            })),
        };

        // First we create the readable journal
        let rx = tx.as_rx()?;

        Ok(Self { rx, tx })
    }
}

impl WritableJournal for LogFileJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        tracing::debug!("journal event: {:?}", entry);

        let mut state = self.state.lock().unwrap();

        // Write the header (with a record size of zero)
        let record_type: JournalEntryRecordType = entry.archive_record_type();
        state.file.write_all(&(record_type as u16).to_be_bytes())?;
        let offset_size = state.file.stream_position()?;
        state.file.write_all(&[0u8; 6])?; // record and pad size (48 bits)

        // Now serialize the actual data to the log
        let offset_start = state.file.stream_position()?;
        entry.serialize_archive(&mut state.serializer)?;
        let offset_end = state.file.stream_position()?;
        let record_size = offset_end - offset_start;

        // If the alightment is out then fail
        if record_size % 8 != 0 {
            tracing::error!(
                "alignment is out for journal event (type={:?}, record_size={}, alignment={})",
                record_type,
                record_size,
                record_size % 8
            );
        }

        // Write the record and then move back to the end again
        state.file.seek(SeekFrom::Start(offset_size))?;
        state.file.write_all(&record_size.to_be_bytes()[2..8])?;
        state.file.seek(SeekFrom::Start(offset_end))?;

        // Now write the actual data and update the offsets
        Ok(record_size)
    }
}

impl ReadableJournal for LogFileJournalRx {
    /// UNSAFE: This method uses unsafe operations to remove the need to zero
    /// the buffer before its read the log entries into it
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
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
                buffer_ptr.advance(8);
                *buffer_pos += 8;
                header
            };

            if header.record_size as usize > buffer_ptr.len() {
                *buffer_pos += buffer_ptr.len();
                tracing::trace!(
                    "journal is corrupt (record_size={} vs remaining={})",
                    header.record_size,
                    buffer_ptr.len()
                );
                return Ok(None);
            }

            // Move the buffer position forward past the record
            let entry = &buffer_ptr[..(header.record_size as usize)];
            buffer_ptr.advance(header.record_size as usize);
            *buffer_pos += header.record_size as usize;

            // Now we read the entry
            let record_type: JournalEntryRecordType = match header.record_type.try_into() {
                Ok(t) => t,
                Err(_) => {
                    tracing::debug!(
                        "unknown journal entry type ({}) - skipping",
                        header.record_type
                    );
                    continue;
                }
            };

            let record = unsafe { record_type.deserialize_archive(entry)? };
            return Ok(Some(record));
        }
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        let ret = self.tx.as_rx()?;
        Ok(Box::new(ret))
    }
}

impl WritableJournal for LogFileJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for LogFileJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
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
    use super::*;

    #[tracing_test::traced_test]
    #[test]
    pub fn test_save_and_load_journal_events() {
        // Get a random file path
        let file = tempfile::NamedTempFile::new().unwrap();

        // Write some events to it
        let journal = LogFileJournal::from_file(file.as_file().try_clone().unwrap()).unwrap();
        journal
            .write(JournalEntry::CreatePipeV1 { fd1: 1, fd2: 2 })
            .unwrap();
        journal
            .write(JournalEntry::SetThreadV1 {
                id: 1.into(),
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
            })
            .unwrap();
        journal.write(JournalEntry::PortAddrClearV1).unwrap();
        drop(journal);

        // Read the events and validate
        let journal = LogFileJournal::new(file.path()).unwrap();
        let event1 = journal.read().unwrap();
        let event2 = journal.read().unwrap();
        let event3 = journal.read().unwrap();
        let event4 = journal.read().unwrap();

        // Check the events
        assert_eq!(event1, Some(JournalEntry::CreatePipeV1 { fd1: 1, fd2: 2 }));
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1.into(),
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
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
        assert_eq!(journal.read().unwrap(), None);

        // Reload the load file
        let journal = LogFileJournal::new(file.path()).unwrap();

        // Before we read it, we will throw in another event
        journal
            .write(JournalEntry::CreatePipeV1 {
                fd1: 1234,
                fd2: 5432,
            })
            .unwrap();

        let event1 = journal.read().unwrap();
        let event2 = journal.read().unwrap();
        let event3 = journal.read().unwrap();
        let event4 = journal.read().unwrap();
        let event5 = journal.read().unwrap();
        assert_eq!(event1, Some(JournalEntry::CreatePipeV1 { fd1: 1, fd2: 2 }));
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1.into(),
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
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

        let event1 = journal.read().unwrap();
        let event2 = journal.read().unwrap();
        let event3 = journal.read().unwrap();
        let event4 = journal.read().unwrap();
        let event5 = journal.read().unwrap();
        let event6 = journal.read().unwrap();

        tracing::info!("event1 {:?}", event1);
        tracing::info!("event2 {:?}", event2);
        tracing::info!("event3 {:?}", event3);
        tracing::info!("event4 {:?}", event4);
        tracing::info!("event5 {:?}", event5);
        tracing::info!("event6 {:?}", event6);

        assert_eq!(event1, Some(JournalEntry::CreatePipeV1 { fd1: 1, fd2: 2 }));
        assert_eq!(
            event2,
            Some(JournalEntry::SetThreadV1 {
                id: 1.into(),
                call_stack: vec![11; 116].into(),
                memory_stack: vec![22; 16].into(),
                store_data: vec![33; 136].into(),
                is_64bit: false,
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
                fd1: 1234,
                fd2: 5432,
            })
        );
        assert_eq!(event6, None);
    }
}
