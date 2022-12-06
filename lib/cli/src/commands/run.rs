use crate::cli::SplitVersion;
use crate::common::get_cache_dir;
#[cfg(feature = "debug")]
use crate::logging;
use crate::store::{CompilerType, StoreOptions};
use crate::suggestions::suggest_function_exports;
use crate::warning;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;
use wasmer::FunctionEnv;
use wasmer::*;
#[cfg(feature = "cache")]
use wasmer_cache::{Cache, FileSystemCache, Hash};
use wasmer_registry::PackageDownloadInfo;
use wasmer_types::Type as ValueType;
#[cfg(feature = "webc_runner")]
use wasmer_wasi::runners::{Runner, WapmContainer};

#[cfg(feature = "wasi")]
mod wasi;

#[cfg(feature = "wasi")]
use wasi::Wasi;

/// Same as `wasmer run`, but without the required `path` argument (injected previously)
#[derive(Debug, Parser, Clone, Default)]
pub struct RunWithoutFile {
    /// When installing packages with `wasmer $package`, force re-downloading the package
    #[clap(long = "force", short = 'f')]
    pub(crate) force_install: bool,

    /// Disable the cache
    #[cfg(feature = "cache")]
    #[clap(long = "disable-cache")]
    pub(crate) disable_cache: bool,

    /// Invoke a specified function
    #[clap(long = "invoke", short = 'i')]
    pub(crate) invoke: Option<String>,

    /// The command name is a string that will override the first argument passed
    /// to the wasm program. This is used in wapm to provide nicer output in
    /// help commands and error messages of the running wasm program
    #[clap(long = "command-name", hide = true)]
    pub(crate) command_name: Option<String>,

    /// A prehashed string, used to speed up start times by avoiding hashing the
    /// wasm module. If the specified hash is not found, Wasmer will hash the module
    /// as if no `cache-key` argument was passed.
    #[cfg(feature = "cache")]
    #[clap(long = "cache-key", hide = true)]
    pub(crate) cache_key: Option<String>,

    #[clap(flatten)]
    pub(crate) store: StoreOptions,

    // TODO: refactor WASI structure to allow shared options with Emscripten
    #[cfg(feature = "wasi")]
    #[clap(flatten)]
    pub(crate) wasi: Wasi,

    /// Enable non-standard experimental IO devices
    #[cfg(feature = "io-devices")]
    #[clap(long = "enable-io-devices")]
    pub(crate) enable_experimental_io_devices: bool,

    /// Enable debug output
    #[cfg(feature = "debug")]
    #[clap(long = "debug", short = 'd')]
    pub(crate) debug: bool,

    #[cfg(feature = "debug")]
    #[clap(long = "verbose")]
    pub(crate) verbose: Option<u8>,

    /// Application arguments
    #[clap(value_name = "ARGS")]
    pub(crate) args: Vec<String>,
}

#[allow(dead_code)]
fn is_dir(e: &walkdir::DirEntry) -> bool {
    let meta = match e.metadata() {
        Ok(o) => o,
        Err(_) => return false,
    };
    meta.is_dir()
}

impl RunWithoutFile {
    /// Given a local path, returns the `Run` command (overriding the `--path` argument).
    pub fn into_run_args(
        mut self,
        package_root_dir: PathBuf, // <- package dir
        command: Option<&str>,
        _debug_output_allowed: bool,
    ) -> Result<Run, anyhow::Error> {
        let (manifest, pathbuf) =
            wasmer_registry::get_executable_file_from_path(&package_root_dir, command)?;

        #[cfg(feature = "wasi")]
        {
            let default = HashMap::default();
            let fs = manifest.fs.as_ref().unwrap_or(&default);
            for (alias, real_dir) in fs.iter() {
                let real_dir = package_root_dir.join(&real_dir);
                if !real_dir.exists() {
                    if _debug_output_allowed {
                        println!(
                            "warning: cannot map {alias:?} to {}: directory does not exist",
                            real_dir.display()
                        );
                    }
                    continue;
                }

                self.wasi.map_dir(alias, real_dir.clone());
            }
        }

        Ok(Run {
            path: pathbuf,
            options: RunWithoutFile {
                force_install: self.force_install,
                #[cfg(feature = "cache")]
                disable_cache: self.disable_cache,
                invoke: self.invoke,
                // If the RunWithoutFile was constructed via a package name,
                // the correct syntax is "package:command-name" (--command-name would be
                // interpreted as a CLI argument for the .wasm file)
                command_name: None,
                #[cfg(feature = "cache")]
                cache_key: self.cache_key,
                store: self.store,
                #[cfg(feature = "wasi")]
                wasi: self.wasi,
                #[cfg(feature = "io-devices")]
                enable_experimental_io_devices: self.enable_experimental_io_devices,
                #[cfg(feature = "debug")]
                debug: self.debug,
                #[cfg(feature = "debug")]
                verbose: self.verbose,
                args: self.args,
            },
        })
    }
}

