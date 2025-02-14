use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use super::*;

/// Journal which leave itself in a consistent state once it commits
/// by closing all the file descriptors that were opened while
/// it was recording writes.
#[derive(Debug)]
pub struct AutoConsistentJournal<W: WritableJournal, R: ReadableJournal> {
    tx: AutoConsistentJournalTx<W>,
    rx: AutoConsistentJournalRx<R>,
}

#[derive(Debug, Default, Clone)]
struct State {
    open_files: HashSet<u32>,
    open_sockets: HashSet<u32>,
}

#[derive(Debug)]
pub struct AutoConsistentJournalTx<W: WritableJournal> {
    state: Arc<Mutex<State>>,
    inner: W,
}

#[derive(Debug)]
pub struct AutoConsistentJournalRx<R: ReadableJournal> {
    inner: R,
}

impl AutoConsistentJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
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
            rx: AutoConsistentJournalRx { inner: rx },
        }
    }
}

impl<W: WritableJournal, R: ReadableJournal> AutoConsistentJournal<W, R> {
    pub fn into_inner(self) -> RecombinedJournal<W, R> {
        RecombinedJournal::new(self.tx.inner, self.rx.inner)
    }
}

impl<W: WritableJournal> WritableJournal for AutoConsistentJournalTx<W> {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        match &entry {
            JournalEntry::OpenFileDescriptorV1 { fd, .. }
            | JournalEntry::CreateEventV1 { fd, .. } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.insert(*fd);
            }
            JournalEntry::SocketAcceptedV1 { fd, .. } => {
                let mut state = self.state.lock().unwrap();
                state.open_sockets.insert(*fd);
            }
            JournalEntry::CreatePipeV1 { read_fd, write_fd } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.insert(*read_fd);
                state.open_files.insert(*write_fd);
            }
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                let mut state = self.state.lock().unwrap();
                if state.open_files.remove(old_fd) {
                    state.open_files.insert(*new_fd);
                }
                if state.open_sockets.remove(old_fd) {
                    state.open_sockets.insert(*new_fd);
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
                if state.open_sockets.contains(original_fd) {
                    state.open_sockets.insert(*copied_fd);
                }
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.remove(fd);
                state.open_sockets.remove(fd);
            }
            JournalEntry::InitModuleV1 { .. }
            | JournalEntry::ClearEtherealV1 { .. }
            | JournalEntry::ProcessExitV1 { .. } => {
                let mut state = self.state.lock().unwrap();
                state.open_files.clear();
                state.open_sockets.clear();
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
            state.open_sockets.clear();
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
            state.open_sockets.clear();
        }
        self.inner.rollback()
    }
}

impl<R: ReadableJournal> ReadableJournal for AutoConsistentJournalRx<R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        Ok(Box::new(AutoConsistentJournalRx {
            inner: self.inner.as_restarted()?,
        }))
    }
}

impl<W: WritableJournal, R: ReadableJournal> WritableJournal for AutoConsistentJournal<W, R> {
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

impl<W: WritableJournal, R: ReadableJournal> ReadableJournal for AutoConsistentJournal<W, R> {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl Journal for AutoConsistentJournal<Box<DynWritableJournal>, Box<DynReadableJournal>> {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
