#![allow(missing_docs, unused)]

mod capabilities;
mod wasi;

use std::{
    collections::{hash_map::DefaultHasher, BTreeMap},
    fmt::{Binary, Display},
    fs::File,
    hash::{BuildHasherDefault, Hash, Hasher},
    io::{ErrorKind, LineWriter, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Error};
use clap::{Parser, ValueEnum};
use indicatif::{MultiProgress, ProgressBar};
use once_cell::sync::Lazy;
use tempfile::NamedTempFile;
use url::Url;
#[cfg(feature = "sys")]
use wasmer::sys::NativeEngineExt;
use wasmer::{
    AsStoreMut, DeserializeError, Engine, Function, Imports, Instance, Module, Store, Type,
    TypedFunction, Value,
};

use wasmer_types::{target::Target, Features};

#[cfg(feature = "compiler")]
use wasmer_compiler::ArtifactBuild;
use wasmer_config::package::PackageSource as PackageSpecifier;
use wasmer_package::utils::from_disk;
use wasmer_types::ModuleHash;

#[cfg(feature = "journal")]
use wasmer_wasix::journal::{LogFileJournal, SnapshotTrigger};
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    journal::CompactingLogFileJournal,
    runners::{
        dcgi::{DcgiInstanceFactory, DcgiRunner},
        dproxy::DProxyRunner,
        wasi::WasiRunner,
        wcgi::{self, AbortHandle, NoOpWcgiCallbacks, WcgiRunner},
        MappedCommand, MappedDirectory, Runner,
    },
    runtime::{
        module_cache::CacheError, package_loader::PackageLoader, resolver::QueryError,
        task_manager::VirtualTaskManagerExt,
    },
    Runtime, WasiError,
};
use webc::metadata::Manifest;
use webc::Container;

use crate::{
    backend::RuntimeOptions, commands::run::wasi::Wasi, common::HashAlgorithm, config::WasmerEnv,
    error::PrettyError, logging::Output,
};

const TICK: Duration = Duration::from_millis(250);

/// The unstable `wasmer run` subcommand.
#[derive(Debug, Parser)]
pub struct Run {
    #[clap(flatten)]
    env: WasmerEnv,
    #[clap(flatten)]
    rt: RuntimeOptions,
    #[clap(flatten)]
    wasi: crate::commands::run::Wasi,
    #[clap(flatten)]
    wcgi: WcgiOptions,
    /// Set the default stack size (default is 1048576)
    #[clap(long = "stack-size")]
    stack_size: Option<usize>,
    /// The entrypoint module for webc packages.
    #[clap(short, long, aliases = &["command", "command-name"])]
    entrypoint: Option<String>,
    /// The function to invoke.
    #[clap(short, long)]
    invoke: Option<String>,
    /// Generate a coredump at this path if a WebAssembly trap occurs
    #[clap(name = "COREDUMP_PATH", long)]
    coredump_on_trap: Option<PathBuf>,
    /// The file, URL, or package to run.
    #[clap(value_parser = PackageSource::infer)]
    input: PackageSource,
    /// Command-line arguments passed to the package
    args: Vec<String>,
    /// Hashing algorithm to be used for module hash
    #[clap(long, value_enum)]
    hash_algorithm: Option<HashAlgorithm>,
}

impl Run {
    pub fn execute(self, output: Output) -> ! {
        let result = self.execute_inner(output);
        exit_with_wasi_exit_code(result);
    }

