use super::*;

#[derive(Debug)]
pub struct CompactingTransactionJournalTx {
    inner: TransactionJournalTx,
}

#[derive(Debug)]
pub struct CompactingTransactionJournalRx {
    inner: TransactionJournalRx,
}

/// Journal which will store the events locally in memory until it
/// is either committed or rolled back
#[derive(Debug)]
pub struct CompactingTransactionJournal {
    tx: CompactingTransactionJournalTx,
    rx: CompactingTransactionJournalRx,
}

impl CompactingTransactionJournal {
    /// Creates a compacting transactional journal which will hold events in
    /// memory until the journal is either committed or rolled back.
    ///
    /// When the journal is commited it will perform a compaction of the events
    /// before they are misseeed to the underlying journal
    pub fn new<J>(inner: J) -> Self
    where
        J: Journal,
    {
        let inner = TransactionJournal::new(inner);
        Self {
            rx: CompactingTransactionJournalRx { inner: inner.rx },
            tx: CompactingTransactionJournalTx { inner: inner.tx },
        }
    }

    pub fn into_inner(self) -> TransactionJournal {
        TransactionJournal {
            rx: self.rx.inner,
            tx: self.tx.inner,
        }
    }
}

impl WritableJournal for CompactingTransactionJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.inner.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.inner.flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        // We read all the events that have been buffered
        let (records, mut new_offset) = {
            let mut state = self.inner.state.lock().unwrap();
            let mut records = Default::default();
            std::mem::swap(&mut records, &mut state.records);
            (records, state.offset)
        };
        if records.is_empty() {
            return Ok(0);
        }

        // We prepare a compacting journal which does nothing
        // with the events other than learn from them
        let compacting = CompactingJournal::new(NullJournal::default())?;
        for record in records.iter() {
            compacting.write(record.clone())?;
        }

        // Next we create an inline journal that is used for streaming the
        // events the journal this is under this super journal
        struct RelayJournal<'a> {
            inner: &'a CompactingTransactionJournalTx,
        }
        impl WritableJournal for RelayJournal<'_> {
            fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
                self.inner.write(entry)
            }
            fn flush(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }
        impl ReadableJournal for RelayJournal<'_> {
            fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
                Ok(None)
            }
            fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
                NullJournal::default().split().1.as_restarted()
            }
        }
        impl Journal for RelayJournal<'_> {
            fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
                NullJournal::default().split()
            }
        }
        let relay_journal = RelayJournal { inner: self };

        // Now we create a filter journal which will filter out the events
        // that are not needed and stream them down
        let mut ret = 0;
        let filter = compacting.create_filter(relay_journal);
        for entry in records {
            let res = filter.write(entry)?;
            if res.record_start == 0 && res.record_end == 0 {
                continue;
            }
            ret += 1;
            new_offset = new_offset.max(res.record_end);
        }
        {
            let mut state = self.inner.state.lock().unwrap();
            state.offset = state.offset.max(new_offset);
        }
        ret += self.inner.commit()?;
        Ok(ret)
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.inner.rollback()
    }
}

impl ReadableJournal for CompactingTransactionJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.inner.as_restarted()
    }
}

impl WritableJournal for CompactingTransactionJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }
}

impl ReadableJournal for CompactingTransactionJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for CompactingTransactionJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
