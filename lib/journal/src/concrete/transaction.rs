use std::sync::{Arc, Mutex};

use derivative::Derivative;

use super::*;

/// Journal which will store the events locally in memory until it
/// is either committed or rolled back
#[derive(Debug)]
pub struct TransactionJournal {
    pub(super) tx: TransactionJournalTx,
    pub(super) rx: TransactionJournalRx,
}

#[derive(Debug, Default, Clone)]
pub(super) struct State {
    pub(super) records: Vec<JournalEntry<'static>>,
    pub(super) offset: u64,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct TransactionJournalTx {
    pub(super) state: Arc<Mutex<State>>,
    #[derivative(Debug = "ignore")]
    inner: Box<DynWritableJournal>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct TransactionJournalRx {
    state: Arc<Mutex<State>>,
    #[derivative(Debug = "ignore")]
    inner: Box<DynReadableJournal>,
}

impl TransactionJournal {
    /// Creates a transactional journal which will hold events in memory
    /// until the journal is either committed or rolled back
    pub fn new<J>(inner: J) -> Self
    where
        J: Journal,
    {
        let state = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = inner.split();
        Self {
            tx: TransactionJournalTx {
                inner: tx,
                state: state.clone(),
            },
            rx: TransactionJournalRx {
                inner: rx,
                state: state.clone(),
            },
        }
    }

    pub fn into_inner(self) -> RecombinedJournal {
        RecombinedJournal::new(self.tx.inner, self.rx.inner)
    }
}

impl WritableJournal for TransactionJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let entry = entry.into_owned();
        let mut state = self.state.lock().unwrap();
        let estimate_size = entry.estimate_size();
        state.records.push(entry);
        Ok(LogWriteResult {
            record_start: state.offset as u64,
            record_end: state.offset as u64 + estimate_size as u64,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.inner.flush()
    }

    /// Commits the transaction
    fn commit(&self) -> anyhow::Result<usize> {
        let (records, mut new_offset) = {
            let mut state = self.state.lock().unwrap();
            let mut records = Default::default();
            std::mem::swap(&mut records, &mut state.records);
            (records, state.offset)
        };

        let mut ret = records.len();
        for entry in records {
            let ret = self.inner.write(entry)?;
            new_offset = new_offset.max(ret.record_end);
        }
        {
            let mut state = self.state.lock().unwrap();
            state.offset = state.offset.max(new_offset);
        }
        ret += self.inner.commit()?;
        Ok(ret)
    }

    /// Rolls back the transaction and aborts its changes
    fn rollback(&self) -> anyhow::Result<usize> {
        let mut ret = {
            let mut state = self.state.lock().unwrap();
            let ret = state.records.len();
            state.records.clear();
            ret
        };
        ret += self.inner.rollback()?;
        Ok(ret)
    }
}

impl ReadableJournal for TransactionJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        let ret = self.inner.read()?;
        if let Some(res) = ret.as_ref() {
            let mut state = self.state.lock().unwrap();
            state.offset = state.offset.max(res.record_end);
        }
        Ok(ret)
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(TransactionJournalRx {
            inner: self.inner.as_restarted()?,
            state: Arc::new(Mutex::new(State::default())),
        }))
    }
}

impl WritableJournal for TransactionJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }
}

impl ReadableJournal for TransactionJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for TransactionJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
