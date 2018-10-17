#![feature(test, libc)]

extern crate test;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate structopt;
extern crate cranelift_codegen;
extern crate cranelift_entity;
extern crate cranelift_native;
extern crate cranelift_wasm;
extern crate wabt;
#[macro_use]
extern crate target_lexicon;
extern crate spin;

use std::time::{Duration, Instant};

// #[macro_use] extern crate log;

use libc;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use structopt::StructOpt;
use wabt::wat2wasm;

#[macro_use]
mod macros;
pub mod common;
pub mod integrations;
pub mod spec;
pub mod webassembly;

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WASM execution runtime.")]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wast
    #[structopt(name = "run")]
    Run(Run),
}

#[derive(Debug, StructOpt)]
struct Run {
    #[structopt(short = "d", long = "debug")]
    debug: bool,
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,
}

/// Read the contents of a file
fn read_file_contents(path: PathBuf) -> Result<Vec<u8>, io::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut file = File::open(path)?;
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Execute a WASM/WAT file
fn execute_wasm(wasm_path: PathBuf) -> Result<(), String> {
    let mut wasm_binary: Vec<u8> =
        read_file_contents(wasm_path).map_err(|err| String::from(err.description()))?;
    if !webassembly::utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wat2wasm(wasm_binary).map_err(|err| String::from(err.description()))?;
    }

    let import_object = integrations::generate_libc_env();
    let webassembly::ResultObject {module, instance} = webassembly::instantiate(wasm_binary, Some(import_object)).map_err(|err| String::from(err.description()))?;
    let func_index = instance.start_func.unwrap_or_else(|| {
        match module.info.exports.get("main") {
            Some(&webassembly::Export::Function(index)) => index,
            _ => panic!("Main function not found"),
        }
    });
    let main: fn() = get_instance_function!(instance, func_index);
    main();
    Ok(())
}

fn run(options: Run) {
    match execute_wasm(options.path.clone()) {
        Ok(()) => {}
        Err(message) => {
            let name = options.path.as_os_str().to_string_lossy();
            println!("error while executing {}: {}", name, message);
            exit(1);
        }
    }
}

fn main() {
    let options = CLIOptions::from_args();
    match options {
        CLIOptions::Run(options) => run(options),
    }
}
