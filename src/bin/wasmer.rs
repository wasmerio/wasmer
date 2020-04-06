#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
use std::env;
use wasmer_bin::commands::{Cache, Run, SelfUpdate, Validate};

use structopt::{clap, StructOpt};

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WebAssembly standalone runtime.", author)]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wat
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
}

fn main() {
    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    let args = env::args();
    let options = CLIOptions::from_iter_safe(args).unwrap_or_else(|e| {
        match e.kind {
            // This fixes a issue that:
            // 1. Shows the version twice when doing `wasmer -V`
            // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
            clap::ErrorKind::VersionDisplayed | clap::ErrorKind::HelpDisplayed => e.exit(),
            _ => CLIOptions::Run(Run::from_args()),
        }
    });

    match options {
        CLIOptions::Run(mut options) => options.execute(),
        CLIOptions::SelfUpdate => SelfUpdate::execute(),
        CLIOptions::Cache(cache) => cache.execute(),
        CLIOptions::Validate(validate) => validate.execute(),
    }
}
