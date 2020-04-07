use super::util;
use super::wasi_version::*;

use std::process::Command;

fn install_toolchain(toolchain_name: &str) {
    println!("Installing rustup toolchain: {}", toolchain_name);
    let rustup_out = Command::new("rustup")
        .arg("toolchain")
        .arg("install")
        .arg(toolchain_name)
        .output()
        .expect("Failed to install toolchain with rustup");
    util::print_info_on_error(&rustup_out, "TOOLCHAIN INSTALL FAILED");
}

pub fn set_it_up(only_latest: bool) {
    println!("Setting up system to generate the WASI tests.");
    println!("WARNING: this may use a lot of disk space.");

    let wasi_versions = if only_latest {
        println!("Only installing the toolchain for the latest WASI version");
        LATEST_WASI_VERSION
    } else {
        println!("Installing the toolchain for all WASI versions");
        ALL_WASI_VERSIONS
    };
    for wasi_version in wasi_versions {
        install_toolchain(wasi_version.get_compiler_toolchain());
    }
}
