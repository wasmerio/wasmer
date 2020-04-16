#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#[cfg(all(target_os = "linux", feature = "loader-kernel"))]
use wasmer_bin::commands::Kernel;
#[cfg(any(
    feature = "backend-cranelift",
    feature = "backend-llvm",
    feature = "backend-singlepass"
))]
use wasmer_bin::commands::Run;
use wasmer_bin::commands::{Cache, SelfUpdate, Validate};

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WebAssembly standalone runtime.", author)]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wat
    #[cfg(any(
        feature = "backend-cranelift",
        feature = "backend-llvm",
        feature = "backend-singlepass"
    ))]
    #[structopt(name = "run")]
    Run(Run),

    /// Wasmer cache
    #[structopt(name = "cache")]
    Cache(Cache),

    /// Validate a Web Assembly binary
    #[structopt(name = "validate")]
    Validate(Validate),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate,

    /// The Wasm kernel loader
    #[cfg(all(target_os = "linux", feature = "loader-kernel"))]
    #[structopt(name = "self-update")]
    Kernel(Kernel),
}

fn main() {
    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    #[cfg(any(
        feature = "backend-cranelift",
        feature = "backend-llvm",
        feature = "backend-singlepass"
    ))]
    let options = CLIOptions::from_iter_safe(std::env::args()).unwrap_or_else(|e| {
        match e.kind {
            // This fixes a issue that:
            // 1. Shows the version twice when doing `wasmer -V`
            // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
            structopt::clap::ErrorKind::VersionDisplayed
            | structopt::clap::ErrorKind::HelpDisplayed => e.exit(),
            _ => CLIOptions::Run(Run::from_args()),
        }
    });

    // Do not try to wrap in Run, if the Run subcommand is not available
    #[cfg(not(any(
        feature = "backend-cranelift",
        feature = "backend-llvm",
        feature = "backend-singlepass"
    )))]
    let options = CLIOptions::from_args();

    match options {
        #[cfg(any(
            feature = "backend-cranelift",
            feature = "backend-llvm",
            feature = "backend-singlepass"
        ))]
        CLIOptions::Run(mut options) => options.execute(),
        CLIOptions::SelfUpdate => SelfUpdate::execute(),
        CLIOptions::Cache(cache) => cache.execute(),
        CLIOptions::Validate(validate) => validate.execute(),
        #[cfg(all(target_os = "linux", feature = "loader-kernel"))]
        CLIOptions::Kernel(kernel) => kernel.execute(),
    }
}
