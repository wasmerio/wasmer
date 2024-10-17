use super::*;
use std::ops::Deref;
use std::sync::Arc;

impl<R: ReadableJournal> ReadableJournal for Arc<R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl<W: WritableJournal> WritableJournal for Arc<W> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.deref().write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.deref().flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        self.deref().commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.deref().rollback()
    }
}

impl ReadableJournal for Arc<DynJournal> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl WritableJournal for Arc<DynJournal> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.deref().write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.deref().flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        self.deref().commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.deref().rollback()
    }
}

impl Journal for Arc<DynJournal> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.clone()), Box::new(self.clone()))
    }
}
