//! When wasmer self-update is executed, this is what gets executed
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};

pub struct SelfUpdate;

impl SelfUpdate {
    #[cfg(not(target_os = "windows"))]
    pub fn execute() {
        println!("Fetching latest installer");
        let cmd = Command::new("curl")
            .arg("https://get.wasmer.io")
            .arg("-sSfL")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut process = Command::new("sh")
            .stdin(cmd.stdout.unwrap())
            .stdout(Stdio::inherit())
            .spawn()
            .ok()
            .expect("Failed to execute.");

        process.wait().unwrap();
    }

    #[cfg(target_os = "windows")]
    pub fn execute() {
        println!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
    }
}
