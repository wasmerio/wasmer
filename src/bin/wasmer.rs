#![deny(unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

extern crate structopt;

use std::env;
use std::fs::{read_to_string, File};
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;

use hashbrown::HashMap;
use structopt::StructOpt;

use wasmer::*;
use wasmer_clif_backend::CraneliftCompiler;
#[cfg(feature = "backend:llvm")]
use wasmer_llvm_backend::LLVMCompiler;
use wasmer_runtime::{
    cache::{Cache as BaseCache, FileSystemCache, WasmHash, WASMER_VERSION_HASH},
    error::RuntimeError,
    Func, Value,
};
use wasmer_runtime_core::{
    self,
    backend::{Compiler, CompilerConfig, MemoryBoundCheckMode},
    loader::{Instance as LoadedInstance, LocalLoader},
};
#[cfg(feature = "backend:singlepass")]
use wasmer_singlepass_backend::SinglePassCompiler;
#[cfg(feature = "wasi")]
use wasmer_wasi;

// stub module to make conditional compilation happy
#[cfg(not(feature = "wasi"))]
mod wasmer_wasi {
    use wasmer_runtime_core::{import::ImportObject, module::Module};

    pub fn is_wasi_module(_module: &Module) -> bool {
        false
    }

    pub fn generate_import_object(_args: Vec<Vec<u8>>, _envs: Vec<Vec<u8>>) -> ImportObject {
        unimplemented!()
    }
}

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

    /// Validate a Web Assembly binary
    #[structopt(name = "validate")]
    Validate(Validate),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate,
}

#[derive(Debug, StructOpt)]
struct Run {
    // Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    // Disable the cache
    #[structopt(
        long = "backend",
        default_value = "cranelift",
        raw(possible_values = "Backend::variants()", case_insensitive = "true")
    )]
    backend: Backend,

    /// Emscripten symbol map
    #[structopt(long = "em-symbol-map", parse(from_os_str), group = "emscripten")]
    em_symbol_map: Option<PathBuf>,

    /// Begin execution at the specified symbol
    #[structopt(long = "em-entrypoint", group = "emscripten")]
    em_entrypoint: Option<String>,

    /// WASI pre-opened directory
    #[structopt(long = "dir", multiple = true, group = "wasi")]
    pre_opened_directories: Vec<String>,

    /// Map a host directory to a different location for the wasm module
    #[structopt(long = "mapdir", multiple = true)]
    mapped_dirs: Vec<String>,

    /// Custom code loader
    #[structopt(
        long = "loader",
        raw(possible_values = "LoaderName::variants()", case_insensitive = "true")
    )]
    loader: Option<LoaderName>,

    #[structopt(long = "command-name", hidden = true)]
    command_name: Option<String>,

    /// Application arguments
    #[structopt(name = "--", raw(multiple = "true"))]
    args: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
enum LoaderName {
    Local,
    #[cfg(feature = "loader:kernel")]
    Kernel,
}

impl LoaderName {
    pub fn variants() -> &'static [&'static str] {
        &[
            "local",
            #[cfg(feature = "loader:kernel")]
            "kernel",
        ]
    }
}

