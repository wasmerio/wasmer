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
    receiver: Arc<Mutex<mpsc::Receiver<LogReadResult<'static>>>>,
}

#[derive(Debug)]
struct SenderState {
    offset: u64,
    sender: mpsc::Sender<LogReadResult<'static>>,
}

#[derive(Debug)]
pub struct PipeJournalTx {
    sender: Arc<Mutex<SenderState>>,
}

impl PipeJournal {
    pub fn channel() -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let end1 = PipeJournal {
            tx: PipeJournalTx {
                sender: Arc::new(Mutex::new(SenderState {
                    offset: 0,
                    sender: tx1,
                })),
            },
            rx: PipeJournalRx {
                receiver: Arc::new(Mutex::new(rx2)),
            },
        };

        let end2 = PipeJournal {
            tx: PipeJournalTx {
                sender: Arc::new(Mutex::new(SenderState {
                    offset: 0,
                    sender: tx2,
                })),
            },
            rx: PipeJournalRx {
                receiver: Arc::new(Mutex::new(rx1)),
            },
        };

        (end1, end2)
    }
}

impl WritableJournal for PipeJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let entry = entry.into_owned();
        let entry_size = entry.estimate_size() as u64;

        let mut sender = self.sender.lock().unwrap();
        sender
            .sender
            .send(LogReadResult {
                record_start: sender.offset,
                record_end: sender.offset + entry_size,
                record: entry,
            })
            .map_err(|err| {
                anyhow::format_err!("failed to send journal event through the pipe - {}", err)
            })?;
        sender.offset += entry_size;
        Ok(LogWriteResult {
            record_start: sender.offset,
            record_end: sender.offset + entry_size,
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl ReadableJournal for PipeJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
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

impl ReadableJournal for PipeJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
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