    #[tracing::instrument(level = "debug", name = "wasmer_run", skip_all)]
    fn execute_inner(mut self, output: Output) -> Result<(), Error> {
        let pb = ProgressBar::new_spinner();
        pb.set_draw_target(output.draw_target());
        pb.enable_steady_tick(TICK);

        pb.set_message("Initializing the WebAssembly VM");

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let handle = runtime.handle().clone();

        // Check for the preferred webc version.
        // Default to v3.
        let webc_version_var = std::env::var("WASMER_WEBC_VERSION");
        let preferred_webc_version = match webc_version_var.as_deref() {
            Ok("2") => webc::Version::V2,
            Ok("3") | Err(_) => webc::Version::V3,
            Ok(other) => {
                bail!("unknown webc version: '{other}'");
            }
        };

        let _guard = handle.enter();

        // Get the input file path
        let mut wasm_bytes: Option<Vec<u8>> = None;

        // Try to detect WebAssembly features before selecting a backend
        tracing::info!("Input source: {:?}", self.input);
        if let PackageSource::File(path) = &self.input {
            tracing::info!("Input file path: {}", path.display());

            // Try to read and detect any file that exists, regardless of extension
            if path.exists() {
                tracing::info!("Found file: {}", path.display());
                match std::fs::read(path) {
                    Ok(bytes) => {
                        tracing::info!("Read {} bytes from file", bytes.len());

                        // Check if it's a WebAssembly module by looking for magic bytes
                        let magic = [0x00, 0x61, 0x73, 0x6D]; // "\0asm"
                        if bytes.len() >= 4 && bytes[0..4] == magic {
                            // Looks like a valid WebAssembly module, save the bytes for feature detection
                            tracing::info!(
                                "Valid WebAssembly module detected, magic header verified"
                            );
                            wasm_bytes = Some(bytes);
                        } else {
                            tracing::info!("File does not have valid WebAssembly magic number, will try to run it anyway");
                            // Still provide the bytes so the engine can attempt to run it
                            wasm_bytes = Some(bytes);
                        }
                    }
                    Err(e) => {
                        tracing::info!("Failed to read file for feature detection: {}", e);
                    }
                }
            } else {
                tracing::info!("File does not exist: {}", path.display());
            }
        } else {
            tracing::info!("Input is not a file, skipping WebAssembly feature detection");
        }

        // Get engine with feature-based backend selection if possible
        let mut engine = match &wasm_bytes {
            Some(wasm_bytes) => {
                tracing::info!("Attempting to detect WebAssembly features from binary");

                self.rt
                    .get_engine_for_module(wasm_bytes, &Target::default())?
            }
            None => {
                // No WebAssembly file available for analysis, check if we have a webc package
                if let PackageSource::Package(ref pkg_source) = &self.input {
                    tracing::info!("Checking package for WebAssembly features: {}", pkg_source);
                    self.rt.get_engine(&Target::default())?
                } else {
                    tracing::info!("No feature detection possible, using default engine");
                    self.rt.get_engine(&Target::default())?
                }
            }
        };

        let engine_kind = engine.deterministic_id();
        tracing::info!("Executing on backend {engine_kind:?}");

        #[cfg(feature = "sys")]
        if engine.is_sys() {
            if self.stack_size.is_some() {
                wasmer_vm::set_stack_size(self.stack_size.unwrap());
            }
            let hash_algorithm = self.hash_algorithm.unwrap_or_default().into();
            engine.set_hash_algorithm(Some(hash_algorithm));
        }

        let engine = engine.clone();

        let runtime = self.wasi.prepare_runtime(
            engine,
            &self.env,
            &capabilities::get_capability_cache_path(&self.env, &self.input)?,
            runtime,
            preferred_webc_version,
        )?;

        // This is a slow operation, so let's temporarily wrap the runtime with
        // something that displays progress
        let monitoring_runtime = Arc::new(MonitoringRuntime::new(runtime, pb.clone()));
        let runtime: Arc<dyn Runtime + Send + Sync> = monitoring_runtime.runtime.clone();
        let monitoring_runtime: Arc<dyn Runtime + Send + Sync> = monitoring_runtime;

        let target = self.input.resolve_target(&monitoring_runtime, &pb)?;

        if let ExecutableTarget::Package(ref pkg) = target {
            self.wasi
                .mapped_dirs
                .extend(pkg.additional_host_mapped_directories.clone());
        }

        pb.finish_and_clear();

        // push the TTY state so we can restore it after the program finishes
        let tty = runtime.tty().map(|tty| tty.tty_get());

        let result = {
            match target {
                ExecutableTarget::WebAssembly {
                    module,
                    module_hash,
                    path,
                } => self.execute_wasm(&path, module, module_hash, runtime.clone()),
                ExecutableTarget::Package(pkg) => {
                    // Check if we should update the engine based on the WebC package features
                    if let Some(cmd) = pkg.get_entrypoint_command() {
                        if let Some(features) = cmd.wasm_features() {
                            // Get the right engine for these features
                            let backends = self.rt.get_available_backends()?;
                            let available_engines = backends
                                .iter()
                                .map(|b| b.to_string())
                                .collect::<Vec<_>>()
                                .join(", ");

                            let filtered_backends = RuntimeOptions::filter_backends_by_features(
                                backends.clone(),
                                &features,
                                &Target::default(),
                            );

                            if !filtered_backends.is_empty() {
                                let engine_id = filtered_backends[0].to_string();

                                // Get a new engine that's compatible with the required features
                                if let Ok(new_engine) = filtered_backends[0].get_engine(
                                    &Target::default(),
                                    &features,
                                    &self.rt,
                                ) {
                                    tracing::info!(
                                        "The command '{}' requires to run the Wasm module with the features {:?}. The backends available are {}. Choosing {}.",
                                        cmd.name(),
                                        features,
                                        available_engines,
                                        engine_id
                                    );
                                    // Create a new runtime with the updated engine
                                    let new_runtime = self.wasi.prepare_runtime(
                                        new_engine,
                                        &self.env,
                                        &capabilities::get_capability_cache_path(
                                            &self.env,
                                            &self.input,
                                        )?,
                                        tokio::runtime::Builder::new_multi_thread()
                                            .enable_all()
                                            .build()?,
                                        preferred_webc_version,
                                    )?;

                                    let new_runtime =
                                        Arc::new(MonitoringRuntime::new(new_runtime, pb.clone()));
                                    return self.execute_webc(&pkg, new_runtime);
                                }
                            }
                        }
                    }
                    self.execute_webc(&pkg, runtime.clone())
                }
            }
        };

        // restore the TTY state as the execution may have changed it
        if let Some(state) = tty {
            if let Some(tty) = runtime.tty() {
                tty.tty_set(state);
            }
        }

        if let Err(e) = &result {
            self.maybe_save_coredump(e);
        }

        result
    }

