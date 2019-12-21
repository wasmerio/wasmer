#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
extern crate structopt;

use std::collections::HashMap;
use std::env;
use std::fs::{metadata, read_to_string, File};
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;

use structopt::{clap, StructOpt};

use wasmer::*;
#[cfg(feature = "backend-cranelift")]
use wasmer_clif_backend::CraneliftCompiler;
#[cfg(feature = "backend-llvm")]
use wasmer_llvm_backend::{
    InkwellMemoryBuffer, InkwellModule, LLVMBackendConfig, LLVMCallbacks, LLVMCompiler,
};
use wasmer_runtime::{
    cache::{Cache as BaseCache, FileSystemCache, WasmHash},
    Value, VERSION,
};
#[cfg(feature = "managed")]
use wasmer_runtime_core::tiering::{run_tiering, InteractiveShellContext, ShellExitOperation};
use wasmer_runtime_core::{
    self,
    backend::{Backend, Compiler, CompilerConfig, Features, MemoryBoundCheckMode},
    debug,
    loader::{Instance as LoadedInstance, LocalLoader},
    Module,
};
#[cfg(feature = "wasi")]
use wasmer_wasi;

#[cfg(feature = "backend-llvm")]
use std::{cell::RefCell, io::Write, rc::Rc};
#[cfg(feature = "backend-llvm")]
use wasmer_runtime_core::backend::BackendCompilerConfig;

#[cfg(not(any(
    feature = "backend-cranelift",
    feature = "backend-llvm",
    feature = "backend-singlepass"
)))]
compile_error!("Please enable one or more of the compiler backends");

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "Wasm execution runtime.", author)]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wat
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

#[derive(Debug, StructOpt, Clone)]
struct PrestandardFeatures {
    /// Enable support for the SIMD proposal.
    #[structopt(long = "enable-simd")]
    simd: bool,

    /// Enable support for the threads proposal.
    #[structopt(long = "enable-threads")]
    threads: bool,

    /// Enable support for all pre-standard proposals.
    #[structopt(long = "enable-all")]
    all: bool,
}

impl PrestandardFeatures {
    /// Generate [`wabt::Features`] struct from CLI options
    pub fn into_wabt_features(&self) -> wabt::Features {
        let mut features = wabt::Features::new();
        if self.simd || self.all {
            features.enable_simd();
        }
        if self.threads || self.all {
            features.enable_threads();
        }
        features.enable_sign_extension();
        features.enable_sat_float_to_int();
        features
    }

    /// Generate [`Features`] struct from CLI options
    pub fn into_backend_features(&self) -> Features {
        Features {
            simd: self.simd || self.all,
            threads: self.threads || self.all,
        }
    }
}

#[cfg(feature = "backend-llvm")]
#[derive(Debug, StructOpt, Clone)]
/// LLVM backend flags.
pub struct LLVMCLIOptions {
    /// Emit LLVM IR before optimization pipeline.
    #[structopt(long = "llvm-pre-opt-ir", parse(from_os_str))]
    pre_opt_ir: Option<PathBuf>,

    /// Emit LLVM IR after optimization pipeline.
    #[structopt(long = "llvm-post-opt-ir", parse(from_os_str))]
    post_opt_ir: Option<PathBuf>,

    /// Emit LLVM generated native code object file.
    #[structopt(long = "llvm-object-file", parse(from_os_str))]
    obj_file: Option<PathBuf>,
}

