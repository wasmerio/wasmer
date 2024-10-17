use std::{
    fs::File,
    io::{Seek, Write},
    path::Path,
    sync::RwLock,
};

use lz4_flex::{block, decompress};

use super::*;

/// The memory file journal processes journal entries by writing any memory mutations
/// directly to a file. Later this can be used as a mounting target for resuming
/// a process without having to reload the journal from scratch.
#[derive(Debug)]
pub struct MemFileJournal {
    file: RwLock<File>,
}

impl MemFileJournal {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            file: RwLock::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)?,
            ),
        })
    }
}

impl Drop for MemFileJournal {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl Clone for MemFileJournal {
    fn clone(&self) -> Self {
        let file = self.file.read().unwrap();
        Self {
            file: RwLock::new(file.try_clone().unwrap()),
        }
    }
}

impl ReadableJournal for MemFileJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        Ok(None)
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(self.clone()))
    }
}

impl WritableJournal for MemFileJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let estimated_size = entry.estimate_size() as u64;
        match entry {
            JournalEntry::UpdateMemoryRegionV1 {
                region,
                compressed_data,
            } => {
                let (uncompressed_size, compressed_data) =
                    block::uncompressed_size(&compressed_data)?;
                let decompressed_data = decompress(compressed_data, uncompressed_size)?;

                let mut file = self.file.write().unwrap();
                file.seek(std::io::SeekFrom::Start(region.start))?;
                file.write_all(&decompressed_data)?;
            }
            JournalEntry::ProcessExitV1 { .. } | JournalEntry::InitModuleV1 { .. } => {
                let file = self.file.read().unwrap();
                file.set_len(0)?;
            }
            _ => {}
        }

        Ok(LogWriteResult {
            record_start: 0,
            record_end: estimated_size,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        let mut file = self.file.write().unwrap();
        file.flush()?;
        Ok(())
    }
}

impl Journal for MemFileJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.clone()), Box::new(self.clone()))
    }
}
