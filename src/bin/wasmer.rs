use anyhow::Result;
#[cfg(feature = "wast")]
use wasmer_bin::commands::Wast;
use wasmer_bin::commands::{Cache, Run, SelfUpdate, Validate};

use structopt::{clap::ErrorKind, StructOpt};

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WebAssembly standalone runtime.", author)]
/// The options for the wasmer Command Line Interface
enum WasmerCLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wat
    #[structopt(name = "run")]
    Run(Run),

    /// Wasmer cache
    #[structopt(name = "cache")]
    Cache(Cache),

    /// Validate a WebAssembly binary
    #[structopt(name = "validate")]
    Validate(Validate),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate(SelfUpdate),

    /// Run spec testsuite
    #[cfg(feature = "wast")]
    #[structopt(name = "wast")]
    Wast(Wast),
}

impl WasmerCLIOptions {
    fn execute(&self) -> Result<()> {
        match self {
            Self::Run(options) => options.execute(),
            Self::SelfUpdate(options) => options.execute(),
            Self::Cache(cache) => cache.execute(),
            Self::Validate(validate) => validate.execute(),
            #[cfg(feature = "wast")]
            Self::Wast(wast) => wast.execute(),
        }
    }
}

fn main() -> Result<()> {
    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    let options = WasmerCLIOptions::from_iter_safe(std::env::args()).unwrap_or_else(|e| {
        match e.kind {
            // This fixes a issue that:
            // 1. Shows the version twice when doing `wasmer -V`
            // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
            ErrorKind::VersionDisplayed | ErrorKind::HelpDisplayed => e.exit(),
            _ => WasmerCLIOptions::Run(Run::from_args()),
        }
    });
    options.execute()
}
