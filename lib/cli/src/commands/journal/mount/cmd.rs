use std::{path::PathBuf, process::Stdio};

use clap::Parser;
use wasmer_wasix::fs::WasiFdSeed;

use super::fs::JournalFileSystemBuilder;
use crate::commands::CliCommand;

/// Mounts a journal as a file system on the local machine
#[derive(Debug, Parser)]
pub struct CmdJournalMount {
    /// Path to the journal that will be printed
    #[clap(index = 1)]
    journal_path: PathBuf,
    /// Path to the directory where the file system will be mounted
    #[clap(index = 2)]
    mount_path: PathBuf,
}

impl CliCommand for CmdJournalMount {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        // First we unmount any existing file system on this path
        std::process::Command::new("/bin/umount")
            .arg(self.mount_path.to_string_lossy().as_ref())
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?
            .wait()
            .ok();

        let fs = JournalFileSystemBuilder::new(&self.journal_path)
            .with_fd_seed(WasiFdSeed::default())
            .with_progress_bar(false)
            .build()?;

        // Mounts the journal file system at a path
        fuse::mount(fs, &self.mount_path, &[])?;
        Ok(())
    }
}
