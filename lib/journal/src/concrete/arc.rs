use super::*;
use std::ops::Deref;
use std::sync::Arc;

impl ReadableJournal for Arc<DynReadableJournal> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl WritableJournal for Arc<DynWritableJournal> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.deref().write(entry)
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
}

impl Journal for Arc<DynJournal> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.clone()), Box::new(self.clone()))
    }
}
