use super::*;

#[derive(Debug)]
pub struct RecombinedJournal<W: WritableJournal, R: ReadableJournal> {
    tx: W,
    rx: R,
}

impl<W: WritableJournal, R: ReadableJournal> RecombinedJournal<W, R> {
    pub fn new(tx: W, rx: R) -> Self {
        Self { tx, rx }
    }
}

impl<W: WritableJournal, R: ReadableJournal> WritableJournal for RecombinedJournal<W, R> {
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

impl<W: WritableJournal, R: ReadableJournal> ReadableJournal for RecombinedJournal<W, R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl<W, R> Journal for RecombinedJournal<W, R>
where
    W: WritableJournal + Send + Sync + 'static,
    R: ReadableJournal + Send + Sync + 'static,
{
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