#[derive(Debug, Parser, Clone, Default)]
/// The options for the `wasmer run` subcommand
pub struct Run {
    /// File to run
    #[clap(name = "FILE", parse(from_os_str))]
    pub(crate) path: PathBuf,

    #[clap(flatten)]
    pub(crate) options: RunWithoutFile,
}

impl Deref for Run {
    type Target = RunWithoutFile;
    fn deref(&self) -> &Self::Target {
        &self.options
    }
}

impl Run {
    /// Execute the run command
    pub fn execute(&self) -> Result<()> {
        #[cfg(feature = "debug")]
        if self.debug {
            logging::set_up_logging(self.verbose.unwrap_or(0)).unwrap();
        }
        self.inner_execute().with_context(|| {
            format!(
                "failed to run `{}`{}",
                self.path.display(),
                if CompilerType::enabled().is_empty() {
                    " (no compilers enabled)"
                } else {
                    ""
                }
            )
        })
    }

    fn inner_module_run(&self, mut store: Store, instance: Instance) -> Result<()> {
        // If this module exports an _initialize function, run that first.
        if let Ok(initialize) = instance.exports.get_function("_initialize") {
            initialize
                .call(&mut store, &[])
                .with_context(|| "failed to run _initialize function")?;
        }

        // Do we want to invoke a function?
        if let Some(ref invoke) = self.invoke {
            let result = self.invoke_function(&mut store, &instance, invoke, &self.args)?;
            println!(
                "{}",
                result
                    .iter()
                    .map(|val| val.to_string())
                    .collect::<Vec<String>>()
                    .join(" ")
            );
        } else {
            let start: Function = self.try_find_function(&instance, "_start", &[])?;
            let result = start.call(&mut store, &[]);
            #[cfg(feature = "wasi")]
            self.wasi.handle_result(result)?;
            #[cfg(not(feature = "wasi"))]
            result?;
        }

        Ok(())
    }

