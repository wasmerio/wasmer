//! The logic for the Wasmer CLI tool.

use crate::commands::Compile;
use crate::commands::{Config, Validate};
use crate::error::PrettyError;
use anyhow::Result;

use clap::{error::ErrorKind, Parser};

#[derive(Parser)]
#[clap(
    name = "wasmer-compiler",
    about = "WebAssembly standalone Compiler.",
    author
)]
/// The options for the wasmer Command Line Interface
enum WasmerCLIOptions {
    /// Validate a WebAssembly binary
    #[clap(name = "validate")]
    Validate(Validate),

    /// Compile a WebAssembly binary
    #[clap(name = "compile")]
    Compile(Compile),

    /// Get various configuration information needed
    /// to compile programs which use Wasmer
    #[clap(name = "config")]
    Config(Config),
}

impl WasmerCLIOptions {
    fn execute(&self) -> Result<()> {
        match self {
            Self::Validate(validate) => validate.execute(),
            Self::Compile(compile) => compile.execute(),
            Self::Config(config) => config.execute(),
        }
    }
}

/// The main function for the Wasmer CLI tool.
pub fn wasmer_main() {
    // We allow windows to print properly colors
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();

    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    //
    // In case we've been run as wasmer-binfmt-interpreter myfile.wasm args,
    // we assume that we're registered via binfmt_misc
    let args = std::env::args().collect::<Vec<_>>();
    let command = args.get(1);
    let options = {
        match command.unwrap_or(&"".to_string()).as_ref() {
            "compile" | "config" | "help" | "inspect" | "validate" => WasmerCLIOptions::parse(),
            _ => {
                WasmerCLIOptions::try_parse_from(args.iter()).unwrap_or_else(|e| {
                    match e.kind() {
                        // This fixes a issue that:
                        // 1. Shows the version twice when doing `wasmer -V`
                        // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
                        ErrorKind::DisplayVersion | ErrorKind::DisplayHelp => e.exit(),
                        _ => WasmerCLIOptions::Compile(Compile::parse()),
                    }
                })
            }
        }
    };

    PrettyError::report(options.execute());
}
