use std::path::PathBuf;

use clap::Parser;
use wasmer_backend_cli::cmd::CliCommand;
use wasmer_wasix::journal::{copy_journal, LogFileJournal, PrintingJournal};

/// Prints a summarized version of contents of a journal to stdout
#[derive(Debug, Parser)]
pub struct CmdJournaInspect {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl CliCommand for CmdJournaInspect {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        let journal = LogFileJournal::new(self.journal_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
