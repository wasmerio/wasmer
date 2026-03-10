//! When wasmer self-update is executed, this is what gets executed
use std::process::{Command, Stdio};

pub fn self_update() {
    println!("Fetching latest installer");
    let cmd = Command::new("curl")
        .arg("https://get.wasmer.io")
        .arg("-sSfL")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut the_process = Command::new("sh")
        .stdin(cmd.stdout.unwrap())
        .stdout(Stdio::inherit())
        .spawn()
        .ok()
        .expect("Failed to execute.");

    the_process.wait().unwrap();
}