    fn inner_execute(&self) -> Result<()> {
        #[cfg(feature = "webc_runner")]
        {
            if let Ok(pf) = WapmContainer::new(self.path.clone()) {
                return Self::run_container(
                    pf,
                    &self.command_name.clone().unwrap_or_default(),
                    &self.args,
                )
                .map_err(|e| anyhow!("Could not run PiritaFile: {e}"));
            }
        }
        let (mut store, module) = self.get_store_module()?;
        #[cfg(feature = "emscripten")]
        {
            use wasmer_emscripten::{
                generate_emscripten_env, is_emscripten_module, run_emscripten_instance, EmEnv,
                EmscriptenGlobals,
            };
            // TODO: refactor this
            if is_emscripten_module(&module) {
                let em_env = EmEnv::new();
                for (k, v) in self.wasi.env_vars.iter() {
                    em_env.set_env_var(k, v);
                }
                // create an EmEnv with default global
                let env = FunctionEnv::new(&mut store, em_env);
                let mut emscripten_globals = EmscriptenGlobals::new(&mut store, &env, &module)
                    .map_err(|e| anyhow!("{}", e))?;
                env.as_mut(&mut store).set_data(
                    &emscripten_globals.data,
                    self.wasi.mapped_dirs.clone().into_iter().collect(),
                );
                let import_object =
                    generate_emscripten_env(&mut store, &env, &mut emscripten_globals);
                let mut instance = match Instance::new(&mut store, &module, &import_object) {
                    Ok(instance) => instance,
                    Err(e) => {
                        let err: Result<(), _> = Err(e);
                        #[cfg(feature = "wasi")]
                        {
                            if Wasi::has_wasi_imports(&module) {
                                return err.with_context(|| "This module has both Emscripten and WASI imports. Wasmer does not currently support Emscripten modules using WASI imports.");
                            }
                        }
                        return err.with_context(|| "Can't instantiate emscripten module");
                    }
                };

                run_emscripten_instance(
                    &mut instance,
                    env.into_mut(&mut store),
                    &mut emscripten_globals,
                    if let Some(cn) = &self.command_name {
                        cn
                    } else {
                        self.path.to_str().unwrap()
                    },
                    self.args.iter().map(|arg| arg.as_str()).collect(),
                    self.invoke.clone(),
                )?;
                return Ok(());
            }
        }

        // If WASI is enabled, try to execute it with it
        #[cfg(feature = "wasi")]
        let ret = {
            use std::collections::BTreeSet;
            use wasmer_wasi::WasiVersion;

            let wasi_versions = Wasi::get_versions(&module);
            match wasi_versions {
                Some(wasi_versions) if !wasi_versions.is_empty() => {
                    if wasi_versions.len() >= 2 {
                        let get_version_list = |versions: &BTreeSet<WasiVersion>| -> String {
                            versions
                                .iter()
                                .map(|v| format!("`{}`", v.get_namespace_str()))
                                .collect::<Vec<String>>()
                                .join(", ")
                        };
                        if self.wasi.deny_multiple_wasi_versions {
                            let version_list = get_version_list(&wasi_versions);
                            bail!("Found more than 1 WASI version in this module ({}) and `--deny-multiple-wasi-versions` is enabled.", version_list);
                        } else if !self.wasi.allow_multiple_wasi_versions {
                            let version_list = get_version_list(&wasi_versions);
                            warning!("Found more than 1 WASI version in this module ({}). If this is intentional, pass `--allow-multiple-wasi-versions` to suppress this warning.", version_list);
                        }
                    }

                    let program_name = self
                        .command_name
                        .clone()
                        .or_else(|| {
                            self.path
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string())
                        })
                        .unwrap_or_default();
                    let (_ctx, instance) = self
                        .wasi
                        .instantiate(&mut store, &module, program_name, self.args.clone())
                        .with_context(|| "failed to instantiate WASI module")?;
                    self.inner_module_run(store, instance)
                }
                // not WASI
                _ => {
                    let instance = Instance::new(&mut store, &module, &imports! {})?;
                    self.inner_module_run(store, instance)
                }
            }
        };
        #[cfg(not(feature = "wasi"))]
        let ret = {
            let instance = Instance::new(&module, &imports! {})?;

            // If this module exports an _initialize function, run that first.
            if let Ok(initialize) = instance.exports.get_function("_initialize") {
                initialize
                    .call(&[])
                    .with_context(|| "failed to run _initialize function")?;
            }

            // Do we want to invoke a function?
            if let Some(ref invoke) = self.invoke {
                let result = self.invoke_function(&instance, invoke, &self.args)?;
                println!(
                    "{}",
                    result
                        .iter()
                        .map(|val| val.to_string())
                        .collect::<Vec<String>>()
                        .join(" ")
                );
            } else {
                let start: Function = self.try_find_function(&instance, "_start", &[])?;
                let result = start.call(&[]);
                #[cfg(feature = "wasi")]
                self.wasi.handle_result(result)?;
                #[cfg(not(feature = "wasi"))]
                result?;
            }
        };

