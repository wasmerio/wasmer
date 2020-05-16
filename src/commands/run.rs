use crate::common::get_cache_dir;
use crate::store::StoreOptions;
use crate::suggestions::suggest_function_exports;
use crate::warning;
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use wasmer::*;
#[cfg(feature = "cache")]
use wasmer_cache::{Cache, FileSystemCache, IoDeserializeError, WasmHash};
#[cfg(feature = "jit")]
use wasmer_engine_jit::JITEngine;

use structopt::StructOpt;

#[cfg(feature = "wasi")]
mod wasi;

#[cfg(feature = "wasi")]
use wasi::Wasi;

#[derive(Debug, StructOpt, Clone)]
/// The options for the `wasmer run` subcommand
pub struct Run {
    /// Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// File to run
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Invoke a specified function
    #[structopt(long = "invoke", short = "i")]
    invoke: Option<String>,

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

    #[structopt(flatten)]
    compiler: StoreOptions,

    #[cfg(feature = "wasi")]
    #[structopt(flatten)]
    wasi: Wasi,

    /// Enable non-standard experimental IO devices
    #[cfg(feature = "io-devices")]
    #[structopt(long = "enable-io-devices")]
    enable_experimental_io_devices: bool,

    /// Enable debug output
    #[cfg(feature = "debug")]
    #[structopt(long = "debug", short = "d")]
    debug: bool,

    /// Application arguments
    #[structopt(name = "--", multiple = true)]
    args: Vec<String>,
}

impl Run {
    #[cfg(feature = "cache")]
    /// Get the Compiler Filesystem cache
    fn get_cache(&self, compiler_name: String) -> Result<FileSystemCache> {
        let mut cache_dir_root = get_cache_dir();
        cache_dir_root.push(compiler_name);
        Ok(FileSystemCache::new(cache_dir_root)?)
    }
    /// Execute the run command
    pub fn execute(&self) -> Result<()> {
        self.inner_execute()
            .context(format!("failed to run `{}`", self.path.display()))
    }
    fn inner_execute(&self) -> Result<()> {
        let module = self
            .get_module()
            .with_context(|| "module instantiation failed")?;
        // Do we want to invoke a function?
        if let Some(ref invoke) = self.invoke {
            let imports = imports! {};
            let instance = Instance::new(&module, &imports)?;
            let result = self.invoke_function(&instance, &invoke, &self.args)?;
            println!(
                "{}",
                result
                    .iter()
                    .map(|val| val.to_string())
                    .collect::<Vec<String>>()
                    .join(" ")
            );
            return Ok(());
        }

        // If WASI is enabled, try to execute it with it
        #[cfg(feature = "wasi")]
        {
            let wasi_version = Wasi::get_version(&module);
            if let Some(version) = wasi_version {
                let program_name = self
                    .command_name
                    .clone()
                    .or_else(|| {
                        self.path
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                    })
                    .unwrap_or_default();
                return self
                    .wasi
                    .execute(module, version, program_name, self.args.clone())
                    .with_context(|| "WASI execution failed");
            }
        }

        // Try to instantiate the wasm file, with no provided imports
        let imports = imports! {};
        let instance = Instance::new(&module, &imports)?;
        let start: Function = self.try_find_function(&instance, "_start", &[])?;
        start.call(&[])?;

        Ok(())
    }

    fn get_module(&self) -> Result<Module> {
        let contents = std::fs::read(self.path.clone())?;
        #[cfg(feature = "jit")]
        {
            if JITEngine::is_deserializable(&contents) {
                let tunables = Tunables::default();
                let engine = JITEngine::headless(tunables);
                let store = Store::new(Arc::new(engine));
                let module = unsafe { Module::deserialize(&store, &contents)? };
                return Ok(module);
            }
        }
        let (store, compiler_name) = self.compiler.get_store()?;
        // We try to get it from cache, in case caching is enabled
        // and the file length is greater than 4KB.
        // For files smaller than 4KB caching is not worth,
        // as it takes space and the speedup is minimal.
        let mut module =
            if cfg!(feature = "cache") && !self.disable_cache && contents.len() > 0x1000 {
                let mut cache = self.get_cache(compiler_name)?;
                // Try to get the hash from the provided `--cache-key`, otherwise
                // generate one from the provided file `.wasm` contents.
                let hash = self
                    .cache_key
                    .as_ref()
                    .and_then(|key| WasmHash::from_str(&key).ok())
                    .unwrap_or_else(|| WasmHash::generate(&contents));
                match unsafe { cache.load(&store, hash) } {
                    Ok(module) => module,
                    Err(e) => {
                        match e {
                            IoDeserializeError::Deserialize(e) => {
                                warning!("cached module is corrupted: {}", e);
                            }
                            IoDeserializeError::Io(_) => {
                                // Do not notify on IO errors
                            }
                        }
                        let module = Module::new(&store, &contents)?;
                        // Store the compiled Module in cache
                        cache.store(hash, module.clone())?;
                        module
                    }
                }
            } else {
                Module::new(&store, &contents)?
            };
        // We set the name outside the cache, to make sure we dont cache the name
        module.set_name(&self.path.file_name().unwrap_or_default().to_string_lossy());

        Ok(module)
    }

    fn try_find_function(
        &self,
        instance: &Instance,
        name: &str,
        args: &[String],
    ) -> Result<Function> {
        Ok(instance
            .exports
            .get_function(&name)
            .map_err(|e| {
                if instance.module().info().functions.is_empty() {
                    anyhow!("The module has no exported functions to call.")
                } else {
                    let suggested_functions = suggest_function_exports(instance.module(), "");
                    let names = suggested_functions
                        .iter()
                        .take(3)
                        .map(|arg| format!("`{}`", arg))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let suggested_command = format!(
                        "wasmer {} -i {} {}",
                        self.path.display(),
                        suggested_functions.get(0).unwrap(),
                        args.join(" ")
                    );
                    let suggestion = format!(
                        "Similar functions found: {}.\nTry with: {}",
                        names, suggested_command
                    );
                    match e {
                        ExportError::Missing(_) => {
                            anyhow!("No export `{}` found in the module.\n{}", name, suggestion)
                        }
                        ExportError::IncompatibleType => anyhow!(
                            "Export `{}` found, but is not a function.\n{}",
                            name,
                            suggestion
                        ),
                    }
                }
            })?
            .clone())
    }

    fn invoke_function(
        &self,
        instance: &Instance,
        invoke: &str,
        args: &[String],
    ) -> Result<Box<[Val]>> {
        let func: Function = self.try_find_function(&instance, invoke, args)?;
        let func_ty = func.ty();
        let required_arguments = func_ty.params().len();
        let provided_arguments = args.len();
        if required_arguments != provided_arguments {
            bail!(
                "Function expected {} arguments, but received {}: \"{}\"",
                required_arguments,
                provided_arguments,
                self.args.join(" ")
            );
        }
        let invoke_args = args
            .iter()
            .zip(func_ty.params().iter())
            .map(|(arg, param_type)| match param_type {
                ValType::I32 => {
                    Ok(Val::I32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i32", arg)
                    })?))
                }
                ValType::I64 => {
                    Ok(Val::I64(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i64", arg)
                    })?))
                }
                ValType::F32 => {
                    Ok(Val::F32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a f32", arg)
                    })?))
                }
                ValType::F64 => {
                    Ok(Val::F64(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a f64", arg)
                    })?))
                }
                _ => Err(anyhow!(
                    "Don't know how to convert {} into {:?}",
                    arg,
                    param_type
                )),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(func.call(&invoke_args)?)
    }
}
