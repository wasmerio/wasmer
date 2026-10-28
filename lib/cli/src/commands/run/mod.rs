mod machine;
mod package_source;
mod runtime;
mod target;
mod wasi;

use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Error, bail};
use clap::Parser;
use colored::Colorize;
use indicatif::ProgressBar;
use wasixv2::{Image, Loader as _};
use wasmer::{Function, Imports, Instance, Module, RuntimeError, Store, Type, Value, wat2wasm};
use wasmer_types::target::Target;
use wasmer_wasix::{Runtime, runtime::package_loader::builtin_loader::FileSystemCache};

use crate::{
    backend::RuntimeOptions, commands::run::target::TargetOnDisk, config::WasmerEnv,
    error::PrettyError, logging::Output,
};

use self::{
    machine::{CachedLoader, MachineProgram, runs_on_machine, wasm_runs_on_machine},
    package_source::CliPackageSource,
    runtime::MonitoringRuntime,
    target::ExecutableTarget,
    wasi::Wasi,
};

const TICK: Duration = Duration::from_millis(250);

/// The unstable `wasmer run` subcommand.
///
/// WASIX and WASI programs run on the WASIX v2 kernel: WASIX v2 modules natively, WASIX v1 and
/// WASI preview1 modules through its v1 compatibility ABI. A module with neither ABI's imports
/// runs as plain WebAssembly. A precompiled module (`wasmer compile` output) runs as it was
/// compiled, without the preemption points the kernel adds to modules: a busy loop in it takes
/// no signals, and only the third Ctrl-C in a row interrupts it.
#[derive(Debug, Parser)]
pub struct Run {
    #[clap(flatten)]
    env: WasmerEnv,
    #[clap(flatten)]
    rt: RuntimeOptions,
    #[clap(flatten)]
    wasi: crate::commands::run::Wasi,
    /// Set the default stack size (default is 1048576)
    #[clap(long = "stack-size")]
    stack_size: Option<usize>,
    /// The command of a webc package to run, instead of its entrypoint.
    #[clap(short, long, aliases = &["command", "command-name"])]
    entrypoint: Option<String>,
    /// The function to invoke. WASIX and WASI programs always start at `_start`.
    #[clap(short, long)]
    invoke: Option<String>,
    /// Generate a coredump at this path if a WebAssembly trap occurs
    #[clap(name = "COREDUMP_PATH", long)]
    coredump_on_trap: Option<PathBuf>,
    /// Enable experimental N-API imports for modules that require them (not supported on the
    /// WASIX v2 kernel)
    #[clap(long = "experimental-napi")]
    experimental_napi: bool,
    /// The file, URL, or package to run.
    #[clap(value_parser = CliPackageSource::infer)]
    input: CliPackageSource,
    /// Command-line arguments passed to the package
    args: Vec<String>,
}

/// The guest exited with a non-zero status, which `wasmer run` exits with.
#[derive(Debug, thiserror::Error)]
#[error("the program exited with status {0}")]
struct GuestExit(i32);

impl Run {
    pub fn execute(self, output: Output) -> ! {
        let result = self.execute_inner(output);
        exit_with_wasi_exit_code(result);
    }

