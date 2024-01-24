use wasmer_backend_cli::cmd::CliCommand;

mod compact;
mod export;
mod filter;
mod import;
mod inspect;

pub use compact::*;
pub use export::*;
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
    Import(CmdJournaImport),
    /// Inspects the contents of a journal and summarizes it to `stdout`
    Inspect(CmdJournaInspect),
    /// Filters out certain events from a journal
    Filter(CmdJournalFilter),
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
        }
    }
}
