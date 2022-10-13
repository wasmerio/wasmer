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
use crate::commands::{Cache, Config, Inspect, Run, RunWithoutFile, SelfUpdate, Validate};
use crate::error::PrettyError;
use clap::{CommandFactory, ErrorKind, Parser};
use spinner::SpinnerHandle;
use wasmer_registry::{get_all_local_packages, PackageDownloadInfo};

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
    /// Run a WebAssembly file. Formats accepted: wasm, wat
    #[clap(name = "run")]
    Run(Run),

    /// Wasmer cache
    #[clap(subcommand, name = "cache")]
    Cache(Cache),

    /// Validate a WebAssembly binary
    #[clap(name = "validate")]
    Validate(Validate),

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    #[clap(name = "compile")]
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
    #[clap(name = "config")]
    Config(Config),

    /// Update wasmer to the latest version
    #[clap(name = "self-update")]
    SelfUpdate(SelfUpdate),

    /// Inspect a WebAssembly file
    #[clap(name = "inspect")]
    Inspect(Inspect),

    /// Run spec testsuite
    #[cfg(feature = "wast")]
    #[clap(name = "wast")]
    Wast(Wast),

    /// Unregister and/or register wasmer as binfmt interpreter
    #[cfg(target_os = "linux")]
    #[clap(name = "binfmt")]
    Binfmt(Binfmt),
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
        (Some("list"), _) => {
            return print_packages();
        }
        _ => {}
    }

    let command = args.get(1);
    let options = if cfg!(target_os = "linux") && binpath.ends_with("wasmer-binfmt-interpreter") {
        WasmerCLIOptions::Run(Run::from_binfmt_args())
    } else {
        match command.unwrap_or(&"".to_string()).as_ref() {
            "cache" | "compile" | "config" | "create-exe" | "help" | "inspect" | "run"
            | "self-update" | "validate" | "wast" | "binfmt" => WasmerCLIOptions::parse(),
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
        return try_run_package_or_file(&args, r);
    }

    options.execute()
}

fn try_run_package_or_file(args: &[String], r: &Run) -> Result<(), anyhow::Error> {
    // Check "r.path" is a file or a package / command name
    if r.path.exists() {
        return r.execute();
    }

    let package = format!("{}", r.path.display());

    let mut sv = match split_version(&package) {
        Ok(o) => o,
        Err(_) => return r.execute(),
    };

    let mut package_download_info = None;
    if !sv.package.contains('/') {
        if let Ok(o) = try_lookup_command(&mut sv) {
            package_download_info = Some(o);
        }
    }

    if let Ok(o) = try_execute_local_package(args, &sv) {
        return Ok(o);
    }

    // else: local package not found - try to download and install package
    try_autoinstall_package(args, &sv, package_download_info)
}

fn try_lookup_command(sv: &mut SplitVersion) -> Result<PackageDownloadInfo, anyhow::Error> {
    let sp = start_spinner(format!("Looking up command {} ...", sv.package));

    for registry in wasmer_registry::get_all_available_registries().unwrap_or_default() {
        let result = wasmer_registry::query_command_from_registry(&registry, &sv.package);
        print!("\r\n");
        let command = sv.package.clone();
        if let Ok(o) = result {
            sv.package = o.package.clone();
            sv.version = Some(o.version.clone());
            sv.command = Some(command);
            return Ok(o);
        }
    }

    sp.close();
    Err(anyhow::anyhow!("command {sv:?} not found"))
}

fn try_execute_local_package(args: &[String], sv: &SplitVersion) -> Result<(), anyhow::Error> {
    let package = match wasmer_registry::get_local_package(&sv.package, sv.version.as_deref()) {
        Some(p) => p,
        None => return Err(anyhow::anyhow!("no local package {sv:?} found")),
    };

    let local_package_wasm_path = wasmer_registry::get_package_local_wasm_file(
        &package.registry,
        &package.name,
        &package.version,
    )
    .map_err(|e| anyhow!("{e}"))?;

    // Try finding the local package
    let mut args_without_package = args.to_vec();
    args_without_package.remove(1);

    let mut run_args = RunWithoutFile::try_parse_from(args_without_package.iter())?;
    run_args.command_name = sv.command.clone();
    run_args
        .into_run_args(local_package_wasm_path, Some(package.manifest))
        .execute()
}

