extern crate structopt;

use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use structopt::StructOpt;

use wasmer::webassembly::InstanceABI;
use wasmer::*;
use wasmer_emscripten;

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "Wasm execution runtime.")]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wast
    #[structopt(name = "run")]
    Run(Run),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate,
}

#[derive(Debug, StructOpt)]
struct Run {
    #[structopt(short = "d", long = "debug")]
    debug: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    /// Application arguments
    #[structopt(name = "--", raw(multiple = "true"))]
    args: Vec<String>,
}

/// Read the contents of a file
fn read_file_contents(path: &PathBuf) -> Result<Vec<u8>, io::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut file = File::open(path)?;
    file.read_to_end(&mut buffer)?;
    // We force to close the file
    drop(file);
    Ok(buffer)
}

/// Execute a wasm/wat file
fn execute_wasm(options: &Run) -> Result<(), String> {
    let wasm_path = &options.path;

    let mut wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    if !utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wabt::wat2wasm(wasm_binary)
            .map_err(|e| format!("Can't convert from wast to wasm: {:?}", e))?;
    }

    let module = webassembly::compile(&wasm_binary[..])
        .map_err(|e| format!("Can't compile module: {:?}", e))?;

    let (_abi, import_object) = if wasmer_emscripten::is_emscripten_module(&module) {
        let (table_min, table_max) = wasmer_emscripten::get_emscripten_table_size(&module);
        let (memory_min, memory_max) = wasmer_emscripten::get_emscripten_memory_size(&module);
        let mut emscripten_globals =
            wasmer_emscripten::EmscriptenGlobals::new(table_min, table_max, memory_min, memory_max);
        (
            InstanceABI::Emscripten,
            wasmer_emscripten::generate_emscripten_env(&mut emscripten_globals),
        )
    } else {
        (
            InstanceABI::None,
            wasmer_runtime_core::import::ImportObject::new(),
        )
    };

    let mut instance = module
        .instantiate(import_object)
        .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

    webassembly::run_instance(
        &module,
        &mut instance,
        options.path.to_str().unwrap(),
        options.args.iter().map(|arg| arg.as_str()).collect(),
    )
    .map_err(|e| format!("{:?}", e))?;
    Ok(())
}

fn run(options: Run) {
    match execute_wasm(&options) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("{:?}", message);
            exit(1);
        }
    }
}

fn main() {
    let options = CLIOptions::from_args();
    match options {
        CLIOptions::Run(options) => run(options),
        CLIOptions::SelfUpdate => update::self_update(),
    }
}
