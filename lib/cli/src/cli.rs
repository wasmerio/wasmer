//! The logic for the Wasmer CLI tool.

#[cfg(target_os = "linux")]
use crate::commands::Binfmt;
#[cfg(feature = "compiler")]
use crate::commands::Compile;
#[cfg(all(feature = "staticlib", feature = "compiler"))]
use crate::commands::CreateExe;
#[cfg(feature = "wast")]
use crate::commands::Wast;
use crate::commands::{Cache, Config, Inspect, Run, SelfUpdate, Validate};
use crate::error::PrettyError;
use anyhow::Result;

use structopt::{clap::ErrorKind, StructOpt};

#[derive(StructOpt)]
#[cfg_attr(
    not(feature = "headless"),
    structopt(name = "wasmer", about = "WebAssembly standalone runtime.", author)
)]
#[cfg_attr(
    feature = "headless",
    structopt(
        name = "wasmer-headless",
        about = "Headless WebAssembly standalone runtime.",
        author
    )
)]
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

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    #[structopt(name = "compile")]
    Compile(Compile),

    /// Compile a WebAssembly binary into a native executable
    #[cfg(all(feature = "staticlib", feature = "compiler"))]
    #[structopt(name = "create-exe")]
    CreateExe(CreateExe),

    /// Get various configuration information needed
    /// to compile programs which use Wasmer
    #[structopt(name = "config")]
    Config(Config),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate(SelfUpdate),

    /// Inspect a WebAssembly file
    #[structopt(name = "inspect")]
    Inspect(Inspect),

    /// Run spec testsuite
    #[cfg(feature = "wast")]
    #[structopt(name = "wast")]
    Wast(Wast),

    /// Unregister and/or register wasmer as binfmt interpreter
    #[cfg(target_os = "linux")]
    #[structopt(name = "binfmt")]
    Binfmt(Binfmt),
}

impl WasmerCLIOptions {
    fn execute(&self) -> Result<()> {
        match self {
            Self::Run(options) => options.execute(),
            Self::SelfUpdate(options) => options.execute(),
            Self::Cache(cache) => cache.execute(),
            Self::Validate(validate) => validate.execute(),
            #[cfg(feature = "compiler")]
            Self::Compile(compile) => compile.execute(),
            #[cfg(all(feature = "staticlib", feature = "compiler"))]
            Self::CreateExe(create_exe) => create_exe.execute(),
            Self::Config(config) => config.execute(),
            Self::Inspect(inspect) => inspect.execute(),
            #[cfg(feature = "wast")]
            Self::Wast(wast) => wast.execute(),
            #[cfg(target_os = "linux")]
            Self::Binfmt(binfmt) => binfmt.execute(),
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
    let binpath = args.get(0).map(|s| s.as_ref()).unwrap_or("");
    let command = args.get(1);
    let options = if cfg!(target_os = "linux") && binpath.ends_with("wasmer-binfmt-interpreter") {
        WasmerCLIOptions::Run(Run::from_binfmt_args())
    } else {
        match command.unwrap_or(&"".to_string()).as_ref() {
            "cache" | "compile" | "config" | "create-exe" | "help" | "inspect" | "run"
            | "self-update" | "validate" | "wast" | "binfmt" => WasmerCLIOptions::from_args(),
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
        }
    };

    PrettyError::report(options.execute());
}
