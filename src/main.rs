#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate structopt;
extern crate wabt;
extern crate cranelift_codegen;
extern crate cranelift_native;
extern crate cranelift_wasm;
extern crate cranelift_entity;
#[macro_use]
extern crate target_lexicon;
extern crate spin;

use std::path::PathBuf;
use std::fs::File;
use std::io;
use std::io::Read;
use std::process::exit;
use std::error::Error;

use structopt::StructOpt;
use wabt::wat2wasm;

pub mod webassembly;
pub mod spec;
pub mod common;


/// The options for the wasmer Command Line Interface
#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WASM execution runtime.")]
struct Opt {
    /// Activate debug mode
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
fn execute_wasm(wasm_path: PathBuf) -> Result<(), String>{
    let mut wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| String::from(err.description()))?;
    if !webassembly::utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wat2wasm(
                wasm_binary
            ).map_err(|err| String::from(err.description()))?;
    }

    webassembly::instantiate(wasm_binary, None).map_err(|err| String::from(err.description()))?;
    Ok(())
}


fn main() {
    let opt = Opt::from_args();
    match execute_wasm(opt.path.clone()) {
        Ok(()) => {}
        Err(message) => {
            let name = opt.path.as_os_str().to_string_lossy();
            println!("error while executing {}: {}", name, message);
            exit(1);
        }
    }
}
