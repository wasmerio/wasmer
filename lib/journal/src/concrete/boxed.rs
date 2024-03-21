use std::ops::Deref;

use super::*;

impl ReadableJournal for Box<DynReadableJournal> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl WritableJournal for Box<DynWritableJournal> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.deref().write(entry)
    }
}

impl ReadableJournal for Box<DynJournal> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.deref().read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.deref().as_restarted()
    }
}

impl WritableJournal for Box<DynJournal> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.deref().write(entry)
    }
}
