//! When wasmer self-update is executed, this is what gets executed
use anyhow::Result;
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
use structopt::StructOpt;

/// The options for the `wasmer self-update` subcommand
#[derive(Debug, StructOpt)]
pub struct SelfUpdate {}

impl SelfUpdate {
    #[cfg(not(target_os = "windows"))]
    /// The execute subcommand (for Unix)
    pub fn execute(&self) -> Result<()> {
        println!("Fetching latest installer");
        let cmd = Command::new("curl")
            .arg("https://get.wasmer.io")
            .arg("-sSfL")
            .stdout(Stdio::piped())
            .spawn()?;

        let mut process = Command::new("sh")
            .stdin(cmd.stdout.unwrap())
            .stdout(Stdio::inherit())
            .spawn()?;

        process.wait().unwrap();
        Ok(())
    }

    #[cfg(target_os = "windows")]
    /// The execute subcommand (for Windows)
    pub fn execute(&self) -> Result<()> {
        println!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
    }
}