fn try_autoinstall_package(
    args: &[String],
    sv: &SplitVersion,
    package: Option<PackageDownloadInfo>,
) -> Result<(), anyhow::Error> {
    let sp = start_spinner(format!("Installing package {} ...", sv.package));
    let v = sv.version.as_deref();
    let result = wasmer_registry::install_package(&sv.package, v, package);
    sp.close();
    print!("\r\n");
    let (package, buf) = match result {
        Ok(o) => o,
        Err(e) => {
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Try auto-installing the remote package
    let mut args_without_package = args.to_vec();
    args_without_package.remove(1);

    let mut run_args = RunWithoutFile::try_parse_from(args_without_package.iter())?;
    run_args.command_name = sv.command.clone();

    run_args
        .into_run_args(buf, Some(package.manifest))
        .execute()
}

fn start_spinner(msg: String) -> SpinnerHandle {
    spinner::SpinnerBuilder::new(msg)
        .spinner(vec![
            "⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷", " ", "⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈",
        ])
        .start()
}

#[derive(Debug, Clone, PartialEq, Default)]
struct SplitVersion {
    package: String,
    version: Option<String>,
    command: Option<String>,
}

#[test]
fn test_split_version() {
    assert_eq!(
        split_version("namespace/name@version:command").unwrap(),
        SplitVersion {
            package: "namespace/name".to_string(),
            version: Some("version".to_string()),
            command: Some("command".to_string()),
        }
    );
    assert_eq!(
        split_version("namespace/name@version").unwrap(),
        SplitVersion {
            package: "namespace/name".to_string(),
            version: Some("version".to_string()),
            command: None,
        }
    );
    assert_eq!(
        split_version("namespace/name").unwrap(),
        SplitVersion {
            package: "namespace/name".to_string(),
            version: None,
            command: None,
        }
    );
    assert_eq!(
        format!("{}", split_version("namespace").unwrap_err()),
        "Invalid package version: \"namespace\"".to_string(),
    );
}

fn split_version(s: &str) -> Result<SplitVersion, anyhow::Error> {
    let command = WasmerCLIOptions::command();
    let prohibited_package_names = command
        .get_subcommands()
        .map(|s| s.get_name())
        .collect::<Vec<_>>();

    let re1 = regex::Regex::new(r#"(.*)/(.*)@(.*):(.*)"#).unwrap();
    let re2 = regex::Regex::new(r#"(.*)/(.*)@(.*)"#).unwrap();
    let re3 = regex::Regex::new(r#"(.*)/(.*)"#).unwrap();

    let captures = if re1.is_match(s) {
        re1.captures(s)
            .map(|c| {
                c.iter()
                    .filter_map(|f| f)
                    .map(|m| m.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else if re2.is_match(s) {
        re2.captures(s)
            .map(|c| {
                c.iter()
                    .filter_map(|f| f)
                    .map(|m| m.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else if re3.is_match(s) {
        re3.captures(s)
            .map(|c| {
                c.iter()
                    .filter_map(|f| f)
                    .map(|m| m.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        return Err(anyhow::anyhow!("Invalid package version: {s:?}"));
    };

    let namespace = match captures.get(1).cloned() {
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

    let sv = SplitVersion {
        package: format!("{namespace}/{name}"),
        version: captures.get(3).cloned(),
        command: captures.get(4).cloned(),
    };

    if prohibited_package_names.contains(&sv.package.trim()) {
        return Err(anyhow::anyhow!("Invalid package name {:?}", sv.package));
    }

    Ok(sv)
}

fn print_packages() -> Result<(), anyhow::Error> {
    use prettytable::{format, row, Table};

    let rows = get_all_local_packages()
        .into_iter()
        .map(|pkg| {
            let commands = pkg
                .manifest
                .command
                .unwrap_or_default()
                .iter()
                .map(|c| c.get_name())
                .collect::<Vec<_>>()
                .join(" \r\n");

            row![pkg.registry, pkg.name, pkg.version, commands]
        })
        .collect::<Vec<_>>();

    let empty_table = rows.is_empty();
    let mut table = Table::init(rows);
    table.set_titles(row!["Registry", "Package", "Version", "Commands"]);
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.set_format(*format::consts::FORMAT_NO_COLSEP);
    if empty_table {
        table.add_empty_row();
    }
    let _ = table.printstd();

    Ok(())
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
