extern crate structopt;

use std::env;
use std::ffi::OsString;
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
use wasmer_runtime_core::Module;

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
    /// Disable the cache
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
struct CacheGenerate {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,
}

#[derive(Debug, StructOpt)]
struct CacheRun {
    /// Module key in the cache
    #[structopt(parse(from_os_str))]
    module_key: OsString,

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

    #[structopt(name = "generate")]
    CacheGenerate(CacheGenerate),

    #[structopt(name = "run")]
    CacheRun(CacheRun),
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

fn load_wasm_binary(wasm_path: &PathBuf) -> Result<Vec<u8>, String> {
    let wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    if utils::is_wasm_binary(&wasm_binary) {
        return Ok(wasm_binary);
    }

    wabt::wat2wasm(wasm_binary).map_err(|e| format!("Can't convert from wast to wasm: {:?}", e))
}

fn run_wasm_module(module: &Module, name: String, args: &Vec<String>) -> Result<(), String> {
    let (_abi, import_object, _em_globals) = if wasmer_emscripten::is_emscripten_module(module) {
        let mut emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new(module);
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
        &name,
        args.iter().map(|arg| arg.as_str()).collect(),
    )
    .map_err(|e| format!("{:?}", e))
}

/// Execute a wasm/wat file
fn execute_wasm(options: &Run) -> Result<(), String> {
    // force disable caching on windows
    #[cfg(target_os = "windows")]
    let disable_cache = true;
    #[cfg(not(target_os = "windows"))]
    let disable_cache = options.disable_cache;

    let module = if disable_cache {
        let wasm_binary = load_wasm_binary(&options.path)?;
        webassembly::compile(&wasm_binary[..])
            .map_err(|e| format!("Can't compile module: {:?}", e))?
    } else {
        // If we have cache enabled
        let wasmer_cache_dir = get_cache_dir();

        // We create a new cache instance.
        // It could be possible to use any other kinds of caching, as long as they
        // implement the Cache trait (with save and load functions)
        let mut cache = unsafe {
            FileSystemCache::new(wasmer_cache_dir).map_err(|e| format!("Cache error: {:?}", e))?
        };

        let wasm_binary = load_wasm_binary(&options.path)?;

        // We generate a hash for the given binary, so we can use it as key
        // for the Filesystem cache
        let hash = WasmHash::generate(&wasm_binary);

        // cache.load will return the Module if it's able to deserialize it properly, and an error if:
        // * The file is not found
        // * The file exists, but it's corrupted or can't be converted to a module
        match cache.load(hash.encode()) {
            Ok(module) => {
                // We are able to load the module from cache
                module
            }
            Err(_) => {
                let module = webassembly::compile(&wasm_binary[..])
                    .map_err(|e| format!("Can't compile module: {:?}", e))?;

                // We save the module into a cache file
                cache.store(hash.encode(), module.clone()).unwrap();
                module
            }
        }
    };

    let key = String::from(options.path.to_str().unwrap());
    run_wasm_module(&module, key, &options.args)
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

fn generate_wasm_cache(options: &CacheGenerate) -> Result<(), String> {
    let wasm_binary = load_wasm_binary(&options.path)?;
    let module = webassembly::compile(&wasm_binary[..])
        .map_err(|e| format!("Can't compile module: {:?}", e))?;

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

    // We save the module into a cache file
    cache
        .store(hash.encode(), module.clone())
        .map_err(|e| format!("Can't store module in cache: {:?}", e))
}

fn cache_generate(options: CacheGenerate) {
    match generate_wasm_cache(&options) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("{:?}", message);
            exit(1);
        }
    }
}

fn run_wasm_module_from_cache(options: &CacheRun) -> Result<(), String> {
    let wasmer_cache_dir = get_cache_dir();

    // We create a new cache instance.
    // It could be possible to use any other kinds of caching, as long as they
    // implement the Cache trait (with save and load functions)
    let cache = unsafe {
        FileSystemCache::new(wasmer_cache_dir).map_err(|e| format!("Cache error: {:?}", e))?
    };

    let key = options.module_key.to_str().unwrap();
    let module = cache
        .load(String::from(key))
        .map_err(|e| format!("Can't execute module from cache: {:?}", e))?;

    run_wasm_module(&module, String::from(key), &options.args)
}

fn cache_run(options: CacheRun) {
    match run_wasm_module_from_cache(&options) {
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

            Cache::CacheGenerate(options) => cache_generate(options),
            Cache::CacheRun(options) => cache_run(options),
        },
        #[cfg(target_os = "windows")]
        CLIOptions::Cache(_) => {
            println!("Caching is disabled for Windows.");
        }
    }
}