    #[tracing::instrument(skip_all)]
    fn execute_wasm(
        &self,
        path: &Path,
        module: Module,
        module_hash: ModuleHash,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        if wasmer_wasix::is_wasi_module(&module) || wasmer_wasix::is_wasix_module(&module) {
            self.execute_wasi_module(path, module, module_hash, runtime)
        } else {
            self.execute_pure_wasm_module(&module)
        }
    }

    #[tracing::instrument(skip_all)]
    fn execute_webc(
        &self,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let id = match self.entrypoint.as_deref() {
            Some(cmd) => cmd,
            None => pkg.infer_entrypoint()?,
        };
        let cmd = pkg
            .get_command(id)
            .with_context(|| format!("Unable to get metadata for the \"{id}\" command"))?;

        let uses = self.load_injected_packages(&runtime)?;

        if DcgiRunner::can_run_command(cmd.metadata())? {
            self.run_dcgi(id, pkg, uses, runtime)
        } else if DProxyRunner::can_run_command(cmd.metadata())? {
            self.run_dproxy(id, pkg, runtime)
        } else if WcgiRunner::can_run_command(cmd.metadata())? {
            self.run_wcgi(id, pkg, uses, runtime)
        } else if WasiRunner::can_run_command(cmd.metadata())? {
            self.run_wasi(id, pkg, uses, runtime)
        } else {
            bail!(
                "Unable to find a runner that supports \"{}\"",
                cmd.metadata().runner
            );
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn load_injected_packages(
        &self,
        runtime: &Arc<dyn Runtime + Send + Sync>,
    ) -> Result<Vec<BinaryPackage>, Error> {
        let mut dependencies = Vec::new();

        for name in &self.wasi.uses {
            let specifier = PackageSpecifier::from_str(name)
                .with_context(|| format!("Unable to parse \"{name}\" as a package specifier"))?;
            let pkg = {
                let specifier = specifier.clone();
                let inner_runtime = runtime.clone();
                runtime
                    .task_manager()
                    .spawn_and_block_on(async move {
                        BinaryPackage::from_registry(&specifier, inner_runtime.as_ref()).await
                    })
                    .with_context(|| format!("Unable to load \"{name}\""))??
            };
            dependencies.push(pkg);
        }

        Ok(dependencies)
    }

    fn run_wasi(
        &self,
        command_name: &str,
        pkg: &BinaryPackage,
        uses: Vec<BinaryPackage>,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let mut runner = self.build_wasi_runner(&runtime)?;
        runner.run_command(command_name, pkg, runtime)
    }

    fn run_wcgi(
        &self,
        command_name: &str,
        pkg: &BinaryPackage,
        uses: Vec<BinaryPackage>,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let mut runner = wasmer_wasix::runners::wcgi::WcgiRunner::new(NoOpWcgiCallbacks);
        self.config_wcgi(runner.config(), uses)?;
        runner.run_command(command_name, pkg, runtime)
    }

    fn config_wcgi(
        &self,
        config: &mut wcgi::Config,
        uses: Vec<BinaryPackage>,
    ) -> Result<(), Error> {
        config
            .args(self.args.clone())
            .addr(self.wcgi.addr)
            .envs(self.wasi.env_vars.clone())
            .map_directories(self.wasi.mapped_dirs.clone())
            .callbacks(Callbacks::new(self.wcgi.addr))
            .inject_packages(uses);
        *config.capabilities() = self.wasi.capabilities();
        if self.wasi.forward_host_env {
            config.forward_host_env();
        }

        #[cfg(feature = "journal")]
        {
            for trigger in self.wasi.snapshot_on.iter().cloned() {
                config.add_snapshot_trigger(trigger);
            }
            if self.wasi.snapshot_on.is_empty() && !self.wasi.writable_journals.is_empty() {
                config.add_default_snapshot_triggers();
            }
            if let Some(period) = self.wasi.snapshot_interval {
                if self.wasi.writable_journals.is_empty() {
                    return Err(anyhow::format_err!(
                        "If you specify a snapshot interval then you must also specify a writable journal file"
                    ));
                }
                config.with_snapshot_interval(Duration::from_millis(period));
            }
            if self.wasi.stop_after_snapshot {
                config.with_stop_running_after_snapshot(true);
            }
            let (r, w) = self.wasi.build_journals()?;
            for journal in r {
                config.add_read_only_journal(journal);
            }
            for journal in w {
                config.add_writable_journal(journal);
            }
        }

        Ok(())
    }

    fn run_dcgi(
        &self,
        command_name: &str,
        pkg: &BinaryPackage,
        uses: Vec<BinaryPackage>,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let factory = DcgiInstanceFactory::new();
        let mut runner = wasmer_wasix::runners::dcgi::DcgiRunner::new(factory);
        self.config_wcgi(runner.config().inner(), uses);
        runner.run_command(command_name, pkg, runtime)
    }

    fn run_dproxy(
        &self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let mut inner = self.build_wasi_runner(&runtime)?;
        let mut runner = wasmer_wasix::runners::dproxy::DProxyRunner::new(inner, pkg);
        runner.run_command(command_name, pkg, runtime)
    }

    #[tracing::instrument(skip_all)]
    fn execute_pure_wasm_module(&self, module: &Module) -> Result<(), Error> {
        /// The rest of the execution happens in the main thread, so we can create the
        /// store here.
        let mut store = self.rt.get_store()?;
        let imports = Imports::default();
        let instance = Instance::new(&mut store, module, &imports)
            .context("Unable to instantiate the WebAssembly module")?;

        let entry_function  = match &self.invoke {
            Some(entry) => {
                instance.exports
                    .get_function(entry)
                    .with_context(|| format!("The module doesn't export a function named \"{entry}\""))?
            },
            None => {
                instance.exports.get_function("_start")
                    .context("The module doesn't export a \"_start\" function. Either implement it or specify an entry function with --invoke")?
            }
        };

        let return_values = invoke_function(&instance, &mut store, entry_function, &self.args)?;

        println!(
            "{}",
            return_values
                .iter()
                .map(|val| val.to_string())
                .collect::<Vec<String>>()
                .join(" ")
        );

        Ok(())
    }

    fn build_wasi_runner(
        &self,
        runtime: &Arc<dyn Runtime + Send + Sync>,
    ) -> Result<WasiRunner, anyhow::Error> {
        let packages = self.load_injected_packages(runtime)?;

        let mut runner = WasiRunner::new();

        let (is_home_mapped, is_tmp_mapped, mapped_diretories) =
            self.wasi.build_mapped_directories()?;

        runner
            .with_args(&self.args)
            .with_injected_packages(packages)
            .with_envs(self.wasi.env_vars.clone())
            .with_mapped_host_commands(self.wasi.build_mapped_commands()?)
            .with_mapped_directories(mapped_diretories)
            .with_home_mapped(is_home_mapped)
            .with_tmp_mapped(is_tmp_mapped)
            .with_forward_host_env(self.wasi.forward_host_env)
            .with_capabilities(self.wasi.capabilities());

        if let Some(ref entry_function) = self.invoke {
            runner.with_entry_function(entry_function);
        }

        #[cfg(feature = "journal")]
        {
            for trigger in self.wasi.snapshot_on.iter().cloned() {
                runner.with_snapshot_trigger(trigger);
            }
            if self.wasi.snapshot_on.is_empty() && !self.wasi.writable_journals.is_empty() {
                runner.with_default_snapshot_triggers();
            }
            if let Some(period) = self.wasi.snapshot_interval {
                if self.wasi.writable_journals.is_empty() {
                    return Err(anyhow::format_err!(
                        "If you specify a snapshot interval then you must also specify a writable journal file"
                    ));
                }
                runner.with_snapshot_interval(Duration::from_millis(period));
            }
            if self.wasi.stop_after_snapshot {
                runner.with_stop_running_after_snapshot(true);
            }
            let (r, w) = self.wasi.build_journals()?;
            for journal in r {
                runner.with_read_only_journal(journal);
            }
            for journal in w {
                runner.with_writable_journal(journal);
            }
            runner.with_skip_stdio_during_bootstrap(self.wasi.skip_stdio_during_bootstrap);
        }

        Ok(runner)
    }

    #[tracing::instrument(skip_all)]
    fn execute_wasi_module(
        &self,
        wasm_path: &Path,
        module: Module,
        module_hash: ModuleHash,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let program_name = wasm_path.display().to_string();

        let runner = self.build_wasi_runner(&runtime)?;
        runner.run_wasm(runtime, &program_name, module, module_hash)
    }

    #[allow(unused_variables)]
    fn maybe_save_coredump(&self, e: &Error) {
        #[cfg(feature = "coredump")]
        if let Some(coredump) = &self.coredump_on_trap {
            if let Err(e) = generate_coredump(e, self.input.to_string(), coredump) {
                tracing::warn!(
                    error = &*e as &dyn std::error::Error,
                    coredump_path=%coredump.display(),
                    "Unable to generate a coredump",
                );
            }
        }
    }

    /// Create Run instance for arguments/env, assuming we're being run from a
    /// CFP binfmt interpreter.
    pub fn from_binfmt_args() -> Self {
        Run::from_binfmt_args_fallible().unwrap_or_else(|e| {
            crate::error::PrettyError::report::<()>(
                Err(e).context("Failed to set up wasmer binfmt invocation"),
            )
        })
    }

    fn from_binfmt_args_fallible() -> Result<Self, Error> {
        if cfg!(not(target_os = "linux")) {
            bail!("binfmt_misc is only available on linux.");
        }

        let argv = std::env::args().collect::<Vec<_>>();
        let (_interpreter, executable, original_executable, args) = match &argv[..] {
            [a, b, c, rest @ ..] => (a, b, c, rest),
            _ => {
                bail!("Wasmer binfmt interpreter needs at least three arguments (including $0) - must be registered as binfmt interpreter with the CFP flags. (Got arguments: {:?})", argv);
            }
        };
        let rt = RuntimeOptions::default();
        Ok(Run {
            env: WasmerEnv::default(),
            rt,
            wasi: Wasi::for_binfmt_interpreter()?,
            wcgi: WcgiOptions::default(),
            stack_size: None,
            entrypoint: Some(original_executable.to_string()),
            invoke: None,
            coredump_on_trap: None,
            input: PackageSource::infer(executable)?,
            args: args.to_vec(),
            hash_algorithm: None,
        })
    }
}

fn invoke_function(
    instance: &Instance,
    store: &mut Store,
    func: &Function,
    args: &[String],
) -> Result<Box<[Value]>, Error> {
    let func_ty = func.ty(store);
    let required_arguments = func_ty.params().len();
    let provided_arguments = args.len();

    anyhow::ensure!(
        required_arguments == provided_arguments,
        "Function expected {} arguments, but received {}",
        required_arguments,
        provided_arguments,
    );

    let invoke_args = args
        .iter()
        .zip(func_ty.params().iter())
        .map(|(arg, param_type)| {
            parse_value(arg, *param_type)
                .with_context(|| format!("Unable to convert {arg:?} to {param_type:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let return_values = func.call(store, &invoke_args)?;

    Ok(return_values)
}

fn parse_value(s: &str, ty: wasmer_types::Type) -> Result<Value, Error> {
    let value = match ty {
        Type::I32 => Value::I32(s.parse()?),
        Type::I64 => Value::I64(s.parse()?),
        Type::F32 => Value::F32(s.parse()?),
        Type::F64 => Value::F64(s.parse()?),
        Type::V128 => Value::V128(s.parse()?),
        _ => bail!("There is no known conversion from {s:?} to {ty:?}"),
    };
    Ok(value)
}

/// The input that was passed in via the command-line.
#[derive(Debug, Clone, PartialEq)]
enum PackageSource {
    /// A file on disk (`*.wasm`, `*.webc`, etc.).
    File(PathBuf),
    /// A directory containing a `wasmer.toml` file
    Dir(PathBuf),
    /// A package to be downloaded (a URL, package name, etc.)
    Package(PackageSpecifier),
}

impl PackageSource {
    fn infer(s: &str) -> Result<PackageSource, Error> {
        let path = Path::new(s);
        if path.is_file() {
            return Ok(PackageSource::File(path.to_path_buf()));
        } else if path.is_dir() {
            return Ok(PackageSource::Dir(path.to_path_buf()));
        }

        if let Ok(pkg) = PackageSpecifier::from_str(s) {
            return Ok(PackageSource::Package(pkg));
        }

        Err(anyhow::anyhow!(
            "Unable to resolve \"{s}\" as a URL, package name, or file on disk"
        ))
    }

    /// Try to resolve the [`PackageSource`] to an executable artifact.
    ///
    /// This will try to automatically download and cache any resources from the
    /// internet.
    #[tracing::instrument(level = "debug", skip_all)]
    fn resolve_target(
        &self,
        rt: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<ExecutableTarget, Error> {
        match self {
            PackageSource::File(path) => ExecutableTarget::from_file(path, rt, pb),
            PackageSource::Dir(d) => ExecutableTarget::from_dir(d, rt, pb),
            PackageSource::Package(pkg) => {
                pb.set_message("Loading from the registry");
                let inner_pck = pkg.clone();
                let inner_rt = rt.clone();
                let pkg = rt.task_manager().spawn_and_block_on(async move {
                    BinaryPackage::from_registry(&inner_pck, inner_rt.as_ref()).await
                })??;
                Ok(ExecutableTarget::Package(pkg))
            }
        }
    }
}

impl Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::File(path) | PackageSource::Dir(path) => write!(f, "{}", path.display()),
            PackageSource::Package(p) => write!(f, "{p}"),
        }
    }
}

/// We've been given the path for a file... What does it contain and how should
/// that be run?
#[derive(Debug, Clone)]
enum TargetOnDisk {
    WebAssemblyBinary,
    Wat,
    LocalWebc,
    Artifact,
}

impl TargetOnDisk {
    fn from_file(path: &Path) -> Result<TargetOnDisk, Error> {
        // Normally the first couple hundred bytes is enough to figure
        // out what type of file this is.
        let mut buffer = [0_u8; 512];

        let mut f = File::open(path)
            .with_context(|| format!("Unable to open \"{}\" for reading", path.display()))?;
        let bytes_read = f.read(&mut buffer)?;

        let leading_bytes = &buffer[..bytes_read];

        if wasmer::is_wasm(leading_bytes) {
            return Ok(TargetOnDisk::WebAssemblyBinary);
        }

        if webc::detect(leading_bytes).is_ok() {
            return Ok(TargetOnDisk::LocalWebc);
        }

        #[cfg(feature = "compiler")]
        if ArtifactBuild::is_deserializable(leading_bytes) {
            return Ok(TargetOnDisk::Artifact);
        }

        // If we can't figure out the file type based on its content, fall back
        // to checking the extension.

        match path.extension().and_then(|s| s.to_str()) {
            Some("wat") => Ok(TargetOnDisk::Wat),
            Some("wasm") => Ok(TargetOnDisk::WebAssemblyBinary),
            Some("webc") => Ok(TargetOnDisk::LocalWebc),
            Some("wasmu") => Ok(TargetOnDisk::WebAssemblyBinary),
            _ => bail!("Unable to determine how to execute \"{}\"", path.display()),
        }
    }
}

#[derive(Debug, Clone)]
enum ExecutableTarget {
    WebAssembly {
        module: Module,
        module_hash: ModuleHash,
        path: PathBuf,
    },
    Package(BinaryPackage),
}

impl ExecutableTarget {
    /// Try to load a Wasmer package from a directory containing a `wasmer.toml`
    /// file.
    #[tracing::instrument(level = "debug", skip_all)]
    fn from_dir(
        dir: &Path,
        runtime: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<Self, Error> {
        pb.set_message(format!("Loading \"{}\" into memory", dir.display()));
        pb.set_message("Resolving dependencies");
        let inner_runtime = runtime.clone();
        let pkg = runtime.task_manager().spawn_and_block_on({
            let path = dir.to_path_buf();

            async move { BinaryPackage::from_dir(&path, inner_runtime.as_ref()).await }
        })??;

        Ok(ExecutableTarget::Package(pkg))
    }

    /// Try to load a file into something that can be used to run it.
    #[tracing::instrument(level = "debug", skip_all)]
    fn from_file(
        path: &Path,
        runtime: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<Self, Error> {
        pb.set_message(format!("Loading from \"{}\"", path.display()));

        match TargetOnDisk::from_file(path)? {
            TargetOnDisk::WebAssemblyBinary | TargetOnDisk::Wat => {
                let wasm = std::fs::read(path)?;

                pb.set_message("Compiling to WebAssembly");
                let module = runtime
                    .load_module_sync(&wasm)
                    .with_context(|| format!("Unable to compile \"{}\"", path.display()))?;

                Ok(ExecutableTarget::WebAssembly {
                    module,
                    module_hash: ModuleHash::xxhash(&wasm),
                    path: path.to_path_buf(),
                })
            }
            TargetOnDisk::Artifact => {
                let engine = runtime.engine();
                pb.set_message("Deserializing pre-compiled WebAssembly module");
                let module = unsafe { Module::deserialize_from_file(&engine, path)? };

                let module_hash = module.info().hash.ok_or_else(|| {
                    anyhow::Error::msg("module hash is not present in the artifact")
                })?;

                Ok(ExecutableTarget::WebAssembly {
                    module,
                    module_hash,
                    path: path.to_path_buf(),
                })
            }
            TargetOnDisk::LocalWebc => {
                let container = from_disk(path)?;
                pb.set_message("Resolving dependencies");

                let inner_runtime = runtime.clone();
                let pkg = runtime.task_manager().spawn_and_block_on(async move {
                    BinaryPackage::from_webc(&container, inner_runtime.as_ref()).await
                })??;
                Ok(ExecutableTarget::Package(pkg))
            }
        }
    }
}

#[cfg(feature = "coredump")]
fn generate_coredump(err: &Error, source_name: String, coredump_path: &Path) -> Result<(), Error> {
    let err: &wasmer::RuntimeError = match err.downcast_ref() {
        Some(e) => e,
        None => {
            log::warn!("no runtime error found to generate coredump with");
            return Ok(());
        }
    };

    let mut coredump_builder =
        wasm_coredump_builder::CoredumpBuilder::new().executable_name(&source_name);

    let mut thread_builder = wasm_coredump_builder::ThreadBuilder::new().thread_name("main");

    for frame in err.trace() {
        let coredump_frame = wasm_coredump_builder::FrameBuilder::new()
            .codeoffset(frame.func_offset() as u32)
            .funcidx(frame.func_index())
            .build();
        thread_builder.add_frame(coredump_frame);
    }

    coredump_builder.add_thread(thread_builder.build());

    let coredump = coredump_builder
        .serialize()
        .map_err(Error::msg)
        .context("Coredump serializing failed")?;

    std::fs::write(coredump_path, &coredump).with_context(|| {
        format!(
            "Unable to save the coredump to \"{}\"",
            coredump_path.display()
        )
    })?;

    Ok(())
}

#[derive(Debug, Clone, Parser)]
pub(crate) struct WcgiOptions {
    /// The address to serve on.
    #[clap(long, short, env, default_value_t = ([127, 0, 0, 1], 8000).into())]
    pub(crate) addr: SocketAddr,
}

impl Default for WcgiOptions {
    fn default() -> Self {
        Self {
            addr: ([127, 0, 0, 1], 8000).into(),
        }
    }
}

#[derive(Debug)]
struct Callbacks {
    stderr: Mutex<LineWriter<std::io::Stderr>>,
    addr: SocketAddr,
}

impl Callbacks {
    fn new(addr: SocketAddr) -> Self {
        Callbacks {
            stderr: Mutex::new(LineWriter::new(std::io::stderr())),
            addr,
        }
    }
}

impl wasmer_wasix::runners::wcgi::Callbacks for Callbacks {
    fn started(&self, _abort: AbortHandle) {
        println!("WCGI Server running at http://{}/", self.addr);
    }

    fn on_stderr(&self, raw_message: &[u8]) {
        if let Ok(mut stderr) = self.stderr.lock() {
            // If the WCGI runner printed any log messages we want to make sure
            // they get propagated to the user. Line buffering is important here
            // because it helps prevent the output from becoming a complete
            // mess.
            let _ = stderr.write_all(raw_message);
        }
    }
}

/// Exit the current process, using the WASI exit code if the error contains
/// one.
fn exit_with_wasi_exit_code(result: Result<(), Error>) -> ! {
    let exit_code = match result {
        Ok(_) => 0,
        Err(error) => {
            match error.chain().find_map(get_exit_code) {
                Some(exit_code) => exit_code.raw(),
                None => {
                    eprintln!("{:?}", PrettyError::new(error));
                    // Something else happened
                    1
                }
            }
        }
    };

    std::io::stdout().flush().ok();
    std::io::stderr().flush().ok();

    std::process::exit(exit_code);
}

fn get_exit_code(
    error: &(dyn std::error::Error + 'static),
) -> Option<wasmer_wasix::types::wasi::ExitCode> {
    if let Some(WasiError::Exit(exit_code)) = error.downcast_ref() {
        return Some(*exit_code);
    }
    if let Some(error) = error.downcast_ref::<wasmer_wasix::WasiRuntimeError>() {
        return error.as_exit_code();
    }

    None
}

#[derive(Debug)]
struct MonitoringRuntime<R> {
    runtime: Arc<R>,
    progress: ProgressBar,
}

impl<R> MonitoringRuntime<R> {
    fn new(runtime: R, progress: ProgressBar) -> Self {
        MonitoringRuntime {
            runtime: Arc::new(runtime),
            progress,
        }
    }
}

impl<R: wasmer_wasix::Runtime + Send + Sync> wasmer_wasix::Runtime for MonitoringRuntime<R> {
    fn networking(&self) -> &virtual_net::DynVirtualNetworking {
        self.runtime.networking()
    }

    fn task_manager(&self) -> &Arc<dyn wasmer_wasix::VirtualTaskManager> {
        self.runtime.task_manager()
    }

    fn package_loader(
        &self,
    ) -> Arc<dyn wasmer_wasix::runtime::package_loader::PackageLoader + Send + Sync> {
        let inner = self.runtime.package_loader();
        Arc::new(MonitoringPackageLoader {
            inner,
            progress: self.progress.clone(),
        })
    }

    fn module_cache(
        &self,
    ) -> Arc<dyn wasmer_wasix::runtime::module_cache::ModuleCache + Send + Sync> {
        self.runtime.module_cache()
    }

    fn source(&self) -> Arc<dyn wasmer_wasix::runtime::resolver::Source + Send + Sync> {
        let inner = self.runtime.source();
        Arc::new(MonitoringSource {
            inner,
            progress: self.progress.clone(),
        })
    }

    fn engine(&self) -> wasmer::Engine {
        self.runtime.engine()
    }

    fn new_store(&self) -> wasmer::Store {
        self.runtime.new_store()
    }

    fn http_client(&self) -> Option<&wasmer_wasix::http::DynHttpClient> {
        self.runtime.http_client()
    }

    fn tty(&self) -> Option<&(dyn wasmer_wasix::os::TtyBridge + Send + Sync)> {
        self.runtime.tty()
    }

    #[cfg(feature = "journal")]
    fn read_only_journals<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Arc<wasmer_wasix::journal::DynReadableJournal>> + 'a> {
        self.runtime.read_only_journals()
    }

    #[cfg(feature = "journal")]
    fn writable_journals<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Arc<wasmer_wasix::journal::DynJournal>> + 'a> {
        self.runtime.writable_journals()
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ wasmer_wasix::journal::DynJournal> {
        self.runtime.active_journal()
    }
}

#[derive(Debug)]
struct MonitoringSource {
    inner: Arc<dyn wasmer_wasix::runtime::resolver::Source + Send + Sync>,
    progress: ProgressBar,
}

#[async_trait::async_trait]
impl wasmer_wasix::runtime::resolver::Source for MonitoringSource {
    async fn query(
        &self,
        package: &PackageSpecifier,
    ) -> Result<Vec<wasmer_wasix::runtime::resolver::PackageSummary>, QueryError> {
        self.progress.set_message(format!("Looking up {package}"));
        self.inner.query(package).await
    }
}

#[derive(Debug)]
struct MonitoringPackageLoader {
    inner: Arc<dyn wasmer_wasix::runtime::package_loader::PackageLoader + Send + Sync>,
    progress: ProgressBar,
}

#[async_trait::async_trait]
impl wasmer_wasix::runtime::package_loader::PackageLoader for MonitoringPackageLoader {
    async fn load(
        &self,
        summary: &wasmer_wasix::runtime::resolver::PackageSummary,
    ) -> Result<Container, Error> {
        let pkg_id = summary.package_id();
        self.progress.set_message(format!("Downloading {pkg_id}"));

        self.inner.load(summary).await
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &wasmer_wasix::runtime::resolver::Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        self.inner
            .load_package_tree(root, resolution, root_is_local_dir)
            .await
    }
}
