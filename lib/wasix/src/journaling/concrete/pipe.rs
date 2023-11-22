use std::sync::mpsc::TryRecvError;
use std::sync::Mutex;
use std::sync::{mpsc, Arc};

use super::*;

// The pipe journal will feed journal entries between two bi-directional ends
// of a pipe.
#[derive(Debug)]
pub struct PipeJournal {
    tx: PipeJournalTx,
    rx: PipeJournalRx,
}

#[derive(Debug)]
pub struct PipeJournalRx {
    receiver: Arc<Mutex<mpsc::Receiver<JournalEntry<'static>>>>,
}

#[derive(Debug)]
pub struct PipeJournalTx {
    sender: Arc<Mutex<mpsc::Sender<JournalEntry<'static>>>>,
}

impl PipeJournal {
    pub fn channel() -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let end1 = PipeJournal {
            tx: PipeJournalTx {
                sender: Arc::new(Mutex::new(tx1)),
            },
            rx: PipeJournalRx {
                receiver: Arc::new(Mutex::new(rx2)),
            },
        };

        let end2 = PipeJournal {
            tx: PipeJournalTx {
                sender: Arc::new(Mutex::new(tx2)),
            },
            rx: PipeJournalRx {
                receiver: Arc::new(Mutex::new(rx1)),
            },
        };

        (end1, end2)
    }
}

impl WritableJournal for PipeJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        let entry = entry.into_owned();

        let sender = self.sender.lock().unwrap();
        sender.send(entry).map_err(|err| {
            anyhow::format_err!("failed to send journal event through the pipe - {}", err)
        })
    }
}

impl ReadableJournal for PipeJournalRx {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        let rx = self.receiver.lock().unwrap();
        match rx.try_recv() {
            Ok(e) => Ok(Some(e)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(anyhow::format_err!(
                "failed to receive journal event from the pipe as its disconnected"
            )),
        }
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(PipeJournalRx {
            receiver: self.receiver.clone(),
        }))
    }
}

impl WritableJournal for PipeJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for PipeJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for PipeJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
