use std::path::PathBuf;

use clap::Parser;
use wasmer_wasix::journal::{
    copy_journal, CompactingLogFileJournal, LogFileJournal, PrintingJournal,
};

use crate::commands::CliCommand;

/// Compacts a journal by removing duplicate or redundant
/// events and rewriting the log
#[derive(Debug, Parser)]
pub struct CmdJournalCompact {
    /// Path to the journal that will be compacted
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl CliCommand for CmdJournalCompact {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        let compactor = CompactingLogFileJournal::new(&self.journal_path)?.with_compact_on_drop();
        drop(compactor);

        let journal = LogFileJournal::new(&self.journal_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
