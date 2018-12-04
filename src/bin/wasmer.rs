extern crate structopt;
extern crate wasmer;

use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use structopt::StructOpt;

use wasmer::*;

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

    if apis::emscripten::is_emscripten_module(&module) {
        // Emscripten __ATINIT__
        if let Some(&webassembly::Export::Function(environ_constructor_index)) = module.info.exports.get("___emscripten_environ_constructor") {
            debug!("emscripten::___emscripten_environ_constructor");
            let ___emscripten_environ_constructor: extern "C" fn(&webassembly::Instance) =
                get_instance_function!(instance, environ_constructor_index);
            call_protected!(___emscripten_environ_constructor(&instance)).map_err(|err| format!("{}", err))?;
        };
        // TODO: We also need to handle TTY.init() and SOCKFS.root = FS.mount(SOCKFS, {}, null)
        let func_index = match module.info.exports.get("_main") {
            Some(&webassembly::Export::Function(index)) => index,
            _ => panic!("_main emscripten function not found"),
        };
        let main: extern "C" fn(u32, u32, &webassembly::Instance) =
            get_instance_function!(instance, func_index);
        return call_protected!(main(0, 0, &instance)).map_err(|err| format!("{}", err));
        // TODO: We should implement emscripten __ATEXIT__
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
        return call_protected!(main(&instance)).map_err(|err| format!("{}", err));
    }
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
        CLIOptions::SelfUpdate => update::self_update(),
    }
}