#[derive(Debug, StructOpt, Clone)]
struct Run {
    /// Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    /// Name of the backend to use. (x86_64)
    #[cfg(target_arch = "x86_64")]
    #[structopt(
        long = "backend",
        default_value = "auto",
        case_insensitive = true,
        possible_values = Backend::variants(),
    )]
    backend: Backend,

    /// Name of the backend to use. (aarch64)
    #[cfg(target_arch = "aarch64")]
    #[structopt(
        long = "backend",
        default_value = "singlepass",
        case_insensitive = true,
        possible_values = Backend::variants(),
    )]
    backend: Backend,

    /// Invoke a specified function
    #[structopt(long = "invoke", short = "i")]
    invoke: Option<String>,

    /// Emscripten symbol map
    #[structopt(long = "em-symbol-map", parse(from_os_str), group = "emscripten")]
    em_symbol_map: Option<PathBuf>,

    /// Begin execution at the specified symbol
    #[structopt(long = "em-entrypoint", group = "emscripten")]
    em_entrypoint: Option<String>,

    /// WASI pre-opened directory
    #[structopt(long = "dir", multiple = true, group = "wasi")]
    pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the wasm module
    #[structopt(long = "mapdir", multiple = true)]
    mapped_dirs: Vec<String>,

    /// Pass custom environment variables
    #[structopt(long = "env", multiple = true)]
    env_vars: Vec<String>,

    /// Custom code loader
    #[structopt(
        long = "loader",
        case_insensitive = true,
        possible_values = LoaderName::variants(),
    )]
    loader: Option<LoaderName>,

    /// Path to previously saved instance image to resume.
    #[cfg(feature = "managed")]
    #[structopt(long = "resume")]
    resume: Option<String>,

    /// Optimized backends for higher tiers.
    #[cfg(feature = "managed")]
    #[structopt(
        long = "optimized-backends",
        multiple = true,
        case_insensitive = true,
        possible_values = Backend::variants(),
    )]
    optimized_backends: Vec<Backend>,

    /// Whether or not state tracking should be disabled during compilation.
    /// State tracking is necessary for tier switching and backtracing.
    #[structopt(long = "track-state")]
    track_state: bool,

    // Enable the CallTrace middleware.
    #[structopt(long = "call-trace")]
    call_trace: bool,

    // Enable the BlockTrace middleware.
    #[structopt(long = "block-trace")]
    block_trace: bool,

    /// The command name is a string that will override the first argument passed
    /// to the wasm program. This is used in wapm to provide nicer output in
    /// help commands and error messages of the running wasm program
    #[structopt(long = "command-name", hidden = true)]
    command_name: Option<String>,

    /// A prehashed string, used to speed up start times by avoiding hashing the
    /// wasm module. If the specified hash is not found, Wasmer will hash the module
    /// as if no `cache-key` argument was passed.
    #[structopt(long = "cache-key", hidden = true)]
    cache_key: Option<String>,

    #[cfg(feature = "backend-llvm")]
    #[structopt(flatten)]
    backend_llvm_options: LLVMCLIOptions,

    #[structopt(flatten)]
    features: PrestandardFeatures,

    /// Enable non-standard experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[structopt(long = "enable-experimental-io-devices", hidden = true)]
    enable_experimental_io_devices: bool,

    /// Application arguments
    #[structopt(name = "--", multiple = true)]
    args: Vec<String>,
}

impl Run {
    /// Used with the `invoke` argument
    fn parse_args(&self, module: &Module, fn_name: &str) -> Result<Vec<Value>, String> {
        utils::parse_args(module, fn_name, &self.args)
            .map_err(|e| format!("Invoke failed: {:?}", e))
    }
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
enum LoaderName {
    Local,
    #[cfg(feature = "loader-kernel")]
    Kernel,
}

impl LoaderName {
    pub fn variants() -> &'static [&'static str] {
        &[
            "local",
            #[cfg(feature = "loader-kernel")]
            "kernel",
        ]
    }
}

