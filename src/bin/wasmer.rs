extern crate structopt;
extern crate wasmer;

use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::rc::Rc;

use structopt::StructOpt;

use wasmer::*;
use wasmer_runtime;
use wasmer_emscripten;

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "WASM execution runtime.")]
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

/// Execute a WASM/WAT file
fn execute_wasm(options: &Run) -> Result<(), String> {
    let wasm_path = &options.path;

    let mut wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| {
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

    let isa = webassembly::get_isa();

    debug!("webassembly - creating module");
    let module = webassembly::compile(&wasm_binary[..])
        .map_err(|err| format!("Can't create the WebAssembly module: {}", err))?;

    let abi = if wasmer_emscripten::is_emscripten_module(&module) {
        webassembly::InstanceABI::Emscripten
    } else {
        webassembly::InstanceABI::None
    };

    let emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new();

    let import_object = if abi == webassembly::InstanceABI::Emscripten {
        wasmer_emscripten::generate_emscripten_env(&emscripten_globals)
    } else {
        wasmer_runtime::import::Imports::new()
    };

    let import_object = Rc::new(import_object);

    let instance_options = webassembly::InstanceOptions {
        mock_missing_imports: true,
        mock_missing_globals: true,
        mock_missing_tables: true,
        abi: abi,
        show_progressbar: true,
//        isa: isa,
    };

    debug!("webassembly - creating instance");

    let mut instance = module.instantiate(import_object)
        .map_err(|err| format!("Can't instantiate the WebAssembly module: {}", err))?;

    webassembly::start_instance(
        Arc::clone(&module),
        &mut instance,
        options.path.to_str().unwrap(),
        options.args.iter().map(|arg| arg.as_str()).collect(),
    )
}

fn run(options: Run) {
    match execute_wasm(&options) {
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
        CLIOptions::SelfUpdate => update::self_update(),
    }
}
