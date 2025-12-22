use crate::commands::CliCommand;

mod compact;
mod export;
mod extract;
mod filter;
mod import;
mod inspect;

pub use compact::*;
pub use export::*;
pub use extract::*;
pub use filter::*;
pub use import::*;
pub use inspect::*;

/// Manage Journal files.
#[derive(clap::Subcommand, Debug)]
pub enum CmdJournal {
    /// Compacts a journal into a smaller size by removed redundant or duplicate events
    Compact(CmdJournalCompact),
    /// Exports the contents of a journal to stdout as JSON objects
    Export(CmdJournalExport),
    /// Imports the events into a journal as JSON objects
    Import(CmdJournalImport),
    /// Inspects the contents of a journal and summarizes it to `stdout`
    Inspect(CmdJournalInspect),
    /// Filters out certain events from a journal
    Filter(CmdJournalFilter),
    /// Extracts an element of a journal
    Extract(CmdJournalExtract),
}

impl CliCommand for CmdJournal {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Compact(cmd) => cmd.run(),
            Self::Import(cmd) => cmd.run(),
            Self::Export(cmd) => cmd.run(),
            Self::Inspect(cmd) => cmd.run(),
            Self::Filter(cmd) => cmd.run(),
            Self::Extract(cmd) => cmd.run(),
        }
    }
}
