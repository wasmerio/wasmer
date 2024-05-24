use std::sync::Arc;
use std::sync::Mutex;

use super::*;

// The buffered journal will keep all the events in memory until it
// is either reset or dropped.
#[derive(Debug)]
pub struct BufferedJournal {
    tx: BufferedJournalTx,
    rx: BufferedJournalRx,
}

#[derive(Debug, Default, Clone)]
struct State {
    records: Arc<Mutex<Vec<JournalEntry<'static>>>>,
    offset: usize,
}

#[derive(Debug)]
pub struct BufferedJournalRx {
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
pub struct BufferedJournalTx {
    state: Arc<Mutex<State>>,
}

impl Default for BufferedJournal {
    fn default() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        Self {
            tx: BufferedJournalTx {
                state: state.clone(),
            },
            rx: BufferedJournalRx { state },
        }
    }
}

impl WritableJournal for BufferedJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let entry = entry.into_owned();
        let state = self.state.lock().unwrap();
        let estimate_size = entry.estimate_size();
        state.records.lock().unwrap().push(entry);
        Ok(LogWriteResult {
            record_start: state.offset as u64,
            record_end: state.offset as u64 + estimate_size as u64,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl ReadableJournal for BufferedJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        let mut state = self.state.lock().unwrap();
        let ret = state.records.lock().unwrap().get(state.offset).cloned();

        let record_start = state.offset as u64;
        if ret.is_some() {
            state.offset += 1;
        }
        Ok(ret.map(|r| LogReadResult {
            record_start,
            record_end: state.offset as u64,
            record: r,
        }))
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        let mut state = self.state.lock().unwrap().clone();
        state.offset = 0;
        Ok(Box::new(BufferedJournalRx {
            state: Arc::new(Mutex::new(state)),
        }))
    }
}

impl WritableJournal for BufferedJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }
}

impl ReadableJournal for BufferedJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for BufferedJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
