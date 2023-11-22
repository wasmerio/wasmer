use super::*;

pub struct CompositeJournal {
    tx: Box<DynWritableJournal>,
    rx: Box<DynReadableJournal>,
}

impl CompositeJournal {
    pub fn new(tx: Box<DynWritableJournal>, rx: Box<DynReadableJournal>) -> Self {
        Self { tx, rx }
    }
}

impl WritableJournal for CompositeJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for CompositeJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for CompositeJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (self.tx, self.rx)
    }
}
