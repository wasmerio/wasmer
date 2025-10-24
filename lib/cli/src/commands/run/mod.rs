#![allow(missing_docs, unused)]

mod capabilities;
mod package_source;
mod runtime;
mod target;
mod wasi;

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
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

use anyhow::{Context, Error, anyhow, bail};
use clap::{Parser, ValueEnum};
use futures::future::BoxFuture;
use indicatif::{MultiProgress, ProgressBar};
use is_terminal::IsTerminal as _;
use once_cell::sync::Lazy;
use tempfile::NamedTempFile;
use url::Url;
#[cfg(feature = "sys")]
use wasmer::sys::NativeEngineExt;
use wasmer::{
    AsStoreMut, DeserializeError, Engine, Function, Imports, Instance, Module, RuntimeError, Store,
    Type, TypedFunction, Value,
};

use wasmer_types::{Features, target::Target};

#[cfg(feature = "compiler")]
use wasmer_compiler::ArtifactBuild;
use wasmer_config::package::PackageSource;
use wasmer_package::utils::from_disk;
use wasmer_types::ModuleHash;

#[cfg(feature = "journal")]
use wasmer_wasix::journal::{LogFileJournal, SnapshotTrigger};
use wasmer_wasix::{
    Runtime, SpawnError, WasiError,
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    journal::CompactingLogFileJournal,
    runners::{
        MappedCommand, MappedDirectory, Runner,
        dcgi::{DcgiInstanceFactory, DcgiRunner},
        dproxy::DProxyRunner,
        wasi::{RuntimeOrEngine, WasiRunner},
        wcgi::{self, AbortHandle, NoOpWcgiCallbacks, WcgiRunner},
    },
    runtime::{
        module_cache::{CacheError, HashedModuleData},
        package_loader::PackageLoader,
        resolver::QueryError,
        task_manager::VirtualTaskManagerExt,
    },
};
use webc::Container;
use webc::metadata::Manifest;

use crate::{
    backend::RuntimeOptions, commands::run::wasi::Wasi, common::HashAlgorithm, config::WasmerEnv,
    error::PrettyError, logging::Output,
};

use self::{
    package_source::CliPackageSource, runtime::MonitoringRuntime, target::ExecutableTarget,
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
    #[clap(value_parser = CliPackageSource::infer)]
    input: CliPackageSource,
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
        if let CliPackageSource::File(path) = &self.input {
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
                            tracing::info!(
                                "File does not have valid WebAssembly magic number, will try to run it anyway"
                            );
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
                if let CliPackageSource::Package(pkg_source) = &self.input {
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
        let monitoring_runtime =
            Arc::new(MonitoringRuntime::new(runtime, pb.clone(), output.quiet));
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
                    if let Some(cmd) = pkg.get_entrypoint_command()
                        && let Some(features) = cmd.wasm_features()
                    {
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
                                let capability_cache_path =
                                    capabilities::get_capability_cache_path(
                                        &self.env,
                                        &self.input,
                                    )?;
                                let new_runtime = self.wasi.prepare_runtime(
                                    new_engine,
                                    &self.env,
                                    &capability_cache_path,
                                    tokio::runtime::Builder::new_multi_thread()
                                        .enable_all()
                                        .build()?,
                                    preferred_webc_version,
                                )?;

                                let new_runtime = Arc::new(MonitoringRuntime::new(
                                    new_runtime,
                                    pb.clone(),
                                    output.quiet,
                                ));
                                return self.execute_webc(&pkg, new_runtime);
                            }
                        }
                    }
                    self.execute_webc(&pkg, runtime.clone())
                }
            }
        };

        // restore the TTY state as the execution may have changed it
        if let Some(state) = tty
            && let Some(tty) = runtime.tty()
        {
            tty.tty_set(state);
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
            let specifier = name
                .parse::<PackageSource>()
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
        Runner::run_command(&mut runner, command_name, pkg, runtime)
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

        let result = invoke_function(&instance, &mut store, entry_function, &self.args)?;

        match result {
            Ok(return_values) => {
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
            Err(err) => {
                bail!("{}", err.display(&mut store));
            }
        }
    }

    fn build_wasi_runner(
        &self,
        runtime: &Arc<dyn Runtime + Send + Sync>,
    ) -> Result<WasiRunner, anyhow::Error> {
        let packages = self.load_injected_packages(runtime)?;

        let mut runner = WasiRunner::new();

        let (is_home_mapped, mapped_diretories) = self.wasi.build_mapped_directories()?;

        runner
            .with_args(&self.args)
            .with_injected_packages(packages)
            .with_envs(self.wasi.env_vars.clone())
            .with_mapped_host_commands(self.wasi.build_mapped_commands()?)
            .with_mapped_directories(mapped_diretories)
            .with_home_mapped(is_home_mapped)
            .with_forward_host_env(self.wasi.forward_host_env)
            .with_capabilities(self.wasi.capabilities());

        if let Some(cwd) = self.wasi.cwd.as_ref() {
            if !cwd.starts_with("/") {
                bail!("The argument to --cwd must be an absolute path");
            }
            runner.with_current_dir(cwd.clone());
        }

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
        runner.run_wasm(
            RuntimeOrEngine::Runtime(runtime),
            &program_name,
            module,
            module_hash,
        )
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
}

fn invoke_function(
    instance: &Instance,
    store: &mut Store,
    func: &Function,
    args: &[String],
) -> anyhow::Result<Result<Box<[Value]>, RuntimeError>> {
    let func_ty = func.ty(store);
    let required_arguments = func_ty.params().len();
    let provided_arguments = args.len();

    anyhow::ensure!(
        required_arguments == provided_arguments,
        "Function expected {required_arguments} arguments, but received {provided_arguments}"
    );

    let invoke_args = args
        .iter()
        .zip(func_ty.params().iter())
        .map(|(arg, param_type)| {
            parse_value(arg, *param_type)
                .with_context(|| format!("Unable to convert {arg:?} to {param_type:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(func.call(store, &invoke_args))
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
