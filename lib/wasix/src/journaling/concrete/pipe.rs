use std::sync::Mutex;

use futures::future::LocalBoxFuture;
use tokio::sync::mpsc::{self, error::TryRecvError};

use super::*;

// The pipe journal will feed journal entries between two bi-directional ends
// of a pipe.
#[derive(Debug)]
pub struct PipeJournal {
    tx: mpsc::Sender<JournalEntry<'static>>,
    rx: Mutex<mpsc::Receiver<JournalEntry<'static>>>,
}

impl PipeJournal {
    pub fn channel(buffer: usize) -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel(buffer);
        let (tx2, rx2) = mpsc::channel(buffer);

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
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        let entry = entry.into_owned();
        Box::pin(async {
            self.tx.send(entry).await.map_err(|err| {
                anyhow::format_err!("failed to send journal event through the pipe - {}", err)
            })
        })
    }

    fn read<'a>(&'a self) -> LocalBoxFuture<'_, anyhow::Result<Option<JournalEntry<'a>>>> {
        Box::pin(async {
            let mut rx = self.rx.lock().unwrap();
            match rx.try_recv() {
                Ok(e) => Ok(Some(e.into())),
                Err(TryRecvError::Empty) => Ok(None),
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow::format_err!(
                        "failed to receive journal event from the pipe as its disconnected"
                    ))
                }
            }
        })
    }
}
