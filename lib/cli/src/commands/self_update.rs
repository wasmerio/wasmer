use anyhow::Context as _;

/// The options for the `wasmer self-update` subcommand
#[derive(clap::Parser, Debug)]
pub struct SelfUpdate {}

impl SelfUpdate {
    /// Runs logic for the `self-update` subcommand
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        self.inner_execute().context("failed to self-update wasmer")
    }

    #[cfg(not(target_os = "windows"))]
    fn inner_execute(&self) -> Result<(), anyhow::Error> {
        use std::process::{Command, Stdio};

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
    fn inner_execute(&self) -> Result<(), anyhow::Error> {
        anyhow::bail!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
    }
}
