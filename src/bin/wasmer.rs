extern crate structopt;

use std::env;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use structopt::StructOpt;

use wasmer::webassembly::InstanceABI;
use wasmer::*;
use wasmer_emscripten;
use wasmer_runtime::cache::{Cache as BaseCache, FileSystemCache, WasmHash};

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "Wasm execution runtime.")]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wast
    #[structopt(name = "run")]
    Run(Run),

    /// Wasmer cache
    #[structopt(name = "cache")]
    Cache(Cache),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate,
}

#[derive(Debug, StructOpt)]
struct Run {
    #[structopt(short = "d", long = "debug")]
    debug: bool,

    // Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    /// Application arguments
    #[structopt(name = "--", raw(multiple = "true"))]
    args: Vec<String>,
}

#[derive(Debug, StructOpt)]
enum Cache {
    #[structopt(name = "clean")]
    Clean,

    #[structopt(name = "dir")]
    Dir,
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

fn get_cache_dir() -> PathBuf {
    match env::var("WASMER_CACHE_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => {
            // We use a temporal directory for saving cache files
            let mut temp_dir = env::temp_dir();
            temp_dir.push("wasmer");
            temp_dir
        }
    }
}

/// Execute a wasm/wat file
fn execute_wasm(options: &Run) -> Result<(), String> {
    // force disable caching on windows
    #[cfg(target_os = "windows")]
    let disable_cache = true;
    #[cfg(not(target_os = "windows"))]
    let disable_cache = options.disable_cache;

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

    let module = if !disable_cache {
        // If we have cache enabled

        // We generate a hash for the given binary, so we can use it as key
        // for the Filesystem cache
        let hash = WasmHash::generate(&wasm_binary);

        let wasmer_cache_dir = get_cache_dir();

        // We create a new cache instance.
        // It could be possible to use any other kinds of caching, as long as they
        // implement the Cache trait (with save and load functions)
        let mut cache = unsafe {
            FileSystemCache::new(wasmer_cache_dir).map_err(|e| format!("Cache error: {:?}", e))?
        };

        // cache.load will return the Module if it's able to deserialize it properly, and an error if:
        // * The file is not found
        // * The file exists, but it's corrupted or can't be converted to a module
        let module = match cache.load(hash) {
            Ok(module) => {
                // We are able to load the module from cache
                module
            }
            Err(_) => {
                let module = webassembly::compile(&wasm_binary[..])
                    .map_err(|e| format!("Can't compile module: {:?}", e))?;

                // We save the module into a cache file
                cache.store(hash, module.clone()).unwrap();
                module
            }
        };
        module
    } else {
        webassembly::compile(&wasm_binary[..])
            .map_err(|e| format!("Can't compile module: {:?}", e))?
    };

    let (_abi, import_object, _em_globals) = if wasmer_emscripten::is_emscripten_module(&module) {
        let mut emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new(&module);
        (
            InstanceABI::Emscripten,
            wasmer_emscripten::generate_emscripten_env(&mut emscripten_globals),
            Some(emscripten_globals), // TODO Em Globals is here to extend, lifetime, find better solution
        )
    } else {
        (
            InstanceABI::None,
            wasmer_runtime_core::import::ImportObject::new(),
            None,
        )
    };

    let mut instance = module
        .instantiate(&import_object)
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
        #[cfg(not(target_os = "windows"))]
        CLIOptions::SelfUpdate => update::self_update(),
        #[cfg(target_os = "windows")]
        CLIOptions::SelfUpdate => {
            println!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
        }
        #[cfg(not(target_os = "windows"))]
        CLIOptions::Cache(cache) => match cache {
            Cache::Clean => {
                use std::fs;
                let cache_dir = get_cache_dir();
                fs::remove_dir_all(cache_dir.clone()).expect("Can't remove cache dir");
                fs::create_dir(cache_dir.clone()).expect("Can't create cache dir");
            }
            Cache::Dir => {
                println!("{}", get_cache_dir().to_string_lossy());
            }
        },
        #[cfg(target_os = "windows")]
        CLIOptions::Cache(_) => {
            println!("Caching is disabled for Windows.");
        }
    }
}
