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

    match (firstarg, secondarg) {
        (None, _) | (Some("help"), _) | (Some("--help"), _) => return print_help(),

        (Some("-vV"), _)
        | (Some("version"), Some("--verbose"))
        | (Some("--version"), Some("--verbose")) => return print_version(false),

        (Some("-v"), _) | (Some("-V"), _) | (Some("version"), _) | (Some("--version"), _) => {
            return print_version(false)
        }

        (Some("cache"), _) => Cache::try_parse_from(args.iter())?.execute(),
        (Some("compile"), _) => Compile::try_parse_from(args.iter())?.execute(),
        (Some("config"), _) => Config::try_parse_from(args.iter())?.execute(),
        (Some("create-exe"), _) => CreateExe::try_parse_from(args.iter())?.execute(),
        (Some("inspect"), _) => Inspect::try_parse_from(args.iter())?.execute(),
        (Some("self-update"), _) => SelfUpdate::try_parse_from(args.iter())?.execute(),
        (Some("validate"), _) => Validate::try_parse_from(args.iter())?.execute(),
        (Some("wast"), _) => Wast::try_parse_from(args.iter())?.execute(),
        #[cfg(feature = "binfmt")]
        (Some("binfmt"), _) => Binfmt::try_parse_from(args.iter())?.execute(),
        (Some("run"), Some(package)) | (Some(package), _) => {
            // Disable printing backtraces in case `Run::try_parse_from` panics
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let result = std::panic::catch_unwind(|| Run::try_parse_from(args.iter()));
            std::panic::set_hook(hook);

            if let Ok(Ok(run)) = result {
                return run.execute();
            } else if let Ok((package, version)) = split_version(package) {
                if let Ok(o) = wasmer_registry::get_package_local(
                    &package,
                    version.as_ref().map(|s| s.as_str()),
                ) {
                    // Try finding the local package
                    let mut args_without_package = args.clone();
                    args_without_package.remove(1);
                    return RunWithoutFile::try_parse_from(args_without_package.iter())?
                        .into_run_args(o)
                        .execute();
                } else {
                    let sp =
                        spinner::SpinnerBuilder::new(format!("Installing package {package} ..."))
                            .spinner(vec![
                                "⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷", " ", "⠁", "⠂", "⠄", "⡀",
                                "⢀", "⠠", "⠐", "⠈",
                            ])
                            .start();

                    let v = version.as_ref().map(|s| s.as_str());
                    let result = wasmer_registry::install_package(&package, v);
                    sp.close();
                    print!("\r\n");
                    if let Ok(o) = result {
                        // Try auto-installing the remote package
                        let mut args_without_package = args.clone();
                        args_without_package.remove(1);
                        return RunWithoutFile::try_parse_from(args_without_package.iter())?
                            .into_run_args(o)
                            .execute();
                    } else {
                        return print_help();
                    }
                }
            } else {
                return print_help();
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

fn print_help() -> Result<(), anyhow::Error> {
    println!("help");
    Ok(())
}

fn print_version(_: bool) -> Result<(), anyhow::Error> {
    println!("version");
    Ok(())
}
