use super::*;

#[derive(Debug)]
pub struct CompactingTransactionJournalTx<W: WritableJournal> {
    inner: TransactionJournalTx<W>,
}

#[derive(Debug)]
pub struct CompactingTransactionJournalRx<R: ReadableJournal> {
    inner: TransactionJournalRx<R>,
}

/// Journal which will store the events locally in memory until it
/// is either committed or rolled back
#[derive(Debug)]
pub struct CompactingTransactionJournal<W: WritableJournal, R: ReadableJournal> {
    tx: CompactingTransactionJournalTx<W>,
    rx: CompactingTransactionJournalRx<R>,
}

impl CompactingTransactionJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
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
}

impl<W: WritableJournal, R: ReadableJournal> CompactingTransactionJournal<W, R> {
    pub fn into_inner(self) -> TransactionJournal<W, R> {
        TransactionJournal {
            rx: self.rx.inner,
            tx: self.tx.inner,
        }
    }
}

impl<W: WritableJournal> WritableJournal for CompactingTransactionJournalTx<W> {
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
        #[derive(Debug)]
        struct RelayJournal<'a, W: WritableJournal> {
            inner: &'a CompactingTransactionJournalTx<W>,
        }
        impl<W: WritableJournal> WritableJournal for RelayJournal<'_, W> {
            fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
                self.inner.write(entry)
            }
            fn flush(&self) -> anyhow::Result<()> {
                Ok(())
            }
            fn commit(&self) -> anyhow::Result<usize> {
                self.inner.commit()
            }
            fn rollback(&self) -> anyhow::Result<usize> {
                self.inner.rollback()
            }
        }
        let relay_journal = RelayJournal { inner: self };

        // Now we create a filter journal which will filter out the events
        // that are not needed and stream them down
        let mut ret = 0;
        let filter =
            compacting.create_split_filter(relay_journal, NullJournal::default().split().1);
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

impl<R: ReadableJournal> ReadableJournal for CompactingTransactionJournalRx<R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.inner.as_restarted()
    }
}

impl<W: WritableJournal, R: ReadableJournal> WritableJournal
    for CompactingTransactionJournal<W, R>
{
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

impl<W: WritableJournal, R: ReadableJournal> ReadableJournal
    for CompactingTransactionJournal<W, R>
{
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for CompactingTransactionJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
