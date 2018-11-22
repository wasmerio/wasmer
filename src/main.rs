#[macro_use]
extern crate error_chain;
extern crate cranelift_codegen;
extern crate cranelift_entity;
extern crate cranelift_native;
extern crate cranelift_wasm;
extern crate libc;
extern crate memmap;
extern crate region;
extern crate structopt;
extern crate wabt;
extern crate wasmparser;
#[macro_use]
extern crate target_lexicon;
extern crate nix;
extern crate rayon;

use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use structopt::StructOpt;

#[macro_use]
mod macros;
pub mod apis;
pub mod common;
pub mod sighandler;
#[cfg(test)]
mod spectests;
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
fn read_file_contents(path: &PathBuf) -> Result<Vec<u8>, io::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut file = File::open(path)?;
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Execute a WASM/WAT file
fn execute_wasm(wasm_path: PathBuf) -> Result<(), String> {
    let mut wasm_binary: Vec<u8> = read_file_contents(&wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;
    if !webassembly::utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wabt::wat2wasm(wasm_binary)
            .map_err(|err| format!("Can't convert from wast to wasm: {:?}", err))?;
    }
    // TODO: We should instantiate after compilation, so we provide the
    // emscripten environment conditionally based on the module
    let import_object = apis::generate_emscripten_env();
    let webassembly::ResultObject { module, instance } =
        webassembly::instantiate(wasm_binary, import_object)
            .map_err(|err| format!("Can't instantiate the WebAssembly module: {}", err))?;

    if apis::is_emscripten_module(&module) {
        let func_index = match module.info.exports.get("_main") {
            Some(&webassembly::Export::Function(index)) => index,
            _ => panic!("_main emscripten function not found"),
        };
        let main: extern "C" fn(u32, u32, &webassembly::Instance) =
            get_instance_function!(instance, func_index);
        main(0, 0, &instance);
    } else {
        let func_index =
            instance
                .start_func
                .unwrap_or_else(|| match module.info.exports.get("main") {
                    Some(&webassembly::Export::Function(index)) => index,
                    _ => panic!("Main function not found"),
                });
        let main: extern "C" fn(&webassembly::Instance) =
            get_instance_function!(instance, func_index);
        main(&instance);
    }

    Ok(())
}

fn run(options: Run) {
    match execute_wasm(options.path.clone()) {
        Ok(()) => {}
        Err(message) => {
            // let name = options.path.as_os_str().to_string_lossy();
            println!("{}", message);
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
