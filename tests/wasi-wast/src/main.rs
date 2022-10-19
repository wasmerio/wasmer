#[macro_use]
extern crate serde;

mod set_up_toolchain;
mod util;
mod wasi_version;
mod wasitests;

pub use crate::set_up_toolchain::install_toolchains;
pub use crate::wasi_version::{
    WasiVersion, ALL_WASI_VERSIONS, LATEST_WASI_VERSION, NIGHTLY_VERSION,
};
pub use crate::wasitests::{build, WasiOptions, WasiTest};

use gumdrop::Options;

#[derive(Debug, Options)]
pub struct TestGenOptions {
    /// if you want to specify specific tests to generate
    #[options(free)]
    free: Vec<String>,
    /// Whether to use the current nightly instead of the latest snapshot0 compiler
    nightly: bool,
    /// Whether or not to do operations for all versions of WASI or just the latest.
    all_versions: bool,
    /// Whether or not the Wasm will be generated.
    generate_wasm: bool,
    /// Whether or not the logic to install the needed Rust compilers is run.
    set_up_toolchain: bool,
    /// Print the help message
    help: bool,
}

fn main() {
    let opts = TestGenOptions::parse_args_default_or_exit();

    if opts.help {
        println!("{}", TestGenOptions::usage());
        std::process::exit(0);
    }

    let generate_all = opts.all_versions;
    let set_up_toolchain = opts.set_up_toolchain;
    let generate_wasm = opts.generate_wasm;
    let nightly = opts.nightly;
    let wasi_versions = if generate_all {
        ALL_WASI_VERSIONS
    } else if nightly {
        NIGHTLY_VERSION
    } else {
        LATEST_WASI_VERSION
    };

    // Install the Rust WASI toolchains for each of the versions
    if set_up_toolchain {
        install_toolchains(wasi_versions);
    }

    // Generate the WASI Wasm files
    if generate_wasm {
        let specific_tests: Vec<&str> = opts.free.iter().map(|st| st.as_str()).collect();
        build(wasi_versions, &specific_tests);
    }
}