impl FromStr for LoaderName {
    type Err = String;
    fn from_str(s: &str) -> Result<LoaderName, String> {
        match s.to_lowercase().as_str() {
            "local" => Ok(LoaderName::Local),
            #[cfg(feature = "loader-kernel")]
            "kernel" => Ok(LoaderName::Kernel),
            _ => Err(format!("The loader {} doesn't exist", s)),
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

    #[structopt(flatten)]
    features: PrestandardFeatures,
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
        Ok(dir) => {
            let mut path = PathBuf::from(dir);
            path.push(VERSION);
            path
        }
        Err(_) => {
            // We use a temporal directory for saving cache files
            let mut temp_dir = env::temp_dir();
            temp_dir.push("wasmer");
            temp_dir.push(VERSION);
            temp_dir
        }
    }
}

fn get_mapped_dirs(input: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    let mut md = vec![];
    for entry in input.iter() {
        if let [alias, real_dir] = entry.split(':').collect::<Vec<&str>>()[..] {
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
    Ok(md)
}

#[cfg(feature = "wasi")]
fn get_env_var_args(input: &[String]) -> Result<Vec<(&str, &str)>, String> {
    let mut ev = vec![];
    for entry in input.iter() {
        if let [env_var, value] = entry.split('=').collect::<Vec<&str>>()[..] {
            ev.push((env_var, value));
        } else {
            return Err(format!(
                "Env vars must be of the form <var_name>=<value>. Found {}",
                &entry
            ));
        }
    }
    Ok(ev)
}

/// Helper function for `execute_wasm` (the `Run` command)
#[cfg(feature = "wasi")]
fn execute_wasi(
    wasi_version: wasmer_wasi::WasiVersion,
    options: &Run,
    env_vars: Vec<(&str, &str)>,
    module: wasmer_runtime_core::Module,
    mapped_dirs: Vec<(String, PathBuf)>,
    _wasm_binary: &[u8],
) -> Result<(), String> {
    let name = if let Some(cn) = &options.command_name {
        cn.clone()
    } else {
        options.path.to_str().unwrap().to_owned()
    };

    let args = options.args.iter().cloned().map(|arg| arg.into_bytes());
    let preopened_files = options.pre_opened_directories.clone();
    let mut wasi_state_builder = wasmer_wasi::state::WasiState::new(&name);
    wasi_state_builder
        .args(args)
        .envs(env_vars)
        .preopen_dirs(preopened_files)
        .map_dirs(mapped_dirs);

    #[cfg(feature = "experimental-io-devices")]
    {
        if options.enable_experimental_io_devices {
            wasi_state_builder.setup_fs(std::rc::Rc::new(
                wasmer_wasi_experimental_io_devices::initialize,
            ));
        }
    }
    let wasi_state = wasi_state_builder.build().map_err(|e| format!("{:?}", e))?;

    let import_object = wasmer_wasi::generate_import_object_from_state(wasi_state, wasi_version);

    #[allow(unused_mut)] // mut used in feature
    let mut instance = module
        .instantiate(&import_object)
        .map_err(|e| format!("Can't instantiate WASI module: {:?}", e))?;

    let start: wasmer_runtime::Func<(), ()> =
        instance.func("_start").map_err(|e| format!("{:?}", e))?;

    #[cfg(feature = "managed")]
    {
        let start_raw: extern "C" fn(&mut wasmer_runtime_core::vm::Ctx) =
            unsafe { ::std::mem::transmute(start.get_vm_func()) };

        unsafe {
            run_tiering(
                module.info(),
                &_wasm_binary,
                if let Some(ref path) = options.resume {
                    let mut f = File::open(path).unwrap();
                    let mut out: Vec<u8> = vec![];
                    f.read_to_end(&mut out).unwrap();
                    Some(
                        wasmer_runtime_core::state::InstanceImage::from_bytes(&out)
                            .map_err(|_| format!("failed to decode image"))?,
                    )
                } else {
                    None
                },
                &import_object,
                start_raw,
                &mut instance,
                options.backend,
                options
                    .optimized_backends
                    .iter()
                    .map(
                        |&backend| -> (Backend, Box<dyn Fn() -> Box<dyn Compiler> + Send>) {
                            let options = options.clone();
                            (
                                backend,
                                Box::new(move || {
                                    get_compiler_by_backend(backend, &options).unwrap()
                                }),
                            )
                        },
                    )
                    .collect(),
                interactive_shell,
            )?
        };
    }

    #[cfg(not(feature = "managed"))]
    {
        use wasmer_runtime::error::RuntimeError;
        #[cfg(unix)]
        use wasmer_runtime_core::{
            fault::{pop_code_version, push_code_version},
            state::CodeVersion,
        };

        let result;

        #[cfg(unix)]
        let cv_pushed = if let Some(msm) = instance.module.runnable_module.get_module_state_map() {
            push_code_version(CodeVersion {
                baseline: true,
                msm: msm,
                base: instance.module.runnable_module.get_code().unwrap().as_ptr() as usize,
                backend: options.backend,
                runnable_module: instance.module.runnable_module.clone(),
            });
            true
        } else {
            false
        };

        if let Some(invoke_fn) = options.invoke.as_ref() {
            eprintln!("WARNING: Invoking aribtrary functions with WASI is not officially supported in the WASI standard yet.  Use this feature at your own risk!");
            let args = options.parse_args(&module, invoke_fn)?;
            let invoke_result = instance
                .dyn_func(invoke_fn)
                .map_err(|e| format!("Invoke failed: {:?}", e))?
                .call(&args)
                .map_err(|e| format!("Calling invoke fn failed: {:?}", e))?;
            println!("{}({:?}) returned {:?}", invoke_fn, args, invoke_result);
            return Ok(());
        } else {
            result = start.call();
        }

        #[cfg(unix)]
        {
            if cv_pushed {
                pop_code_version().unwrap();
            }
        }

        if let Err(ref err) = result {
            match err {
                RuntimeError::Trap { msg } => return Err(format!("wasm trap occured: {}", msg)),
                RuntimeError::Error { data } => {
                    if let Some(error_code) = data.downcast_ref::<wasmer_wasi::ExitCode>() {
                        std::process::exit(error_code.code as i32)
                    }
                }
            }
            return Err(format!("error: {:?}", err));
        }
    }
    Ok(())
}

#[cfg(feature = "backend-llvm")]
impl LLVMCallbacks for LLVMCLIOptions {
    fn preopt_ir_callback(&mut self, module: &InkwellModule) {
        if let Some(filename) = &self.pre_opt_ir {
            module.print_to_file(filename).unwrap();
        }
    }

    fn postopt_ir_callback(&mut self, module: &InkwellModule) {
        if let Some(filename) = &self.post_opt_ir {
            module.print_to_file(filename).unwrap();
        }
    }

    fn obj_memory_buffer_callback(&mut self, memory_buffer: &InkwellMemoryBuffer) {
        if let Some(filename) = &self.obj_file {
            let mem_buf_slice = memory_buffer.as_slice();
            let mut file = File::create(filename).unwrap();
            let mut pos = 0;
            while pos < mem_buf_slice.len() {
                pos += file.write(&mem_buf_slice[pos..]).unwrap();
            }
        }
    }
}

/// Execute a wasm/wat file
fn execute_wasm(options: &Run) -> Result<(), String> {
    let disable_cache = options.disable_cache;

    let mapped_dirs = get_mapped_dirs(&options.mapped_dirs[..])?;
    #[cfg(feature = "wasi")]
    let env_vars = get_env_var_args(&options.env_vars[..])?;
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
                return Err(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                        .to_string(),
                );
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
                return Err(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                        .to_string(),
                );
            }
            .to_owned();

            em_symbol_map.insert(num, name_str);
        }
        Some(em_symbol_map)
    } else {
        None
    };

