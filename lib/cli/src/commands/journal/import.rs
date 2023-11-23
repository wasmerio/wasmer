use std::{io::ErrorKind, path::PathBuf};

use clap::Parser;
use wasmer_deploy_cli::cmd::AsyncCliCommand;
use wasmer_wasix::journal::{JournalEntry, LogFileJournal, WritableJournal};

/// Imports events into a journal file. Events are streamed as JSON
/// objects into `stdin`
#[derive(Debug, Parser)]
pub struct CmdJournaImport {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
}

impl AsyncCliCommand for CmdJournaImport {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}

impl CmdJournaImport {
    async fn run(self) -> Result<(), anyhow::Error> {
        // Erase the journal file at the path and reopen it
        if self.journal_path.exists() {
            std::fs::remove_file(&self.journal_path)?;
        }
        let journal = LogFileJournal::new(self.journal_path)?;

        // Read all the events from `stdin`, deserialize them and save them to the journal
        let stdin = std::io::stdin();
        let mut lines = String::new();
        let mut line = String::new();
        loop {
            // Read until the end marker
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) => return Ok(()),
                    Ok(_) => {
                        lines.push_str(&line);
                        if line.trim_end() == "}" {
                            break;
                        }
                    }
                    Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(()),
                    Err(err) => return Err(err.into()),
                }
            }

            let record = serde_json::from_str::<JournalEntry<'static>>(&lines)?;
            println!("{}", record);
            journal.write(record)?;
            lines.clear();
        }
    }
}
