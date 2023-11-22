use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::sync::Mutex;

use super::*;

// The pipe journal will feed journal entries between two bi-directional ends
// of a pipe.
#[derive(Debug)]
pub struct PipeJournal {
    tx: mpsc::Sender<JournalEntry<'static>>,
    rx: Mutex<mpsc::Receiver<JournalEntry<'static>>>,
}

impl PipeJournal {
    pub fn channel() -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let end1 = PipeJournal {
            tx: tx1,
            rx: Mutex::new(rx2),
        };

        let end2 = PipeJournal {
            tx: tx2,
            rx: Mutex::new(rx1),
        };

        (end1, end2)
    }
}

impl Journal for PipeJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        let entry = entry.into_owned();
        self.tx.send(entry).map_err(|err| {
            anyhow::format_err!("failed to send journal event through the pipe - {}", err)
        })
    }

    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        let rx = self.rx.lock().unwrap();
        match rx.try_recv() {
            Ok(e) => Ok(Some(e)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(anyhow::format_err!(
                "failed to receive journal event from the pipe as its disconnected"
            )),
        }
    }
}
