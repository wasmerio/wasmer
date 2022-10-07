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
use anyhow::Result;
use clap::Parser;
use wasmer_registry::get_all_local_packages;

#[derive(Parser)]
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

/// The main function for the Wasmer CLI tool.
pub fn wasmer_main() {
    // We allow windows to print properly colors
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();

    let cmd_output = parse_cli_args();

    PrettyError::report(cmd_output);
}

fn parse_cli_args() -> Result<(), anyhow::Error> {
    let args = std::env::args().collect::<Vec<_>>();
    let binpath = args.get(0).map(|s| s.as_ref()).unwrap_or("");

    // In case we've been run as wasmer-binfmt-interpreter myfile.wasm args,
    // we assume that we're registered via binfmt_misc
    if cfg!(target_os = "linux") && binpath.ends_with("wasmer-binfmt-interpreter") {
        return Run::from_binfmt_args().execute();
    }

    let firstarg = args.get(1).map(|s| s.as_str());
    let secondarg = args.get(2).map(|s| s.as_str());

    let mut args_without_first_arg = args.clone();
    args_without_first_arg.remove(0);

    match (firstarg, secondarg) {
        (None, _) | (Some("help"), _) | (Some("--help"), _) => return print_help(true),
        (Some("-h"), _) => return print_help(false),

        (Some("-vV"), _)
        | (Some("version"), Some("--verbose"))
        | (Some("--version"), Some("--verbose")) => return print_version(true),

        (Some("-v"), _) | (Some("-V"), _) | (Some("version"), _) | (Some("--version"), _) => {
            return print_version(false)
        }

        (Some("cache"), _) => Cache::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("compile"), _) => Compile::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("config"), _) => Config::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("create-exe"), _) => {
            CreateExe::try_parse_from(args_without_first_arg.iter())?.execute()
        }
        (Some("inspect"), _) => Inspect::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("self-update"), _) => {
            SelfUpdate::try_parse_from(args_without_first_arg.iter())?.execute()
        }
        (Some("validate"), _) => Validate::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("wast"), _) => Wast::try_parse_from(args_without_first_arg.iter())?.execute(),
        #[cfg(feature = "binfmt")]
        (Some("binfmt"), _) => Binfmt::try_parse_from(args_without_first_arg.iter())?.execute(),
        (Some("list"), _) => {
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

                    row![
                        pkg.registry.clone(),
                        pkg.name.clone(),
                        pkg.version.clone(),
                        commands
                    ]
                })
                .collect::<Vec<_>>();

            let empty_table = rows.is_empty();
            if empty_table {
                println!("--------------------------------------");
                println!("Registry  Package  Version  Commands ");
                println!("======================================");
                println!("");
            } else {
                let mut table = Table::init(rows);
                table.set_titles(row!["Registry", "Package", "Version", "Commands"]);
                table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
                table.set_format(*format::consts::FORMAT_NO_COLSEP);
                let _ = table.printstd();
            }

            Ok(())
        }
        (Some("run"), Some(package)) | (Some(package), _) => {
            if package.starts_with("-") {
                return Err(anyhow!("Unknown CLI argument {package:?}"));
            }

            // Disable printing backtraces in case `Run::try_parse_from` panics
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let result = std::panic::catch_unwind(|| Run::try_parse_from(args.iter()));
            std::panic::set_hook(hook);

            if let Ok(Ok(run)) = result {
                return run.execute();
            } else if let Ok((package, version)) = split_version(package) {
                if let Some(package) = wasmer_registry::get_local_package(
                    &package,
                    version.as_ref().map(|s| s.as_str()),
                ) {
                    let local_package_wasm_path = wasmer_registry::get_package_local_wasm_file(
                        &package.registry,
                        &package.name,
                        &package.version,
                    )
                    .map_err(|e| anyhow!("{e}"))?;

                    // Try finding the local package
                    let mut args_without_package = args.clone();
                    args_without_package.remove(1);
                    return RunWithoutFile::try_parse_from(args_without_package.iter())?
                        .into_run_args(local_package_wasm_path, Some(package.manifest.clone()))
                        .execute();
                }

                // else: local package not found
                let sp = spinner::SpinnerBuilder::new(format!("Installing package {package} ..."))
                    .spinner(vec![
                        "⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷", " ", "⠁", "⠂", "⠄", "⡀", "⢀", "⠠",
                        "⠐", "⠈",
                    ])
                    .start();

                let v = version.as_ref().map(|s| s.as_str());
                let result = wasmer_registry::install_package(&package, v);
                sp.close();
                print!("\r\n");
                match result {
                    Ok((package, buf)) => {
                        // Try auto-installing the remote package
                        let mut args_without_package = args.clone();
                        args_without_package.remove(1);
                        return RunWithoutFile::try_parse_from(args_without_package.iter())?
                            .into_run_args(buf, Some(package.manifest.clone()))
                            .execute();
                    }
                    Err(e) => {
                        println!("{e}");
                        return Ok(());
                    }
                }
            } else {
                return print_help(true);
            }
        }
    }
}

fn split_version(s: &str) -> Result<(String, Option<String>), anyhow::Error> {
    let package_version = s.split("@").collect::<Vec<_>>();
    match package_version.as_slice() {
        &[p, v] => Ok((p.trim().to_string(), Some(v.trim().to_string()))),
        &[p] => Ok((p.trim().to_string(), None)),
        _ => Err(anyhow!("Invalid package / version: {s:?}")),
    }
}

fn print_help(verbose: bool) -> Result<(), anyhow::Error> {
    use clap::CommandFactory;
    let mut cmd = WasmerCLIOptions::command();
    if verbose {
        let _ = cmd.print_long_help();
    } else {
        let _ = cmd.print_help();
    }
    Ok(())
}

fn print_version(verbose: bool) -> Result<(), anyhow::Error> {
    if !verbose {
        println!("{}", env!("CARGO_PKG_VERSION"));
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
            #[allow(unused_mut)]
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
