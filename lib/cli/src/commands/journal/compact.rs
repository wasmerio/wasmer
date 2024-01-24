use std::path::PathBuf;

use clap::Parser;
use wasmer_backend_cli::cmd::AsyncCliCommand;
use wasmer_wasix::journal::{
    copy_journal, CompactingLogFileJournal, LogFileJournal, PrintingJournal,
};

/// Compacts a journal by removing duplicate or redundant
/// events and rewriting the log
#[derive(Debug, Parser)]
pub struct CmdJournalCompact {
    /// Path to the journal that will be compacted
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl AsyncCliCommand for CmdJournalCompact {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}

impl CmdJournalCompact {
    async fn run(self) -> Result<(), anyhow::Error> {
        let compactor = CompactingLogFileJournal::new(&self.journal_path)?.with_compact_on_drop();
        drop(compactor);

        let journal = LogFileJournal::new(&self.journal_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
