extern crate structopt;
extern crate wasmer;

use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use apis::emscripten::{allocate_on_stack, allocate_cstr_on_stack};
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

    /// Application arguments
    #[structopt(name = "--", raw(multiple="true"))]
    args: Vec<String>,
}


/// Read the contents of a file
fn read_file_contents(path: &PathBuf) -> Result<Vec<u8>, io::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut file = File::open(path)?;
    file.read_to_end(&mut buffer)?;
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

    // TODO: We should instantiate after compilation, so we provide the
    // emscripten environment conditionally based on the module
    let import_object = apis::generate_emscripten_env();
    let webassembly::ResultObject { module, mut instance } =
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

        let (argc, argv) = store_module_arguments(options, &mut instance);

        return call_protected!(main(argc, argv, &instance)).map_err(|err| format!("{}", err));
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

fn store_module_arguments(options: &Run, instance: &mut webassembly::Instance) -> (u32, u32) {
    let argc = options.args.len() + 1;

    let (argv_offset, argv_slice): (_, &mut [u32]) = unsafe { allocate_on_stack(((argc + 1) * 4) as u32, instance) };
    assert!(argv_slice.len() >= 1);

    argv_slice[0] = unsafe { allocate_cstr_on_stack(options.path.to_str().unwrap(), instance).0 };

    for (slot, arg) in argv_slice[1..argc].iter_mut().zip(options.args.iter()) {
        *slot = unsafe { allocate_cstr_on_stack(&arg, instance).0 };
    }

    argv_slice[argc] = 0;

    (argc as u32, argv_offset)
}

// fn get_module_arguments(options: &Run, instance: &mut webassembly::Instance) -> (u32, u32) {
//     // Application Arguments
//     let mut arg_values: Vec<String> = Vec::new();
//     let mut arg_addrs: Vec<*const u8> = Vec::new();
//     let arg_length = options.args.len() + 1;

//     arg_values.reserve_exact(arg_length);
//     arg_addrs.reserve_exact(arg_length);

//     // Push name of wasm file
//     arg_values.push(format!("{}\0", options.path.to_str().unwrap()));
//     arg_addrs.push(arg_values[0].as_ptr());

//     // Push additional arguments
//     for (i, arg) in options.args.iter().enumerate() {
//         arg_values.push(format!("{}\0", arg));
//         arg_addrs.push(arg_values[i + 1].as_ptr());
//     }

//     // Get argument count and pointer to addresses
//     let argv = arg_addrs.as_ptr() as *mut *mut i8;
//     let argc = arg_length as u32;

//     // Copy the the arguments into the wasm memory and get offset
//     let argv_offset =  unsafe {
//         copy_cstr_array_into_wasm(argc, argv, instance)
//     };

//     debug!("argc = {:?}", argc);
//     debug!("argv = {:?}", arg_addrs);

//     (argc, argv_offset)
// }
