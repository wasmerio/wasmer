use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use derivative::Derivative;

use super::*;

/// Journal which leave itself in a consistent state once it commits
/// by closing all the file descriptors that were opened while
/// it was recording writes.
#[derive(Debug)]
pub struct AutoConsistentJournal {
    tx: AutoConsistentJournalTx,
    rx: AutoConsistentJournalRx,
}

#[derive(Debug, Default, Clone)]
struct State {
    open_files: HashSet<u32>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct AutoConsistentJournalTx {
    state: Arc<Mutex<State>>,
    #[derivative(Debug = "ignore")]
    inner: Box<DynWritableJournal>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct AutoConsistentJournalRx {
    state: Arc<Mutex<State>>,
    #[derivative(Debug = "ignore")]
    inner: Box<DynReadableJournal>,
}

impl AutoConsistentJournal {
    /// Creates a journal which will automatically correct inconsistencies when
    /// it commits. E.g. it will close any open file descriptors that were left
    /// open as it was processing events.
    pub fn new<J>(inner: J) -> Self
    where
        J: Journal,
    {
        let state = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = inner.split();
        Self {
            tx: AutoConsistentJournalTx {
                inner: tx,
                state: state.clone(),
            },
            rx: AutoConsistentJournalRx {
                inner: rx,
                state: state.clone(),
            },
        }
    }

    pub fn into_inner(self) -> RecombinedJournal {
        RecombinedJournal::new(self.tx.inner, self.rx.inner)
    }
}

impl WritableJournal for AutoConsistentJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        match &entry {
            JournalEntry::OpenFileDescriptorV1 { fd, .. }
            | JournalEntry::SocketAcceptedV1 { fd, .. }
            | JournalEntry::CreateEventV1 { fd, .. } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.insert(*fd);
            }
            JournalEntry::CreatePipeV1 { fd1, fd2 } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.insert(*fd1);
                state.open_files.insert(*fd2);
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                let mut state = self.state.lock().unwrap();
                if state.open_files.remove(old_fd) {
                    state.open_files.insert(*new_fd);
                }
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                let mut state = self.state.lock().unwrap();
                if state.open_files.contains(original_fd) {
                    state.open_files.insert(*copied_fd);
                }
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.remove(fd);
            }
            JournalEntry::InitModuleV1 { .. }
            | JournalEntry::ClearEtherealV1 { .. }
            | JournalEntry::ProcessExitV1 { .. } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.clear();
            }
            _ => {}
        }
        self.inner.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.inner.flush()
    }

    /// Commits the transaction
    fn commit(&self) -> anyhow::Result<usize> {
        let open_files = {
            let mut state = self.state.lock().unwrap();
            let mut open_files = Default::default();
            std::mem::swap(&mut open_files, &mut state.open_files);
            open_files
        };
        for fd in open_files {
            let entry = JournalEntry::CloseFileDescriptorV1 { fd };
            self.inner.write(entry)?;
        }
        self.inner.commit()
    }

    /// Rolls back the transaction and aborts its changes
    fn rollback(&self) -> anyhow::Result<usize> {
        {
            let mut state = self.state.lock().unwrap();
            state.open_files.clear();
        }
        self.inner.rollback()
    }
}

impl ReadableJournal for AutoConsistentJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(AutoConsistentJournalRx {
            inner: self.inner.as_restarted()?,
            state: Arc::new(Mutex::new(State::default())),
        }))
    }
}

impl WritableJournal for AutoConsistentJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }
}

impl ReadableJournal for AutoConsistentJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for AutoConsistentJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
