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
use wasmer::{Function, Imports, Instance, Module, RuntimeError, Store, Type, Value, wat2wasm};
use wasmer_types::target::Target;
use wasmer_wasix::Runtime;

use crate::{
    backend::RuntimeOptions, commands::run::target::TargetOnDisk, config::WasmerEnv,
    error::PrettyError, logging::Output,
};

use self::{
    machine::{MachineProgram, runs_on_machine},
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
/// runs as plain WebAssembly.
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
    /// The entrypoint module for webc packages.
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

        let engine = match self.module_bytes() {
            Some(wasm) => {
                // Before compiling: a wasm64 module may not compile at all.
                if let Some(reason) =
                    wasixv2::is_wasix1_module(&wasm).and_then(|f| f.unsupported_reason())
                {
                    bail!("{reason}");
                }
                tracing::info!("Attempting to detect WebAssembly features from binary");
                self.rt.get_engine_for_module(&wasm, &Target::default())?
            }
            None => self.rt.get_engine(&Target::default())?,
        };
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
        let monitoring_runtime = Arc::new(MonitoringRuntime::new(
            runtime,
            pb.clone(),
            output.is_quiet_or_no_tty(),
        ));
        let runtime: Arc<dyn Runtime + Send + Sync> = monitoring_runtime.clone();

        let target = self.input.resolve_target(&runtime, &pb)?;

        if let ExecutableTarget::Package(ref pkg) = target {
            self.wasi
                .volumes
                .extend(pkg.additional_host_mapped_directories.clone());
        }

        pb.finish_and_clear();

        match target {
            ExecutableTarget::WebAssembly { module, path, .. } => {
                if runs_on_machine(&module)? {
                    let program = MachineProgram::module(&path, engine, module);
                    self.execute_machine(program, &handle)
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
                let program = self.machine_program(&pkg, &tree, &runtime)?;
                self.execute_machine(program, &handle)
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

#[cfg(feature = "coredump")]
fn generate_coredump(
    err: &RuntimeError,
    source_name: String,
    coredump_path: &std::path::Path,
) -> Result<(), Error> {
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