    #[tracing::instrument(level = "debug", name = "wasmer_run", skip_all)]
    fn execute_inner(mut self, output: Output) -> Result<(), Error> {
        self.print_option_warnings();

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

        let wasm = self.module_bytes();
        let target = Target::default();
        let backend = match &wasm {
            Some(wasm) => {
                // Before compiling: a wasm64 module may not compile at all.
                if let Some(reason) =
                    wasixv2::is_wasix1_module(wasm).and_then(|f| f.unsupported_reason())
                {
                    bail!("{reason}");
                }
                tracing::info!("Attempting to detect WebAssembly features from binary");
                let features = self.rt.detect_features_from_wasm(wasm).unwrap_or_default();
                self.rt.get_backend_for_features(&features, &target)?
            }
            None => *self
                .rt
                .get_available_backends()?
                .first()
                .context("no compiler backend enabled")?,
        };
        let engine = backend.get_engine(&target, &self.rt)?;
        tracing::info!("Executing on backend {:?}", engine.deterministic_id());

        #[cfg(feature = "sys")]
        if engine.is_sys()
            && let Some(stack_size) = self.stack_size
        {
            wasmer_vm::set_stack_size(stack_size);
        }

        let runtime = self.wasi.prepare_runtime(
            engine.clone(),
            &self.env,
            runtime,
            preferred_webc_version,
            self.rt.compiler_debug_dir.is_some(),
        )?;

        // This is a slow operation, so let's temporarily wrap the runtime with
        // something that displays progress
        let monitoring_runtime = Arc::new(
            MonitoringRuntime::new(runtime, pb.clone(), output.is_quiet_or_no_tty())
                .with_checkouts(FileSystemCache::new(self.env.cache_dir().join("checkouts"))),
        );
        let runtime: Arc<dyn Runtime + Send + Sync> = monitoring_runtime.clone();
        // Compiles everything the machine runs, starting with the engine picked here.
        let loader = Arc::new(CachedLoader::new(
            runtime.clone(),
            self.rt.clone(),
            (backend, engine.clone()),
        ));

        // `--use`: packages whose commands and volumes join the program's.
        let used = self.used_packages(&monitoring_runtime)?;

        // A `#!` script runs as its interpreter does on Linux: the kernel's `execve` reads the
        // `#!` line and starts the interpreter it names, from the guest's filesystem.
        if let CliPackageSource::File(path) = &self.input
            && machine::is_script(path)
        {
            pb.finish_and_clear();
            let program = MachineProgram::script(self.script_guest_path(path)?).using(used);
            return self.execute_machine(program, loader, &engine, &handle);
        }

        // A module the machine runs compiles once, with its preemption points, as the images
        // it `execve`s do.
        if let (CliPackageSource::File(path), Some(wasm)) = (&self.input, wasm)
            && wasm_runs_on_machine(&wasm)?
        {
            pb.set_message("Compiling to WebAssembly");
            let image = Image::copied(wasm);
            let (engine, module) = handle
                .block_on(loader.load(&engine, &image))
                .with_context(|| format!("Unable to compile \"{}\"", path.display()))?;
            pb.finish_and_clear();
            let program = MachineProgram::module(path, engine.clone(), module).using(used);
            return self.execute_machine(program, loader, &engine, &handle);
        }

        let target = self.input.resolve_target(&runtime, &pb)?;

        if let ExecutableTarget::Package(ref pkg) = target {
            self.wasi
                .volumes
                .extend(pkg.additional_host_mapped_directories.clone());
        }

        pb.finish_and_clear();

        match target {
            // A precompiled artifact runs as it is, without preemption points; anything else
            // that reaches here is plain WebAssembly.
            ExecutableTarget::WebAssembly { module, path, .. } => {
                if runs_on_machine(&module)? {
                    let program = MachineProgram::module(&path, engine.clone(), module).using(used);
                    self.execute_machine(program, loader, &engine, &handle)
                } else {
                    self.execute_pure_wasm_module(&module)
                }
            }
            ExecutableTarget::Package(pkg) => {
                if pkg.webc_version == webc::Version::V2 {
                    crate::warning!(
                        "WebC v2 is a deprecated format and support for it will be removed in a future release"
                    );
                }
                let tree = monitoring_runtime.package_tree();
                let program = self.machine_program(&pkg, &tree)?.using(used);
                self.execute_machine(program, loader, &engine, &handle)
            }
        }
    }

    /// The input's WebAssembly, when it is a module file, to pick a backend with the features
    /// it uses.
    fn module_bytes(&self) -> Option<Vec<u8>> {
        let CliPackageSource::File(path) = &self.input else {
            return None;
        };
        match TargetOnDisk::from_file(path).ok()? {
            TargetOnDisk::WebAssemblyBinary => std::fs::read(path).ok(),
            TargetOnDisk::Wat => {
                let wat = std::fs::read(path).ok()?;
                wat2wasm(&wat).ok().map(|wasm| wasm.into_owned())
            }
            TargetOnDisk::LocalWebc | TargetOnDisk::Artifact => None,
        }
    }

    #[tracing::instrument(skip_all)]
    fn execute_pure_wasm_module(&self, module: &Module) -> Result<(), Error> {
        // The rest of the execution happens in the main thread, so we can create the
        // store here.
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

        let result = invoke_function(&mut store, entry_function, &self.args)?;

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

    fn print_option_warnings(&self) {
        if !self.wasi.mapped_dirs.is_empty() {
            eprintln!(
                "{}The `{}` option is deprecated and will be removed in the next major release. Please use `{}` instead.",
                "warning: ".yellow(),
                "--mapdir".yellow(),
                "--volume".green()
            );
        }
        if !self.wasi.pre_opened_directories.is_empty() {
            eprintln!(
                "{}The `{}` option is deprecated and will be removed in the next major release. Please use `{}` instead.",
                "warning: ".yellow(),
                "--dir".yellow(),
                "--volume".green()
            );
        }
    }
}

fn invoke_function(
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

    if let Some(result_type) = func_ty.results().iter().find(|ty| ty.is_ref()) {
        bail!("Function result type {result_type} is not supported by --invoke");
    }

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

/// Exit the current process, with the guest's exit status if the error carries one.
fn exit_with_wasi_exit_code(result: Result<(), Error>) -> ! {
    let exit_code = match result {
        Ok(_) => 0,
        Err(error) => match error.downcast_ref::<GuestExit>() {
            Some(GuestExit(status)) => *status,
            None => {
                eprintln!("{:?}", PrettyError::new(error));
                // Something else happened
                1
            }
        },
    };

    std::io::stdout().flush().ok();
    std::io::stderr().flush().ok();

    std::process::exit(exit_code);
}