    // Don't error on --enable-all for other backends.
    if options.features.simd && options.backend != Backend::LLVM {
        return Err("SIMD is only supported in the LLVM backend for now".to_string());
    }

    if !utils::is_wasm_binary(&wasm_binary) {
        let features = options.features.into_wabt_features();
        wasm_binary = wabt::wat2wasm_with_features(wasm_binary, features)
            .map_err(|e| format!("Can't convert from wast to wasm: {:?}", e))?;
    }

    let compiler: Box<dyn Compiler> = get_compiler_by_backend(options.backend, options)
        .ok_or_else(|| {
            format!(
                "the requested backend, \"{}\", is not enabled",
                options.backend.to_string()
            )
        })?;

    #[allow(unused_mut)]
    let mut backend_specific_config = None;
    #[cfg(feature = "backend-llvm")]
    {
        if options.backend == Backend::LLVM {
            backend_specific_config = Some(BackendCompilerConfig(Box::new(LLVMBackendConfig {
                callbacks: Some(Rc::new(RefCell::new(options.backend_llvm_options.clone()))),
            })))
        }
    }

    let track_state = options.track_state;

    #[cfg(feature = "loader-kernel")]
    let is_kernel_loader = if let Some(LoaderName::Kernel) = options.loader {
        true
    } else {
        false
    };

