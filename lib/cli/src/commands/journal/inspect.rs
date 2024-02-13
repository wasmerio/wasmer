use std::path::PathBuf;

use clap::Parser;
use wasmer_wasix::journal::{copy_journal, LogFileJournal, PrintingJournal};

use crate::commands::CliCommand;

/// Prints a summarized version of contents of a journal to stdout
#[derive(Debug, Parser)]
pub struct CmdJournalInspect {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl CliCommand for CmdJournalInspect {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        let journal = LogFileJournal::new(self.journal_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
