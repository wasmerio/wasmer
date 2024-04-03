use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};

use super::*;

/// Journal that counts the size of the entries that are written to it
#[derive(Debug, Clone, Default)]
pub struct CountingJournal {
    n_cnt: Arc<AtomicUsize>,
    n_size: Arc<AtomicU64>,
}

impl CountingJournal {
    pub fn cnt(&self) -> usize {
        self.n_cnt.load(Ordering::SeqCst)
    }

    pub fn size(&self) -> u64 {
        self.n_size.load(Ordering::SeqCst)
    }
}

impl ReadableJournal for CountingJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        Ok(None)
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::<CountingJournal>::default())
    }
}

impl WritableJournal for CountingJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let size = entry.estimate_size() as u64;
        let offset = self.n_cnt.fetch_add(1, Ordering::SeqCst);
        self.n_size.fetch_add(size, Ordering::SeqCst);
        Ok(LogWriteResult {
            record_start: offset as u64,
            record_end: offset as u64 + size,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Journal for CountingJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.clone()), Box::new(self))
    }
}