    #[cfg(not(feature = "loader-kernel"))]
    let is_kernel_loader = false;

    let module = if is_kernel_loader {
        webassembly::compile_with_config_with(
            &wasm_binary[..],
            CompilerConfig {
                symbol_map: em_symbol_map.clone(),
                memory_bound_check_mode: MemoryBoundCheckMode::Disable,
                enforce_stack_check: true,
                track_state,
                features: options.features.into_backend_features(),
                backend_specific_config,
                ..Default::default()
            },
            &*compiler,
        )
        .map_err(|e| format!("Can't compile module: {:?}", e))?
    } else if disable_cache {
        webassembly::compile_with_config_with(
            &wasm_binary[..],
            CompilerConfig {
                symbol_map: em_symbol_map.clone(),
                track_state,
                features: options.features.into_backend_features(),
                backend_specific_config,
                ..Default::default()
            },
            &*compiler,
        )
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
        let load_cache_key = || -> Result<_, String> {
            if let Some(ref prehashed_cache_key) = options.cache_key {
                if let Ok(module) =
                    WasmHash::decode(prehashed_cache_key).and_then(|prehashed_key| {
                        cache.load_with_backend(prehashed_key, options.backend)
                    })
                {
                    debug!("using prehashed key: {}", prehashed_cache_key);
                    return Ok(module);
                }
            }
            // We generate a hash for the given binary, so we can use it as key
            // for the Filesystem cache
            let hash = WasmHash::generate(&wasm_binary);

            // cache.load will return the Module if it's able to deserialize it properly, and an error if:
            // * The file is not found
            // * The file exists, but it's corrupted or can't be converted to a module
            match cache.load_with_backend(hash, options.backend) {
                Ok(module) => {
                    // We are able to load the module from cache
                    Ok(module)
                }
                Err(_) => {
                    let module = webassembly::compile_with_config_with(
                        &wasm_binary[..],
                        CompilerConfig {
                            symbol_map: em_symbol_map.clone(),
                            track_state,
                            features: options.features.into_backend_features(),
                            backend_specific_config,
                            ..Default::default()
                        },
                        &*compiler,
                    )
                    .map_err(|e| format!("Can't compile module: {:?}", e))?;
                    // We try to save the module into a cache file
                    cache.store(hash, module.clone()).unwrap_or_default();

                    Ok(module)
                }
            }
        };

        load_cache_key()?
    };

    if let Some(loader) = options.loader {
        let mut import_object = wasmer_runtime_core::import::ImportObject::new();
        import_object.allow_missing_functions = true; // Import initialization might be left to the loader.
        let instance = module
            .instantiate(&import_object)
            .map_err(|e| format!("Can't instantiate loader module: {:?}", e))?;

        let mut args: Vec<Value> = Vec::new();
        for arg in options.args.iter() {
            let x = arg.as_str().parse().map_err(|_| {
                format!(
                    "Can't parse the provided argument {:?} as a integer",
                    arg.as_str()
                )
            })?;
            args.push(Value::I32(x));
        }

        let index = instance.resolve_func("_start").map_err(|_| {
            format!("The loader requires a _start function to be present in the module")
        })?;

        let mut ins: Box<dyn LoadedInstance<Error = String>> = match loader {
            LoaderName::Local => Box::new(
                instance
                    .load(LocalLoader)
                    .map_err(|e| format!("Can't use the local loader: {:?}", e))?,
            ),
            #[cfg(feature = "loader-kernel")]
            LoaderName::Kernel => Box::new(
                instance
                    .load(::wasmer_kernel_loader::KernelLoader)
                    .map_err(|e| format!("Can't use the local loader: {:?}", e))?,
            ),
        };
        println!("{:?}", ins.call(index, &args));
        return Ok(());
    }

