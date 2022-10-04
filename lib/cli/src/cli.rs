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
use crate::commands::{Cache, Config, Inspect, Run, SelfUpdate, Validate};
use crate::error::PrettyError;
use anyhow::Result;

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

    match (args.get(1).map(|s| s.as_str()), args.get(2).map(|s| s.as_str())) {
        (None, _) |
        (Some("help"), _) | 
        (Some("--help"), _) => return print_help(),
        
        (Some("-vV"), _) | 
        (Some("version"), Some("--verbose")) |
        (Some("--version"), Some("--verbose")) => return print_version(false),

        (Some("-v"), _) | 
        (Some("version"), _) |
        (Some("--version"), _) => return print_version(false),
        
        (Some("cache"), Some(_)) |
        (Some("compile"), Some(_)) | 
        (Some("config"), Some(_)) |
        (Some("create-exe"), Some(_)) | 
        (Some("inspect"), Some(_)) |
        (Some("self-update"), _) |
        (Some("validate"), Some(_)) |
        (Some("wast"), Some(_)) |
        (Some("binfmt"), Some(_)) => {
            println!("running {:?}", args.get(1));
            return Ok(())
        },
        (Some("run"), Some(_)) |
        (Some(_), _) => {
            use clap::Parser;
            // wasmer run file
            // wasmer run [package]
            if let Ok(run) = Run::try_parse() {
                return run.execute();
            } else {
                return print_help();
            }
        },
    }
}

fn print_help() -> Result<(), anyhow::Error>{
    println!("help");
    Ok(())
}

fn print_version(_: bool) -> Result<(), anyhow::Error> {
    println!("version");
    Ok(())
}