use std::path::PathBuf;

use clap::Parser;
use wasmer_wasix::journal::{copy_journal, JournalPrintingMode, LogFileJournal, PrintingJournal};

use crate::commands::CliCommand;

/// Exports all the events in a journal to STDOUT as JSON data
#[derive(Debug, Parser)]
pub struct CmdJournalExport {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl CliCommand for CmdJournalExport {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        let journal = LogFileJournal::new(self.journal_path)?;
        let printer = PrintingJournal::new(JournalPrintingMode::Json);
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
