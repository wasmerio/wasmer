use std::{path::PathBuf, str::FromStr};

use clap::Parser;
use wasmer_wasix::journal::{
    copy_journal, FilteredJournalBuilder, LogFileJournal, PrintingJournal,
};

use crate::commands::CliCommand;

/// Flags that specify what should be filtered out
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum FilterOut {
    /// Filters out all the memory events
    Memory,
    /// Filters out all the thread stacks
    Threads,
    /// Filters out all the file system operations
    FileSystem,
    /// Filters out all core syscalls
    Core,
    /// Filters out the snapshots
    Snapshots,
    /// Filters out the networking
    Networking,
}

impl FromStr for FilterOut {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "mem" | "memory" => Self::Memory,
            "thread" | "threads" => Self::Threads,
            "fs" | "file" | "filesystem" | "file-system" => Self::FileSystem,
            "core" => Self::Core,
            "snap" | "snapshot" | "snapshots" => Self::Snapshots,
            "net" | "network" | "networking" => Self::Networking,
            t => return Err(format!("unknown filter type - {t}")),
        })
    }
}

/// Rewrites a journal log removing events that match the
/// filter parameters.
#[derive(Debug, Parser)]
pub struct CmdJournalFilter {
    /// Path to the journal that will be read
    #[clap(index = 1)]
    source_path: PathBuf,
    /// Path to the journal that will be the output of the filter
    #[clap(index = 2)]
    target_path: PathBuf,
    /// Filters to be applied to the output journal, filter options are
    /// - 'mem' | 'memory' -> removes all WASM memory related events
    /// - 'thread' | 'threads' -> removes all events related to the state of the threads
    /// - 'fs' | 'file' -> removes file system mutation events
    /// - 'core' -> removes core operating system operations such as TTY
    /// - 'snap' | 'snapshot' -> removes the snapshots from the journal
    /// - 'net' | 'network' -> removes network socket and interface events
    #[clap(short, long = "filter")]
    filters: Vec<FilterOut>,
}

impl CliCommand for CmdJournalFilter {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        // Create a new file name that will be the temp new file
        // while its written
        let mut temp_filename = self
            .target_path
            .file_name()
            .ok_or_else(|| {
                anyhow::format_err!(
                    "The path is not a valid filename - {}",
                    self.target_path.to_string_lossy()
                )
            })?
            .to_string_lossy()
            .to_string();
        temp_filename.insert_str(0, ".staging.");
        let temp_path = self.target_path.with_file_name(&temp_filename);
        std::fs::remove_file(&temp_path).ok();

        // Load the source journal and the target journal (in the temp location)
        let source = LogFileJournal::new(self.source_path)?;
        let target = LogFileJournal::new(temp_path.clone())?;

        // Put a filter on the farget
        let mut builder = FilteredJournalBuilder::new();
        for filter in self.filters {
            builder = match filter {
                FilterOut::Memory => builder.with_ignore_memory(true),
                FilterOut::Threads => builder.with_ignore_threads(true),
                FilterOut::FileSystem => builder.with_ignore_fs(true),
                FilterOut::Core => builder.with_ignore_core(true),
                FilterOut::Snapshots => builder.with_ignore_snapshots(true),
                FilterOut::Networking => builder.with_ignore_networking(true),
            }
        }
        let target = builder.build(target);

        // Copy the journal over and rename the temp file to the target file
        copy_journal(&source, &target)?;
        drop(target);
        std::fs::rename(temp_path, self.target_path.clone())?;

        // Now print the outcome
        let journal = LogFileJournal::new(&self.target_path)?;
        let printer = PrintingJournal::default();
        copy_journal(&journal, &printer)?;
        Ok(())
    }
}
