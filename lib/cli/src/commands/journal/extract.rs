use std::path::PathBuf;

use clap::Parser;
use wasmer_wasix::journal::{copy_journal, LogFileJournal};

use crate::commands::CliCommand;

#[derive(Debug, Parser)]
pub struct CmdExtractWhatMemory {
    /// Path to the memory file that will be updated using this journal
    #[clap(index = 1)]
    memory_file_path: PathBuf,
}

/// What to extract from the journal
#[derive(clap::Subcommand, Debug)]
pub enum CmdExtractWhat {
    Memory(CmdExtractWhatMemory),
}

/// Extracts an element from the journal
#[derive(Debug, Parser)]
pub struct CmdJournalExtract {
    /// Path to the journal that will be compacted
    #[clap(index = 1)]
    journal_path: PathBuf,

    #[clap(subcommand)]
    what: CmdExtractWhat,
}

impl CliCommand for CmdJournalExtract {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        let journal = LogFileJournal::new(&self.journal_path)?;

        match self.what {
            CmdExtractWhat::Memory(cmd) => {
                let memory_file =
                    wasmer_wasix::journal::MemFileJournal::new(&cmd.memory_file_path)?;
                copy_journal(&journal, &memory_file)?;
            }
        }
        Ok(())
    }
}
