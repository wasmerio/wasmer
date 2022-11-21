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
use std::{fmt, str::FromStr};

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

    // Check if the file is a package name
    if let WasmerCLIOptions::Run(r) = &options {
        #[cfg(not(feature = "debug"))]
        let debug = false;
        #[cfg(feature = "debug")]
        let debug = r.options.debug;
        return crate::commands::try_run_package_or_file(&args, r, debug);
    }

    options.execute()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SplitVersion {
    pub(crate) original: String,
    pub(crate) registry: Option<String>,
    pub(crate) package: String,
    pub(crate) version: Option<String>,
    pub(crate) command: Option<String>,
}

impl fmt::Display for SplitVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let version = self.version.as_deref().unwrap_or("latest");
        let command = self
            .command
            .as_ref()
            .map(|s| format!(":{s}"))
            .unwrap_or_default();
        write!(f, "{}@{version}{command}", self.package)
    }
}

#[test]
fn test_split_version() {
    assert_eq!(
        SplitVersion::parse("registry.wapm.io/graphql/python/python").unwrap(),
        SplitVersion {
            original: "registry.wapm.io/graphql/python/python".to_string(),
            registry: Some("https://registry.wapm.io/graphql".to_string()),
            package: "python/python".to_string(),
            version: None,
            command: None,
        }
    );
    assert_eq!(
        SplitVersion::parse("registry.wapm.io/python/python").unwrap(),
        SplitVersion {
            original: "registry.wapm.io/python/python".to_string(),
            registry: Some("https://registry.wapm.io/graphql".to_string()),
            package: "python/python".to_string(),
            version: None,
            command: None,
        }
    );
    assert_eq!(
        SplitVersion::parse("namespace/name@version:command").unwrap(),
        SplitVersion {
            original: "namespace/name@version:command".to_string(),
            registry: None,
            package: "namespace/name".to_string(),
            version: Some("version".to_string()),
            command: Some("command".to_string()),
        }
    );
    assert_eq!(
        SplitVersion::parse("namespace/name@version").unwrap(),
        SplitVersion {
            original: "namespace/name@version".to_string(),
            registry: None,
            package: "namespace/name".to_string(),
            version: Some("version".to_string()),
            command: None,
        }
    );
    assert_eq!(
        SplitVersion::parse("namespace/name").unwrap(),
        SplitVersion {
            original: "namespace/name".to_string(),
            registry: None,
            package: "namespace/name".to_string(),
            version: None,
            command: None,
        }
    );
    assert_eq!(
        SplitVersion::parse("registry.wapm.io/namespace/name").unwrap(),
        SplitVersion {
            original: "registry.wapm.io/namespace/name".to_string(),
            registry: Some("https://registry.wapm.io/graphql".to_string()),
            package: "namespace/name".to_string(),
            version: None,
            command: None,
        }
    );
    assert_eq!(
        format!("{}", SplitVersion::parse("namespace").unwrap_err()),
        "Invalid package version: \"namespace\"".to_string(),
    );
}

impl SplitVersion {
    pub fn parse(s: &str) -> Result<SplitVersion, anyhow::Error> {
        s.parse()
    }
}

impl FromStr for SplitVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let command = WasmerCLIOptions::command();
        let mut prohibited_package_names = command.get_subcommands().map(|s| s.get_name());

        let re1 = regex::Regex::new(r#"(.*)/(.*)@(.*):(.*)"#).unwrap();
        let re2 = regex::Regex::new(r#"(.*)/(.*)@(.*)"#).unwrap();
        let re3 = regex::Regex::new(r#"(.*)/(.*)"#).unwrap();
        let re4 = regex::Regex::new(r#"(.*)/(.*):(.*)"#).unwrap();

        let mut no_version = false;

        let captures = if re1.is_match(s) {
            re1.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re2.is_match(s) {
            re2.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re4.is_match(s) {
            no_version = true;
            re4.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re3.is_match(s) {
            re3.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            return Err(anyhow::anyhow!("Invalid package version: {s:?}"));
        };

        let mut namespace = match captures.get(1).cloned() {
            Some(s) => s,
            None => {
                return Err(anyhow::anyhow!(
                    "Invalid package version: {s:?}: no namespace"
                ))
            }
        };

        let name = match captures.get(2).cloned() {
            Some(s) => s,
            None => return Err(anyhow::anyhow!("Invalid package version: {s:?}: no name")),
        };

        let mut registry = None;
        if namespace.contains('/') {
            let (r, n) = namespace.rsplit_once('/').unwrap();
            let mut real_registry = r.to_string();
            if !real_registry.ends_with("graphql") {
                real_registry = format!("{real_registry}/graphql");
            }
            if !real_registry.contains("://") {
                real_registry = format!("https://{real_registry}");
            }
            registry = Some(real_registry);
            namespace = n.to_string();
        }

        let sv = SplitVersion {
            original: s.to_string(),
            registry,
            package: format!("{namespace}/{name}"),
            version: if no_version {
                None
            } else {
                captures.get(3).cloned()
            },
            command: captures.get(if no_version { 3 } else { 4 }).cloned(),
        };

        let svp = sv.package.clone();
        anyhow::ensure!(
            !prohibited_package_names.any(|s| s == sv.package.trim()),
            "Invalid package name {svp:?}"
        );

        Ok(sv)
    }
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
