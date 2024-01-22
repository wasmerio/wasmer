use std::path::PathBuf;

use clap::Parser;
use wasmer_edge_cli::cmd::AsyncCliCommand;
use wasmer_wasix::journal::{copy_journal, LogFileJournal, PrintingJournal};

/// Prints a summarized version of contents of a journal to stdout
#[derive(Debug, Parser)]
pub struct CmdJournalInspect {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl AsyncCliCommand for CmdJournalInspect {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}

impl CmdJournalInspect {
    async fn run(self) -> Result<(), anyhow::Error> {
        let journal = LogFileJournal::new(self.journal_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