    // TODO: refactor this
    if wasmer_emscripten::is_emscripten_module(&module) {
        let mut emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new(&module)?;
        let import_object = wasmer_emscripten::generate_emscripten_env(&mut emscripten_globals);
        let mut instance = module
            .instantiate(&import_object)
            .map_err(|e| format!("Can't instantiate emscripten module: {:?}", e))?;

        wasmer_emscripten::run_emscripten_instance(
            &module,
            &mut instance,
            &mut emscripten_globals,
            if let Some(cn) = &options.command_name {
                cn
            } else {
                options.path.to_str().unwrap()
            },
            options.args.iter().map(|arg| arg.as_str()).collect(),
            options.em_entrypoint.clone(),
            mapped_dirs,
        )
        .map_err(|e| format!("{:?}", e))?;
    } else {
        #[cfg(feature = "wasi")]
        let wasi_version = wasmer_wasi::get_wasi_version(&module, true);
        #[cfg(feature = "wasi")]
        let is_wasi = wasi_version.is_some();
        #[cfg(not(feature = "wasi"))]
        let is_wasi = false;

        if is_wasi {
            #[cfg(feature = "wasi")]
            execute_wasi(
                wasi_version.unwrap(),
                options,
                env_vars,
                module,
                mapped_dirs,
                &wasm_binary,
            )?;
        } else {
            let import_object = wasmer_runtime_core::import::ImportObject::new();
            let instance = module
                .instantiate(&import_object)
                .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

            let invoke_fn = match options.invoke.as_ref() {
                Some(fun) => fun,
                _ => "main",
            };
            let args = options.parse_args(&module, invoke_fn)?;

            let result = instance
                .dyn_func(&invoke_fn)
                .map_err(|e| format!("{:?}", e))?
                .call(&args)
                .map_err(|e| format!("{:?}", e))?;
            println!("{}({:?}) returned {:?}", invoke_fn, args, result);
        }
    }

    Ok(())
}

#[cfg(feature = "managed")]
fn interactive_shell(mut ctx: InteractiveShellContext) -> ShellExitOperation {
    use std::io::Write;

    let mut stdout = ::std::io::stdout();
    let stdin = ::std::io::stdin();

    loop {
        print!("Wasmer> ");
        stdout.flush().unwrap();
        let mut line = String::new();
        stdin.read_line(&mut line).unwrap();
        let mut parts = line.split(" ").filter(|x| x.len() > 0).map(|x| x.trim());

        let cmd = parts.next();
        if cmd.is_none() {
            println!("Command required");
            continue;
        }
        let cmd = cmd.unwrap();

        match cmd {
            "snapshot" => {
                let path = parts.next();
                if path.is_none() {
                    println!("Usage: snapshot [out_path]");
                    continue;
                }
                let path = path.unwrap();

                if let Some(ref image) = ctx.image {
                    let buf = image.to_bytes();
                    let mut f = match File::create(path) {
                        Ok(x) => x,
                        Err(e) => {
                            println!("Cannot open output file at {}: {:?}", path, e);
                            continue;
                        }
                    };
                    if let Err(e) = f.write_all(&buf) {
                        println!("Cannot write to output file at {}: {:?}", path, e);
                        continue;
                    }
                    println!("Done");
                } else {
                    println!("Program state not available");
                }
            }
            "continue" | "c" => {
                if let Some(image) = ctx.image.take() {
                    return ShellExitOperation::ContinueWith(image);
                } else {
                    println!("Program state not available, cannot continue execution");
                }
            }
            "backtrace" | "bt" => {
                if let Some(ref image) = ctx.image {
                    println!("{}", image.execution_state.output());
                } else {
                    println!("State not available");
                }
            }
            "exit" | "quit" => {
                exit(0);
            }
            "" => {}
            _ => {
                println!("Unknown command: {}", cmd);
            }
        }
    }
}