impl FromStr for LoaderName {
    type Err = String;
    fn from_str(s: &str) -> Result<LoaderName, String> {
        match s.to_lowercase().as_str() {
            "local" => Ok(LoaderName::Local),
            #[cfg(feature = "loader:kernel")]
            "kernel" => Ok(LoaderName::Kernel),
            _ => Err(format!("The loader {} doesn't exist", s)),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
enum Backend {
    Cranelift,
    Singlepass,
    LLVM,
}

impl Backend {
    pub fn variants() -> &'static [&'static str] {
        &[
            "cranelift",
            #[cfg(feature = "backend:singlepass")]
            "singlepass",
            #[cfg(feature = "backend:llvm")]
            "llvm",
        ]
    }
}

impl FromStr for Backend {
    type Err = String;
    fn from_str(s: &str) -> Result<Backend, String> {
        match s.to_lowercase().as_str() {
            "singlepass" => Ok(Backend::Singlepass),
            "cranelift" => Ok(Backend::Cranelift),
            "llvm" => Ok(Backend::LLVM),
            // "llvm" => Err(
            //     "The LLVM backend option is not enabled by default due to binary size constraints"
            //         .to_string(),
            // ),
            _ => Err(format!("The backend {} doesn't exist", s)),
        }
    }
}

#[derive(Debug, StructOpt)]
enum Cache {
    /// Clear the cache
    #[structopt(name = "clean")]
    Clean,

    /// Display the location of the cache
    #[structopt(name = "dir")]
    Dir,
}

#[derive(Debug, StructOpt)]
struct Validate {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,
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
            temp_dir.push(WASMER_VERSION_HASH);
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

    let mapped_dirs = {
        let mut md = vec![];
        for entry in options.mapped_dirs.iter() {
            if let &[alias, real_dir] = &entry.split(':').collect::<Vec<&str>>()[..] {
                let pb = PathBuf::from(&real_dir);
                if let Ok(pb_metadata) = pb.metadata() {
                    if !pb_metadata.is_dir() {
                        return Err(format!(
                            "\"{}\" exists, but it is not a directory",
                            &real_dir
                        ));
                    }
                } else {
                    return Err(format!("Directory \"{}\" does not exist", &real_dir));
                }
                md.push((alias.to_string(), pb));
                continue;
            }
            return Err(format!(
                "Directory mappings must consist of two paths separate by a colon. Found {}",
                &entry
            ));
        }
        md
    };
    let wasm_path = &options.path;

    let mut wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    let em_symbol_map = if let Some(em_symbol_map_path) = options.em_symbol_map.clone() {
        let em_symbol_map_content: String = read_to_string(&em_symbol_map_path)
            .map_err(|err| {
                format!(
                    "Can't read symbol map file {}: {}",
                    em_symbol_map_path.as_os_str().to_string_lossy(),
                    err,
                )
            })?
            .to_owned();
        let mut em_symbol_map = HashMap::new();
        for line in em_symbol_map_content.lines() {
            let mut split = line.split(':');
            let num_str = if let Some(ns) = split.next() {
                ns
            } else {
                return Err(format!(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                ));
            };
            let num: u32 = num_str.parse::<u32>().map_err(|err| {
                format!(
                    "Failed to parse {} as a number in symbol map: {}",
                    num_str, err
                )
            })?;
            let name_str: String = if let Some(name_str) = split.next() {
                name_str
            } else {
                return Err(format!(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                ));
            }
            .to_owned();

            em_symbol_map.insert(num, name_str);
        }
        Some(em_symbol_map)
    } else {
        None
    };

    if !utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wabt::wat2wasm(wasm_binary)
            .map_err(|e| format!("Can't convert from wast to wasm: {:?}", e))?;
    }

    let compiler: Box<dyn Compiler> = match options.backend {
        #[cfg(feature = "backend:singlepass")]
        Backend::Singlepass => Box::new(SinglePassCompiler::new()),
        #[cfg(not(feature = "backend:singlepass"))]
        Backend::Singlepass => return Err("The singlepass backend is not enabled".to_string()),
        Backend::Cranelift => Box::new(CraneliftCompiler::new()),
        #[cfg(feature = "backend:llvm")]
        Backend::LLVM => Box::new(LLVMCompiler::new()),
        #[cfg(not(feature = "backend:llvm"))]
        Backend::LLVM => return Err("the llvm backend is not enabled".to_string()),
    };

    #[cfg(feature = "loader:kernel")]
    let is_kernel_loader = if let Some(LoaderName::Kernel) = options.loader {
        true
    } else {
        false
    };

    #[cfg(not(feature = "loader:kernel"))]
    let is_kernel_loader = false;

    let module = if is_kernel_loader {
        webassembly::compile_with_config_with(
            &wasm_binary[..],
            CompilerConfig {
                symbol_map: em_symbol_map,
                memory_bound_check_mode: MemoryBoundCheckMode::Disable,
                enforce_stack_check: true,
            },
            &*compiler,
        )
        .map_err(|e| format!("Can't compile module: {:?}", e))?
    } else if disable_cache {
        webassembly::compile_with_config_with(
            &wasm_binary[..],
            CompilerConfig {
                symbol_map: em_symbol_map,
                ..Default::default()
            },
            &*compiler,
        )
        .map_err(|e| format!("Can't compile module: {:?}", e))?
    } else {
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
                let module = webassembly::compile_with_config_with(
                    &wasm_binary[..],
                    CompilerConfig {
                        symbol_map: em_symbol_map,
                        ..Default::default()
                    },
                    &*compiler,
                )
                .map_err(|e| format!("Can't compile module: {:?}", e))?;
                // We try to save the module into a cache file
                cache.store(hash, module.clone()).unwrap_or_default();

                module
            }
        };
        module
    };

    if let Some(loader) = options.loader {
        let mut import_object = wasmer_runtime_core::import::ImportObject::new();
        import_object.allow_missing_functions = true; // Import initialization might be left to the loader.
        let instance = module
            .instantiate(&import_object)
            .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

        let args: Vec<Value> = options
            .args
            .iter()
            .map(|arg| arg.as_str())
            .map(|x| {
                Value::I32(x.parse().expect(&format!(
                    "Can't parse the provided argument {:?} as a integer",
                    x
                )))
            })
            .collect();
        let index = instance
            .resolve_func("_start")
            .expect("The loader requires a _start function to be present in the module");
        let mut ins: Box<LoadedInstance<Error = String>> = match loader {
            LoaderName::Local => Box::new(
                instance
                    .load(LocalLoader)
                    .expect("Can't use the local loader"),
            ),
            #[cfg(feature = "loader:kernel")]
            LoaderName::Kernel => Box::new(
                instance
                    .load(::wasmer_kernel_loader::KernelLoader)
                    .expect("Can't use the kernel loader"),
            ),
        };
        println!("{:?}", ins.call(index, &args));
        return Ok(());
    }

    // TODO: refactor this
    if wasmer_emscripten::is_emscripten_module(&module) {
        let mut emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new(&module);
        let import_object = wasmer_emscripten::generate_emscripten_env(&mut emscripten_globals);
        let mut instance = module
            .instantiate(&import_object)
            .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

        if !mapped_dirs.is_empty() {
            eprintln!("WARN: mapdir is not implemented for emscripten targets");
        }
        wasmer_emscripten::run_emscripten_instance(
            &module,
            &mut instance,
            if let Some(cn) = &options.command_name {
                cn
            } else {
                options.path.to_str().unwrap()
            },
            options.args.iter().map(|arg| arg.as_str()).collect(),
            options.em_entrypoint.clone(),
        )
        .map_err(|e| format!("{:?}", e))?;
    } else {
        if cfg!(feature = "wasi") && wasmer_wasi::is_wasi_module(&module) {
            let import_object = wasmer_wasi::generate_import_object(
                if let Some(cn) = &options.command_name {
                    [cn.clone()]
                } else {
                    [options.path.to_str().unwrap().to_owned()]
                }
                .iter()
                .chain(options.args.iter())
                .cloned()
                .map(|arg| arg.into_bytes())
                .collect(),
                env::vars()
                    .map(|(k, v)| format!("{}={}", k, v).into_bytes())
                    .collect(),
                options.pre_opened_directories.clone(),
                mapped_dirs,
            );

            let instance = module
                .instantiate(&import_object)
                .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

            let start: Func<(), ()> = instance.func("_start").map_err(|e| format!("{:?}", e))?;

            let result = start.call();

            if let Err(ref err) = result {
                match err {
                    RuntimeError::Trap { msg } => panic!("wasm trap occured: {}", msg),
                    RuntimeError::Error { data } => {
                        if let Some(error_code) = data.downcast_ref::<wasmer_wasi::ExitCode>() {
                            std::process::exit(error_code.code as i32)
                        }
                    }
                }
                panic!("error: {:?}", err)
            }
        } else {
            let import_object = wasmer_runtime_core::import::ImportObject::new();
            let instance = module
                .instantiate(&import_object)
                .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

            let args: Vec<Value> = options
                .args
                .iter()
                .map(|arg| arg.as_str())
                .map(|x| Value::I32(x.parse().unwrap()))
                .collect();
            instance
                .dyn_func("main")
                .map_err(|e| format!("{:?}", e))?
                .call(&args)
                .map_err(|e| format!("{:?}", e))?;
        }
    }

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

