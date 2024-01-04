use super::*;

pub struct RecombinedJournal {
    tx: Box<DynWritableJournal>,
    rx: Box<DynReadableJournal>,
}

impl RecombinedJournal {
    pub fn new(tx: Box<DynWritableJournal>, rx: Box<DynReadableJournal>) -> Self {
        Self { tx, rx }
    }
}

impl WritableJournal for RecombinedJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<u64> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for RecombinedJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for RecombinedJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (self.tx, self.rx)
    }
}