fn update_backend(options: &mut Run) {
    let binary_size = match metadata(&options.path) {
        Ok(wasm_binary) => wasm_binary.len(),
        Err(_e) => 0,
    };

    // Update backend when a backend flag is `auto`.
    // Use the Singlepass backend if it's enabled and the file provided is larger
    // than 10MiB (10485760 bytes), or it's enabled and the target architecture
    // is AArch64. Otherwise, use the Cranelift backend.
    if options.backend == Backend::Auto {
        if Backend::variants().contains(&Backend::Singlepass.to_string())
            && (binary_size > 10485760 || cfg!(target_arch = "aarch64"))
        {
            options.backend = Backend::Singlepass;
        } else {
            options.backend = Backend::Cranelift;
        }
    }
}

fn run(options: &mut Run) {
    update_backend(options);
    match execute_wasm(options) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("Error: {}", message);
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

    wasmer_runtime_core::validate_and_report_errors_with_features(
        &wasm_binary,
        validate.features.into_backend_features(),
    )
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

fn get_compiler_by_backend(backend: Backend, _opts: &Run) -> Option<Box<dyn Compiler>> {
    Some(match backend {
        #[cfg(feature = "backend-singlepass")]
        Backend::Singlepass => {
            use wasmer_runtime_core::codegen::MiddlewareChain;
            use wasmer_runtime_core::codegen::StreamingCompiler;
            use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;

            let opts = _opts.clone();
            let middlewares_gen = move || {
                let mut middlewares = MiddlewareChain::new();
                if opts.call_trace {
                    use wasmer_middleware_common::call_trace::CallTrace;
                    middlewares.push(CallTrace::new());
                }
                if opts.block_trace {
                    use wasmer_middleware_common::block_trace::BlockTrace;
                    middlewares.push(BlockTrace::new());
                }
                middlewares
            };

            let c: StreamingCompiler<SinglePassMCG, _, _, _, _> =
                StreamingCompiler::new(middlewares_gen);
            Box::new(c)
        }
        #[cfg(not(feature = "backend-singlepass"))]
        Backend::Singlepass => return None,
        #[cfg(feature = "backend-cranelift")]
        Backend::Cranelift => Box::new(CraneliftCompiler::new()),
        #[cfg(not(feature = "backend-cranelift"))]
        Backend::Cranelift => return None,
        #[cfg(feature = "backend-llvm")]
        Backend::LLVM => Box::new(LLVMCompiler::new()),
        #[cfg(not(feature = "backend-llvm"))]
        Backend::LLVM => return None,
        Backend::Auto => return None,
    })
}

fn main() {
    // We try to run wasmer with the normal arguments.
    // Eg. `wasmer <SUBCOMMAND>`
    // In case that fails, we fallback trying the Run subcommand directly.
    // Eg. `wasmer myfile.wasm --dir=.`
    let options = CLIOptions::from_iter_safe(env::args()).unwrap_or_else(|e| {
        match e.kind {
            // This fixes a issue that:
            // 1. Shows the version twice when doing `wasmer -V`
            // 2. Shows the run help (instead of normal help) when doing `wasmer --help`
            clap::ErrorKind::VersionDisplayed | clap::ErrorKind::HelpDisplayed => e.exit(),
            _ => CLIOptions::Run(Run::from_args()),
        }
    });
    match options {
        CLIOptions::Run(mut options) => run(&mut options),
        #[cfg(not(target_os = "windows"))]
        CLIOptions::SelfUpdate => update::self_update(),
        #[cfg(target_os = "windows")]
        CLIOptions::SelfUpdate => {
            println!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
        }
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
    }
}

#[test]
fn filesystem_cache_should_work() -> Result<(), String> {
    let wasmer_cache_dir = get_cache_dir();

    unsafe { FileSystemCache::new(wasmer_cache_dir).map_err(|e| format!("Cache error: {:?}", e))? };

    Ok(())
}