fn validate_wasm(validate: Validate) -> Result<(), String> {
    let wasm_path = validate.path;
    let wasm_path_as_str = wasm_path.to_str().unwrap();

    let wasm_binary: Vec<u8> = read_file_contents(&wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    if !utils::is_wasm_binary(&wasm_binary) {
        return Err(format!(
            "Cannot recognize \"{}\" as a WASM binary",
            wasm_path_as_str,
        ));
    }

    wasmer_runtime_core::validate_and_report_errors(&wasm_binary)
        .map_err(|err| format!("Validation failed: {}", err))?;

    Ok(())
}

/// Runs logic for the `validate` subcommand
fn validate(validate: Validate) {
    match validate_wasm(validate) {
        Err(message) => {
            eprintln!("Error: {}", message);
            exit(-1);
        }
        _ => (),
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
                if cache_dir.exists() {
                    fs::remove_dir_all(cache_dir.clone()).expect("Can't remove cache dir");
                }
                fs::create_dir_all(cache_dir.clone()).expect("Can't create cache dir");
            }
            Cache::Dir => {
                println!("{}", get_cache_dir().to_string_lossy());
            }
        },
        CLIOptions::Validate(validate_options) => {
            validate(validate_options);
        }
        #[cfg(target_os = "windows")]
        CLIOptions::Cache(_) => {
            println!("Caching is disabled for Windows.");
        }
    }
}