        ret
    }

    #[cfg(feature = "webc_runner")]
    fn run_container(container: WapmContainer, id: &str, args: &[String]) -> Result<(), String> {
        let mut result = None;

        #[cfg(feature = "wasi")]
        {
            if let Some(r) = result {
                return r;
            }

            let mut runner = wasmer_wasi::runners::wasi::WasiRunner::default();
            runner.set_args(args.to_vec());
            result = Some(if id.is_empty() {
                runner.run(&container).map_err(|e| format!("{e}"))
            } else {
                runner.run_cmd(&container, id).map_err(|e| format!("{e}"))
            });
        }

        #[cfg(feature = "emscripten")]
        {
            if let Some(r) = result {
                return r;
            }

            let mut runner = wasmer_wasi::runners::emscripten::EmscriptenRunner::default();
            runner.set_args(args.to_vec());
            result = Some(if id.is_empty() {
                runner.run(&container).map_err(|e| format!("{e}"))
            } else {
                runner.run_cmd(&container, id).map_err(|e| format!("{e}"))
            });
        }

        result.unwrap_or_else(|| Err("neither emscripten or wasi file".to_string()))
    }

    fn get_store_module(&self) -> Result<(Store, Module)> {
        let contents = std::fs::read(self.path.clone())?;
        if wasmer_compiler::Artifact::is_deserializable(&contents) {
            let engine = wasmer_compiler::EngineBuilder::headless();
            let store = Store::new(engine);
            let module = unsafe { Module::deserialize_from_file(&store, &self.path)? };
            return Ok((store, module));
        }
        let (store, compiler_type) = self.store.get_store()?;
        #[cfg(feature = "cache")]
        let module_result: Result<Module> = if !self.disable_cache && contents.len() > 0x1000 {
            self.get_module_from_cache(&store, &contents, &compiler_type)
        } else {
            Module::new(&store, contents).map_err(|e| e.into())
        };
        #[cfg(not(feature = "cache"))]
        let module_result = Module::new(&store, &contents);

        let mut module = module_result.with_context(|| {
            format!(
                "module instantiation failed (compiler: {})",
                compiler_type.to_string()
            )
        })?;
        // We set the name outside the cache, to make sure we dont cache the name
        module.set_name(&self.path.file_name().unwrap_or_default().to_string_lossy());

        Ok((store, module))
    }

    #[cfg(feature = "cache")]
    fn get_module_from_cache(
        &self,
        store: &Store,
        contents: &[u8],
        compiler_type: &CompilerType,
    ) -> Result<Module> {
        // We try to get it from cache, in case caching is enabled
        // and the file length is greater than 4KB.
        // For files smaller than 4KB caching is not worth,
        // as it takes space and the speedup is minimal.
        let mut cache = self.get_cache(compiler_type)?;
        // Try to get the hash from the provided `--cache-key`, otherwise
        // generate one from the provided file `.wasm` contents.
        let hash = self
            .cache_key
            .as_ref()
            .and_then(|key| Hash::from_str(key).ok())
            .unwrap_or_else(|| Hash::generate(contents));
        match unsafe { cache.load(store, hash) } {
            Ok(module) => Ok(module),
            Err(e) => {
                match e {
                    DeserializeError::Io(_) => {
                        // Do not notify on IO errors
                    }
                    err => {
                        warning!("cached module is corrupted: {}", err);
                    }
                }
                let module = Module::new(store, contents)?;
                // Store the compiled Module in cache
                cache.store(hash, &module)?;
                Ok(module)
            }
        }
    }

    #[cfg(feature = "cache")]
    /// Get the Compiler Filesystem cache
    fn get_cache(&self, compiler_type: &CompilerType) -> Result<FileSystemCache> {
        let mut cache_dir_root = get_cache_dir();
        cache_dir_root.push(compiler_type.to_string());
        let mut cache = FileSystemCache::new(cache_dir_root)?;

        let extension = "wasmu";
        cache.set_cache_extension(Some(extension));
        Ok(cache)
    }

    fn try_find_function(
        &self,
        instance: &Instance,
        name: &str,
        args: &[String],
    ) -> Result<Function> {
        Ok(instance
            .exports
            .get_function(name)
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
                        suggested_functions.get(0).unwrap_or(&String::new()),
                        args.join(" ")
                    );
                    let suggestion = if suggested_functions.is_empty() {
                        String::from("Can not find any export functions.")
                    } else {
                        format!(
                            "Similar functions found: {}.\nTry with: {}",
                            names, suggested_command
                        )
                    };
                    match e {
                        ExportError::Missing(_) => {
                            anyhow!("No export `{}` found in the module.\n{}", name, suggestion)
                        }
                        ExportError::IncompatibleType => anyhow!(
                            "Export `{}` found, but is not a function.\n{}",
                            name,
                            suggestion
                        ),
                        ExportError::SerializationFailed(err) => {
                            anyhow!("Failed to serialize the module - {}", err)
                        }
                    }
                }
            })?
            .clone())
    }

    fn invoke_function(
        &self,
        ctx: &mut impl AsStoreMut,
        instance: &Instance,
        invoke: &str,
        args: &[String],
    ) -> Result<Box<[Value]>> {
        let func: Function = self.try_find_function(instance, invoke, args)?;
        let func_ty = func.ty(ctx);
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
                ValueType::I32 => {
                    Ok(Value::I32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i32", arg)
                    })?))
                }
                ValueType::I64 => {
                    Ok(Value::I64(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i64", arg)
                    })?))
                }
                ValueType::F32 => {
                    Ok(Value::F32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a f32", arg)
                    })?))
                }
                ValueType::F64 => {
                    Ok(Value::F64(arg.parse().map_err(|_| {
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
        Ok(func.call(ctx, &invoke_args)?)
    }

    /// Create Run instance for arguments/env,
    /// assuming we're being run from a CFP binfmt interpreter.
    pub fn from_binfmt_args() -> Run {
        Self::from_binfmt_args_fallible().unwrap_or_else(|e| {
            crate::error::PrettyError::report::<()>(
                Err(e).context("Failed to set up wasmer binfmt invocation"),
            )
        })
    }

    #[cfg(target_os = "linux")]
    fn from_binfmt_args_fallible() -> Result<Run> {
        let argv = std::env::args_os().collect::<Vec<_>>();
        let (_interpreter, executable, original_executable, args) = match &argv[..] {
            [a, b, c, d @ ..] => (a, b, c, d),
            _ => {
                bail!("Wasmer binfmt interpreter needs at least three arguments (including $0) - must be registered as binfmt interpreter with the CFP flags. (Got arguments: {:?})", argv);
            }
        };
        // TODO: Optimally, args and env would be passed as an UTF-8 Vec.
        // (Can be pulled out of std::os::unix::ffi::OsStrExt)
        // But I don't want to duplicate or rewrite run.rs today.
        let args = args
            .iter()
            .enumerate()
            .map(|(i, s)| {
                s.clone().into_string().map_err(|s| {
                    anyhow!(
                        "Cannot convert argument {} ({:?}) to UTF-8 string",
                        i + 1,
                        s
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let original_executable = original_executable
            .clone()
            .into_string()
            .map_err(|s| anyhow!("Cannot convert executable name {:?} to UTF-8 string", s))?;
        let store = StoreOptions::default();
        // TODO: store.compiler.features.all = true; ?
        Ok(Self {
            path: executable.into(),
            options: RunWithoutFile {
                args,
                command_name: Some(original_executable),
                store,
                wasi: Wasi::for_binfmt_interpreter()?,
                ..Default::default()
            },
        })
    }
    #[cfg(not(target_os = "linux"))]
    fn from_binfmt_args_fallible() -> Result<Run> {
        bail!("binfmt_misc is only available on linux.")
    }
}

fn start_spinner(msg: String) -> Option<spinoff::Spinner> {
    if !isatty::stdout_isatty() {
        return None;
    }
    #[cfg(target_os = "windows")]
    {
        use colored::control;
        let _ = control::set_virtual_terminal(true);
    }
    Some(spinoff::Spinner::new(
        spinoff::Spinners::Dots,
        msg,
        spinoff::Color::White,
    ))
}

/// Before looking up a command from the registry, try to see if we have
/// the command already installed
fn try_run_local_command(
    args: &[String],
    sv: &SplitVersion,
    debug_msgs_allowed: bool,
) -> Result<(), ExecuteLocalPackageError> {
    let result = wasmer_registry::try_finding_local_command(&sv.original).ok_or_else(|| {
        ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!(
            "could not find command {} locally",
            sv.original
        ))
    })?;
    let package_dir = result
        .get_path()
        .map_err(|e| ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!("{e}")))?;

    // Try auto-installing the remote package
    let args_without_package = fixup_args(args, &sv.original);
    let mut run_args = RunWithoutFile::try_parse_from(args_without_package.iter())
        .map_err(|e| ExecuteLocalPackageError::DuringExec(e.into()))?;
    run_args.command_name = sv.command.clone();

    run_args
        .into_run_args(package_dir, sv.command.as_deref(), debug_msgs_allowed)
        .map_err(ExecuteLocalPackageError::DuringExec)?
        .execute()
        .map_err(ExecuteLocalPackageError::DuringExec)
}

pub(crate) fn try_autoinstall_package(
    args: &[String],
    sv: &SplitVersion,
    package: Option<PackageDownloadInfo>,
    force_install: bool,
) -> Result<(), anyhow::Error> {
    use std::io::Write;
    let mut sp = start_spinner(format!("Installing package {} ...", sv.package));
    let debug_msgs_allowed = sp.is_some();
    let v = sv.version.as_deref();
    let result = wasmer_registry::install_package(
        sv.registry.as_deref(),
        &sv.package,
        v,
        package,
        force_install,
    );
    if let Some(sp) = sp.take() {
        sp.clear();
    }
    let _ = std::io::stdout().flush();
    let (_, package_dir) = match result {
        Ok(o) => o,
        Err(e) => {
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Try auto-installing the remote package
    let args_without_package = fixup_args(args, &sv.original);
    let mut run_args = RunWithoutFile::try_parse_from(args_without_package.iter())?;
    run_args.command_name = sv.command.clone();

    run_args
        .into_run_args(package_dir, sv.command.as_deref(), debug_msgs_allowed)?
        .execute()
}

// We need to distinguish between errors that happen
// before vs. during execution
enum ExecuteLocalPackageError {
    BeforeExec(anyhow::Error),
    DuringExec(anyhow::Error),
}

fn try_execute_local_package(
    args: &[String],
    sv: &SplitVersion,
    debug_msgs_allowed: bool,
) -> Result<(), ExecuteLocalPackageError> {
    let package = wasmer_registry::get_local_package(None, &sv.package, sv.version.as_deref())
        .ok_or_else(|| {
            ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!("no local package {sv:?} found"))
        })?;

    let package_dir = package
        .get_path()
        .map_err(|e| ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!("{e}")))?;

    // Try finding the local package
    let args_without_package = fixup_args(args, &sv.original);

    RunWithoutFile::try_parse_from(args_without_package.iter())
        .map_err(|e| ExecuteLocalPackageError::DuringExec(e.into()))?
        .into_run_args(package_dir, sv.command.as_deref(), debug_msgs_allowed)
        .map_err(ExecuteLocalPackageError::DuringExec)?
        .execute()
        .map_err(|e| ExecuteLocalPackageError::DuringExec(e.context(anyhow::anyhow!("{}", sv))))
}

fn try_lookup_command(sv: &mut SplitVersion) -> Result<PackageDownloadInfo, anyhow::Error> {
    use std::io::Write;
    let mut sp = start_spinner(format!("Looking up command {} ...", sv.package));

    for registry in wasmer_registry::get_all_available_registries().unwrap_or_default() {
        let result = wasmer_registry::query_command_from_registry(&registry, &sv.package);
        if let Some(s) = sp.take() {
            s.clear();
        }
        let _ = std::io::stdout().flush();
        let command = sv.package.clone();
        if let Ok(o) = result {
            sv.package = o.package.clone();
            sv.version = Some(o.version.clone());
            sv.command = Some(command);
            return Ok(o);
        }
    }

    if let Some(sp) = sp.take() {
        sp.clear();
    }
    let _ = std::io::stdout().flush();
    Err(anyhow::anyhow!("command {sv} not found"))
}

/// Removes the difference between "wasmer run {file} arg1 arg2" and "wasmer {file} arg1 arg2"
fn fixup_args(args: &[String], command: &str) -> Vec<String> {
    let mut args_without_package = args.to_vec();
    if args_without_package.get(1).map(|s| s.as_str()) == Some(command) {
        let _ = args_without_package.remove(1);
    } else if args_without_package.get(2).map(|s| s.as_str()) == Some(command) {
        let _ = args_without_package.remove(1);
        let _ = args_without_package.remove(1);
    }
    args_without_package
}

#[test]
fn test_fixup_args() {
    let first_args = vec![
        format!("wasmer"),
        format!("run"),
        format!("python/python"),
        format!("--arg1"),
        format!("--arg2"),
    ];

    let second_args = vec![
        format!("wasmer"), // no "run"
        format!("python/python"),
        format!("--arg1"),
        format!("--arg2"),
    ];

    let arg1_transformed = fixup_args(&first_args, "python/python");
    let arg2_transformed = fixup_args(&second_args, "python/python");

    assert_eq!(arg1_transformed, arg2_transformed);
}

pub(crate) fn try_run_package_or_file(
    args: &[String],
    r: &Run,
    debug: bool,
) -> Result<(), anyhow::Error> {
    let debug_msgs_allowed = isatty::stdout_isatty();

    // Check "r.path" is a file or a package / command name
    if r.path.exists() {
        if r.path.is_dir() && r.path.join("wapm.toml").exists() {
            let args_without_package = fixup_args(args, &format!("{}", r.path.display()));
            return RunWithoutFile::try_parse_from(args_without_package.iter())?
                .into_run_args(
                    r.path.clone(),
                    r.command_name.as_deref(),
                    debug_msgs_allowed,
                )?
                .execute();
        }
        return r.execute();
    }

    // c:// might be parsed as a URL on Windows
    let url_string = format!("{}", r.path.display());
    if let Ok(url) = url::Url::parse(&url_string) {
        if url.scheme() == "http" || url.scheme() == "https" {
            match try_run_url(&url, args, r, debug) {
                Err(ExecuteLocalPackageError::BeforeExec(_)) => {}
                Err(ExecuteLocalPackageError::DuringExec(e)) => return Err(e),
                Ok(o) => return Ok(o),
            }
        }
    }

    let package = format!("{}", r.path.display());

    let mut is_fake_sv = false;
    let mut sv = match SplitVersion::parse(&package) {
        Ok(o) => o,
        Err(_) => {
            let mut fake_sv = SplitVersion {
                original: package.to_string(),
                registry: None,
                package: package.to_string(),
                version: None,
                command: None,
            };
            is_fake_sv = true;
            match try_run_local_command(args, &fake_sv, debug) {
                Ok(()) => return Ok(()),
                Err(ExecuteLocalPackageError::DuringExec(e)) => return Err(e),
                _ => {}
            }
            match try_lookup_command(&mut fake_sv) {
                Ok(o) => SplitVersion {
                    original: package.to_string(),
                    registry: None,
                    package: o.package,
                    version: Some(o.version),
                    command: r.command_name.clone(),
                },
                Err(e) => {
                    return Err(
                        anyhow::anyhow!("No package for command {package:?} found, file {package:?} not found either")
                        .context(e)
                        .context(anyhow::anyhow!("{}", r.path.display()))
                    );
                }
            }
        }
    };

    if sv.command.is_none() {
        sv.command = r.command_name.clone();
    }

    if sv.command.is_none() && is_fake_sv {
        sv.command = Some(package);
    }

    let mut package_download_info = None;
    if !sv.package.contains('/') {
        if let Ok(o) = try_lookup_command(&mut sv) {
            package_download_info = Some(o);
        }
    }

    match try_execute_local_package(args, &sv, debug_msgs_allowed) {
        Ok(o) => return Ok(o),
        Err(ExecuteLocalPackageError::DuringExec(e)) => return Err(e),
        _ => {}
    }

    if debug && isatty::stdout_isatty() {
        eprintln!("finding local package {} failed", sv);
    }

    // else: local package not found - try to download and install package
    try_autoinstall_package(args, &sv, package_download_info, r.force_install)
}

fn try_run_url(
    url: &Url,
    _args: &[String],
    r: &Run,
    _debug: bool,
) -> Result<(), ExecuteLocalPackageError> {
    let checksum = wasmer_registry::get_remote_webc_checksum(url).map_err(|e| {
        ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!("error fetching {url}: {e}"))
    })?;

    let packages = wasmer_registry::get_all_installed_webc_packages();

    if !packages.iter().any(|p| p.checksum == checksum) {
        let sp = start_spinner(format!("Installing {}", url));

        let result = wasmer_registry::install_webc_package(url, &checksum);

        result.map_err(|e| {
            ExecuteLocalPackageError::BeforeExec(anyhow::anyhow!("error fetching {url}: {e}"))
        })?;

        if let Some(sp) = sp {
            sp.clear();
        }
    }

    let webc_dir = wasmer_registry::get_webc_dir();

    let webc_install_path = webc_dir
        .context("Error installing package: no webc dir")
        .map_err(ExecuteLocalPackageError::BeforeExec)?
        .join(checksum);

    let mut r = r.clone();
    r.path = webc_install_path;
    r.execute().map_err(ExecuteLocalPackageError::DuringExec)
}
