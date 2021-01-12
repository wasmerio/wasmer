use anyhow::Result;
use wasmer_cli::commands::{Config, Run};
use wasmer_cli::error::PrettyError;

use structopt::{clap::ErrorKind, StructOpt};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "wasmer-headless",
    about = "Headless WebAssembly standalone runtime.",
    author
)]
/// The options for the wasmer Command Line Interface
enum WasmerCLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wat
    #[structopt(name = "run")]
    Run(Run),

    /// Get various configuration information needed
    /// to compile programs which use Wasmer
    #[structopt(name = "config")]
    Config(Config),
}

impl WasmerCLIOptions {
    fn execute(&self) -> Result<()> {
        match self {
            Self::Run(options) => options.execute(),
            Self::Config(config) => config.execute(),
        }
    }
}

fn main() {
    // We allow windows to print properly colors
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();

    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    let args = std::env::args().collect::<Vec<_>>();
    let command = args.get(1);
    let options = match command.unwrap_or(&"".to_string()).as_ref() {
        "config" | "run" => WasmerCLIOptions::from_args(),
        _ => {
            WasmerCLIOptions::from_iter_safe(args.iter()).unwrap_or_else(|e| {
                match e.kind {
                    // This fixes a issue that:
                    // 1. Shows the version twice when doing `wasmer -V`
                    // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
                    ErrorKind::VersionDisplayed | ErrorKind::HelpDisplayed => e.exit(),
                    _ => WasmerCLIOptions::Run(Run::from_args()),
                }
            })
        }
    };

    PrettyError::report(options.execute());
}
