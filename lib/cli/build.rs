use std::process::Command;

use chrono::prelude::*;

pub fn main() {
    // Set WASMER_GIT_HASH
    let git_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=WASMER_BUILD_GIT_HASH={git_hash}");

    if git_hash.len() > 5 {
        println!(
            "cargo:rustc-env=WASMER_BUILD_GIT_HASH_SHORT={}",
            &git_hash[..7]
        );
    } else {
        println!("cargo:rustc-env=WASMER_BUILD_GIT_HASH_SHORT=???????");
    }

    let utc: DateTime<Utc> = Utc::now();
    let date = utc.format("%Y-%m-%d").to_string();
    println!("cargo:rustc-env=WASMER_BUILD_DATE={date}");
}
