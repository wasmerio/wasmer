use super::util;
use super::wasi_version::*;

use std::process::Command;

fn install_toolchain(toolchain_name: &str) {
    println!("Installing rustup toolchain: {toolchain_name}");
    let rustup_out = Command::new("rustup")
        .arg("toolchain")
        .arg("install")
        .arg(toolchain_name)
        .output()
        .expect("Failed to install toolchain with rustup");
    util::print_info_on_error(&rustup_out, "TOOLCHAIN INSTALL FAILED");

    println!("Installing rustup WASI target");
    let rustup_out = Command::new("rustup")
        .arg("target")
        .arg("add")
        .arg("wasm32-wasip1")
        .arg("--toolchain")
        .arg(toolchain_name)
        .output()
        .expect("Failed to wasi target in Rust toolchain");
    util::print_info_on_error(&rustup_out, "WASI TARGET IN TOOLCHAIN INSTALL FAILED");
}

pub fn install_toolchains(wasi_versions: &[WasiVersion]) {
    println!("Setting up system to generate the WASI tests.");
    println!("WARNING: this may use a lot of disk space.");

    for wasi_version in wasi_versions {
        install_toolchain(wasi_version.get_compiler_toolchain());
    }
}
