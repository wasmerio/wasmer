use std::{ops::Deref, sync::Arc};

use super::*;

impl<R: ReadableJournal + ?Sized> ReadableJournal for Box<R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl<W: WritableJournal + ?Sized> WritableJournal for Box<W> {
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

impl Journal for Box<DynJournal> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        let this = Arc::new(self);
        (Box::new(this.clone()), Box::new(this))
    }
}
