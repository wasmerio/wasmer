//! The logic for the Wasmer CLI tool.

#[cfg(target_os = "linux")]
use crate::commands::Binfmt;
#[cfg(feature = "compiler")]
use crate::commands::Compile;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
use crate::commands::CreateExe;
#[cfg(feature = "static-artifact-create")]
use crate::commands::CreateObj;
#[cfg(feature = "wast")]
use crate::commands::Wast;
use crate::commands::{
    Add, Cache, Config, Inspect, List, Login, Run, SelfUpdate, Validate, Whoami,
};
use crate::error::PrettyError;
use clap::{CommandFactory, ErrorKind, Parser};

#[derive(Parser, Debug)]
#[cfg_attr(
    not(feature = "headless"),
    clap(
        name = "wasmer",
        about = "WebAssembly standalone runtime.",
        version,
        author
    )
)]
#[cfg_attr(
    feature = "headless",
    clap(
        name = "wasmer-headless",
        about = "WebAssembly standalone runtime (headless).",
        version,
        author
    )
)]
/// The options for the wasmer Command Line Interface
enum WasmerCLIOptions {
    /// List all locally installed packages
    List(List),

    /// Run a WebAssembly file. Formats accepted: wasm, wat
    Run(Run),

    /// Login into a wapm.io-like registry
    Login(Login),

    /// Wasmer cache
    #[clap(subcommand)]
    Cache(Cache),

    /// Validate a WebAssembly binary
    Validate(Validate),

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    Compile(Compile),

    /// Compile a WebAssembly binary into a native executable
    ///
    /// To use, you need to set the `WASMER_DIR` environment variable
    /// to the location of your Wasmer installation. This will probably be `~/.wasmer`. It
    /// should include a `lib`, `include` and `bin` subdirectories. To create an executable
    /// you will need `libwasmer`, so by setting `WASMER_DIR` the CLI knows where to look for
    /// header files and libraries.
    ///
    /// Example usage:
    ///
    /// ```text
    /// $ # in two lines:
    /// $ export WASMER_DIR=/home/user/.wasmer/
    /// $ wasmer create-exe qjs.wasm -o qjs.exe # or in one line:
    /// $ WASMER_DIR=/home/user/.wasmer/ wasmer create-exe qjs.wasm -o qjs.exe
    /// $ file qjs.exe
    /// qjs.exe: ELF 64-bit LSB pie executable, x86-64 ...
    /// ```
    ///
    /// ## Cross-compilation
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    #[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
    #[clap(name = "create-exe", verbatim_doc_comment)]
    CreateExe(CreateExe),

    /// Compile a WebAssembly binary into an object file
    ///
    /// To use, you need to set the `WASMER_DIR` environment variable to the location of your
    /// Wasmer installation. This will probably be `~/.wasmer`. It should include a `lib`,
    /// `include` and `bin` subdirectories. To create an object you will need `libwasmer`, so by
    /// setting `WASMER_DIR` the CLI knows where to look for header files and libraries.
    ///
    /// Example usage:
    ///
    /// ```text
    /// $ # in two lines:
    /// $ export WASMER_DIR=/home/user/.wasmer/
    /// $ wasmer create-obj qjs.wasm --object-format symbols -o qjs.obj # or in one line:
    /// $ WASMER_DIR=/home/user/.wasmer/ wasmer create-exe qjs.wasm --object-format symbols -o qjs.obj
    /// $ file qjs.obj
    /// qjs.obj: ELF 64-bit LSB relocatable, x86-64 ...
    /// ```
    ///
    /// ## Cross-compilation
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    #[cfg(feature = "static-artifact-create")]
    #[structopt(name = "create-obj", verbatim_doc_comment)]
    CreateObj(CreateObj),

    /// Get various configuration information needed
    /// to compile programs which use Wasmer
    Config(Config),

    /// Update wasmer to the latest version
    #[clap(name = "self-update")]
    SelfUpdate(SelfUpdate),

    /// Inspect a WebAssembly file
    Inspect(Inspect),

    /// Run spec testsuite
    #[cfg(feature = "wast")]
    Wast(Wast),

    /// Unregister and/or register wasmer as binfmt interpreter
    #[cfg(target_os = "linux")]
    Binfmt(Binfmt),

    /// Shows the current logged in user for the current active registry
    Whoami(Whoami),

    /// Add a WAPM package's bindings to your application.
    Add(Add),
}

