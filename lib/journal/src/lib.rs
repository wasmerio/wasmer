mod base64;
mod concrete;
mod entry;
mod snapshot;
mod util;

pub use concrete::*;
pub use entry::*;
pub use snapshot::*;
pub use util::*;

use serde::{Deserialize, Serialize};
use std::{ops::Deref, str::FromStr};

/// The results of an operation to write a log entry to the log
#[derive(Debug)]
pub struct LogWriteResult {
    // Start of the actual entry
    pub record_start: u64,
    // End of the actual entry
    pub record_end: u64,
}

impl LogWriteResult {
    pub fn record_size(&self) -> u64 {
        self.record_end - self.record_start
    }
}

/// The snapshot capturer will take a series of objects that represents the state of
/// a WASM process at a point in time and saves it so that it can be restored.
/// It also allows for the restoration of that state at a later moment
#[allow(unused_variables)]
pub trait WritableJournal {
    /// Takes in a stream of snapshot log entries and saves them so that they
    /// may be restored at a later moment
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult>;
}

/// The results of an operation to read a log entry from the log
#[derive(Debug)]
pub struct LogReadResult<'a> {
    /// Offset into the journal where this entry exists
    pub record_start: u64,
    /// Offset of the end of the entry
    pub record_end: u64,
    /// Represents the journal entry
    pub record: JournalEntry<'a>,
}

impl<'a> LogReadResult<'a> {
    pub fn into_inner(self) -> JournalEntry<'a> {
        self.record
    }
}

impl<'a> Deref for LogReadResult<'a> {
    type Target = JournalEntry<'a>;

    fn deref(&self) -> &Self::Target {
        &self.record
    }
}

/// The snapshot capturer will take a series of objects that represents the state of
/// a WASM process at a point in time and saves it so that it can be restored.
/// It also allows for the restoration of that state at a later moment
#[allow(unused_variables)]
pub trait ReadableJournal {
    /// Returns a stream of snapshot objects that the runtime will use
    /// to restore the state of a WASM process to a previous moment in time
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>>;

    /// Resets the journal so that reads will start from the
    /// beginning again
    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>>;
}

/// The snapshot capturer will take a series of objects that represents the state of
/// a WASM process at a point in time and saves it so that it can be restored.
/// It also allows for the restoration of that state at a later moment
#[allow(unused_variables)]
pub trait Journal: WritableJournal + ReadableJournal {
    /// Splits the journal into a read and write side
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>);
}

pub type DynJournal = dyn Journal + Send + Sync;
pub type DynWritableJournal = dyn WritableJournal + Send + Sync;
pub type DynReadableJournal = dyn ReadableJournal + Send + Sync;