impl WasmerCLIOptions {
    fn execute(&self) -> Result<(), anyhow::Error> {
        match self {
            Self::Run(options) => options.execute(),
            Self::SelfUpdate(options) => options.execute(),
            Self::Cache(cache) => cache.execute(),
            Self::Validate(validate) => validate.execute(),
            #[cfg(feature = "compiler")]
            Self::Compile(compile) => compile.execute(),
            #[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
            Self::CreateExe(create_exe) => create_exe.execute(),
            #[cfg(feature = "static-artifact-create")]
            Self::CreateObj(create_obj) => create_obj.execute(),
            Self::Config(config) => config.execute(),
            Self::Inspect(inspect) => inspect.execute(),
            Self::List(list) => list.execute(),
            Self::Login(login) => login.execute(),
            #[cfg(feature = "wast")]
            Self::Wast(wast) => wast.execute(),
            #[cfg(target_os = "linux")]
            Self::Binfmt(binfmt) => binfmt.execute(),
            Self::Whoami(whoami) => whoami.execute(),
            Self::Add(install) => install.execute(),
        }
    }
}

/// The main function for the Wasmer CLI tool.
pub fn wasmer_main() {
    // We allow windows to print properly colors
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();

    PrettyError::report(wasmer_main_inner())
}

fn wasmer_main_inner() -> Result<(), anyhow::Error> {
    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    //
    // In case we've been run as wasmer-binfmt-interpreter myfile.wasm args,
    // we assume that we're registered via binfmt_misc
    let args = std::env::args().collect::<Vec<_>>();
    let binpath = args.get(0).map(|s| s.as_ref()).unwrap_or("");

    let firstarg = args.get(1).map(|s| s.as_str());
    let secondarg = args.get(2).map(|s| s.as_str());

    match (firstarg, secondarg) {
        (None, _) | (Some("help"), _) | (Some("--help"), _) => {
            return print_help(true);
        }
        (Some("-h"), _) => {
            return print_help(false);
        }
        (Some("-vV"), _)
        | (Some("version"), Some("--verbose"))
        | (Some("--version"), Some("--verbose")) => {
            return print_version(true);
        }

        (Some("-v"), _) | (Some("-V"), _) | (Some("version"), _) | (Some("--version"), _) => {
            return print_version(false);
        }
        _ => {}
    }

    let command = args.get(1);
    let options = if cfg!(target_os = "linux") && binpath.ends_with("wasmer-binfmt-interpreter") {
        WasmerCLIOptions::Run(Run::from_binfmt_args())
    } else {
        match command.unwrap_or(&"".to_string()).as_ref() {
            "add" | "cache" | "compile" | "config" | "create-exe" | "help" | "inspect" | "run"
            | "self-update" | "validate" | "wast" | "binfmt" | "list" | "login" => {
                WasmerCLIOptions::parse()
            }
            _ => {
                WasmerCLIOptions::try_parse_from(args.iter()).unwrap_or_else(|e| {
                    match e.kind() {
                        // This fixes a issue that:
                        // 1. Shows the version twice when doing `wasmer -V`
                        // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
                        ErrorKind::DisplayVersion | ErrorKind::DisplayHelp => e.exit(),
                        _ => WasmerCLIOptions::Run(Run::parse()),
                    }
                })
            }
        }
    };

    options.execute()
}

fn print_help(verbose: bool) -> Result<(), anyhow::Error> {
    let mut cmd = WasmerCLIOptions::command();
    if verbose {
        let _ = cmd.print_long_help();
    } else {
        let _ = cmd.print_help();
    }
    Ok(())
}

#[allow(unused_mut, clippy::vec_init_then_push)]
fn print_version(verbose: bool) -> Result<(), anyhow::Error> {
    if !verbose {
        println!("wasmer {}", env!("CARGO_PKG_VERSION"));
    } else {
        println!(
            "wasmer {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            env!("WASMER_BUILD_GIT_HASH_SHORT"),
            env!("WASMER_BUILD_DATE")
        );
        println!("binary: {}", env!("CARGO_PKG_NAME"));
        println!("commit-hash: {}", env!("WASMER_BUILD_GIT_HASH"));
        println!("commit-date: {}", env!("WASMER_BUILD_DATE"));
        println!("host: {}", target_lexicon::HOST);
        println!("compiler: {}", {
            let mut s = Vec::<&'static str>::new();

            #[cfg(feature = "singlepass")]
            s.push("singlepass");
            #[cfg(feature = "cranelift")]
            s.push("cranelift");
            #[cfg(feature = "llvm")]
            s.push("llvm");

            s.join(",")
        });
    }
    Ok(())
}
