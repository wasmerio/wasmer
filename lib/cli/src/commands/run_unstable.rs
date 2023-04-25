#![allow(missing_docs, unused)]

use std::{
    collections::BTreeMap,
    fmt::Display,
    fs::File,
    io::{ErrorKind, LineWriter, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Error};
use clap::Parser;
use clap_verbosity_flag::WarnLevel;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use url::Url;
use wapm_targz_to_pirita::FileMap;
use wasmer::{
    DeserializeError, Engine, Function, Imports, Instance, Module, Store, Type, TypedFunction,
    Value,
};
use wasmer_cache::Cache;
#[cfg(feature = "compiler")]
use wasmer_compiler::ArtifactBuild;
use wasmer_registry::Package;
use wasmer_wasix::runners::wcgi::AbortHandle;
use wasmer_wasix::runners::{MappedDirectory, Runner};
use webc::{metadata::Manifest, v1::DirOrFile, Container};

use crate::{
    store::StoreOptions,
    wasmer_home::{DownloadCached, ModuleCache, WasmerHome},
};

/// The unstable `wasmer run` subcommand.
#[derive(Debug, Parser)]
pub struct RunUnstable {
    #[clap(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<WarnLevel>,
    #[clap(flatten)]
    wasmer_home: WasmerHome,
    #[clap(flatten)]
    store: StoreOptions,
    #[clap(flatten)]
    wasi: crate::commands::run::Wasi,
    #[clap(flatten)]
    wcgi: WcgiOptions,
    #[cfg(feature = "sys")]
    /// The stack size (default is 1048576)
    #[clap(long = "stack-size")]
    stack_size: Option<usize>,
    /// The function or command to invoke.
    #[clap(short, long, aliases = &["command", "invoke"])]
    entrypoint: Option<String>,
    /// Generate a coredump at this path if a WebAssembly trap occurs
    #[clap(name = "COREDUMP PATH", long, parse(from_os_str))]
    coredump_on_trap: Option<PathBuf>,
    /// The file, URL, or package to run.
    #[clap(value_parser = PackageSource::infer)]
    input: PackageSource,
    /// Command-line arguments passed to the package
    args: Vec<String>,
}

impl RunUnstable {
    pub fn execute(&self) -> Result<(), Error> {
        crate::logging::set_up_logging(self.verbosity.log_level_filter());

        let target = self
            .input
            .resolve_target(&self.wasmer_home)
            .with_context(|| format!("Unable to resolve \"{}\"", self.input))?;

        let (mut store, _) = self.store.get_store()?;

        let mut cache = self.wasmer_home.module_cache();
        let result = match target.load(&mut cache, &store)? {
            ExecutableTarget::WebAssembly(wasm) => self.execute_wasm(&target, &wasm, &mut store),
            ExecutableTarget::Webc(container) => {
                self.execute_webc(&target, container, cache, &mut store)
            }
        };

        if let Err(e) = &result {
            #[cfg(feature = "coredump")]
            if let Some(coredump) = &self.coredump_on_trap {
                if let Err(e) = generate_coredump(e, target.path(), coredump) {
                    tracing::warn!(
                        error = &*e as &dyn std::error::Error,
                        coredump_path=%coredump.display(),
                        "Unable to generate a coredump",
                    );
                }
            }
        }

        result
    }

    fn execute_wasm(
        &self,
        target: &TargetOnDisk,
        module: &Module,
        store: &mut Store,
    ) -> Result<(), Error> {
        #[cfg(feature = "sys")]
        if self.stack_size.is_some() {
            wasmer_vm::set_stack_size(self.stack_size.unwrap());
        }
        if wasmer_emscripten::is_emscripten_module(module) {
            self.execute_emscripten_module()
        } else if wasmer_wasix::is_wasi_module(module) || wasmer_wasix::is_wasix_module(module) {
            self.execute_wasi_module(target.path(), module, store)
        } else {
            self.execute_pure_wasm_module(module, store)
        }
    }

    #[tracing::instrument(skip_all)]
    fn execute_webc(
        &self,
        target: &TargetOnDisk,
        container: Container,
        mut cache: ModuleCache,
        store: &mut Store,
    ) -> Result<(), Error> {
        #[cfg(feature = "sys")]
        if self.stack_size.is_some() {
            wasmer_vm::set_stack_size(self.stack_size.unwrap());
        }
        let id = match self.entrypoint.as_deref() {
            Some(cmd) => cmd,
            None => infer_webc_entrypoint(container.manifest())?,
        };
        let command = container
            .manifest()
            .commands
            .get(id)
            .with_context(|| format!("Unable to get metadata for the \"{id}\" command"))?;

        let (store, _compiler_type) = self.store.get_store()?;
        let runner_base = command
            .runner
            .as_str()
            .split_once('@')
            .map(|(base, version)| base)
            .unwrap_or_else(|| command.runner.as_str());

        let cache = Mutex::new(cache);

        match runner_base {
            webc::metadata::annotations::EMSCRIPTEN_RUNNER_URI => {
                let mut runner = wasmer_wasix::runners::emscripten::EmscriptenRunner::new(store);
                runner.set_args(self.args.clone());
                if runner.can_run_command(id, command).unwrap_or(false) {
                    return runner
                        .run_cmd(&container, id)
                        .context("Emscripten runner failed");
                }
            }
            webc::metadata::annotations::WCGI_RUNNER_URI => {
                let mut runner = wasmer_wasix::runners::wcgi::WcgiRunner::new(id).with_compile(
                    move |engine, bytes| {
                        let mut cache = cache.lock().unwrap();
                        compile_wasm_cached("".to_string(), bytes, &mut cache, engine)
                    },
                );

                runner
                    .config()
                    .args(self.args.clone())
                    .store(store)
                    .addr(self.wcgi.addr)
                    .envs(self.wasi.env_vars.clone())
                    .map_directories(self.wasi.mapped_dirs.clone())
                    .callbacks(Callbacks::new(self.wcgi.addr));
                if self.wasi.forward_host_env {
                    runner.config().forward_host_env();
                }
                if runner.can_run_command(id, command).unwrap_or(false) {
                    return runner.run_cmd(&container, id).context("WCGI runner failed");
                }
            }
            // TODO: Add this on the webc annotation itself
            "https://webc.org/runner/wasi/command"
            | webc::metadata::annotations::WASI_RUNNER_URI => {
                let mut runner = wasmer_wasix::runners::wasi::WasiRunner::new(store)
                    .with_compile(move |engine, bytes| {
                        let mut cache = cache.lock().unwrap();
                        compile_wasm_cached("".to_string(), bytes, &mut cache, engine)
                    })
                    .with_args(self.args.clone())
                    .with_envs(self.wasi.env_vars.clone())
                    .with_mapped_directories(self.wasi.mapped_dirs.clone());
                if self.wasi.forward_host_env {
                    runner.set_forward_host_env();
                }
                if runner.can_run_command(id, command).unwrap_or(false) {
                    return runner.run_cmd(&container, id).context("WASI runner failed");
                }
            }
            _ => {}
        }

        anyhow::bail!(
            "Unable to find a runner that supports \"{}\"",
            command.runner
        );
    }

    #[tracing::instrument(skip_all)]
    fn execute_pure_wasm_module(&self, module: &Module, store: &mut Store) -> Result<(), Error> {
        let imports = Imports::default();
        let instance = Instance::new(store, module, &imports)
            .context("Unable to instantiate the WebAssembly module")?;

        let entrypoint  = match &self.entrypoint {
            Some(entry) => {
                instance.exports
                    .get_function(entry)
                    .with_context(|| format!("The module doesn't contain a \"{entry}\" function"))?
            },
            None => {
                instance.exports.get_function("_start")
                    .context("The module doesn't contain a \"_start\" function. Either implement it or specify an entrypoint function.")?
            }
        };

        let return_values = invoke_function(&instance, store, entrypoint, &self.args)?;

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

    #[tracing::instrument(skip_all)]
    fn execute_wasi_module(
        &self,
        wasm_path: &Path,
        module: &Module,
        store: &mut Store,
    ) -> Result<(), Error> {
        let program_name = wasm_path.display().to_string();
        let builder = self
            .wasi
            .prepare(store, module, program_name, self.args.clone())?;

        builder.run_with_store(module.clone(), store)?;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    fn execute_emscripten_module(&self) -> Result<(), Error> {
        todo!()
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
        _ => anyhow::bail!("There is no known conversion from {s:?} to {ty:?}"),
    };
    Ok(value)
}

fn infer_webc_entrypoint(manifest: &Manifest) -> Result<&str, Error> {
    if let Some(entrypoint) = manifest.entrypoint.as_deref() {
        return Ok(entrypoint);
    }

    let commands: Vec<_> = manifest.commands.keys().collect();

    match commands.as_slice() {
        [] => anyhow::bail!("The WEBC file doesn't contain any executable commands"),
        [one] => Ok(one.as_str()),
        [..] => {
            anyhow::bail!(
                "Unable to determine the WEBC file's entrypoint. Please choose one of {commands:?}"
            );
        }
    }
}

fn compile_directory_to_webc(dir: &Path) -> Result<Vec<u8>, Error> {
    let mut files = BTreeMap::new();
    load_files_from_disk(&mut files, dir, dir)?;

    let wasmer_toml = DirOrFile::File("wasmer.toml".into());
    if let Some(toml_data) = files.remove(&wasmer_toml) {
        // HACK(Michael-F-Bryan): The version of wapm-targz-to-pirita we are
        // using doesn't know we renamed "wapm.toml" to "wasmer.toml", so we
        // manually patch things up if people have already migrated their
        // projects.
        files
            .entry(DirOrFile::File("wapm.toml".into()))
            .or_insert(toml_data);
    }

    let functions = wapm_targz_to_pirita::TransformManifestFunctions::default();
    wapm_targz_to_pirita::generate_webc_file(files, dir, None, &functions)
}

fn load_files_from_disk(files: &mut FileMap, dir: &Path, base: &Path) -> Result<(), Error> {
    let entries = dir
        .read_dir()
        .with_context(|| format!("Unable to read the contents of \"{}\"", dir.display()))?;

    for entry in entries {
        let path = entry?.path();
        let relative_path = path.strip_prefix(base)?.to_path_buf();

        if path.is_dir() {
            load_files_from_disk(files, &path, base)?;
            files.insert(DirOrFile::Dir(relative_path), Vec::new());
        } else if path.is_file() {
            let data = std::fs::read(&path)
                .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
            files.insert(DirOrFile::File(relative_path), data);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum PackageSource {
    File(PathBuf),
    Dir(PathBuf),
    Package(Package),
    Url(Url),
}

impl PackageSource {
    fn infer(s: &str) -> Result<PackageSource, Error> {
        let path = Path::new(s);
        if path.is_file() {
            return Ok(PackageSource::File(path.to_path_buf()));
        } else if path.is_dir() {
            return Ok(PackageSource::Dir(path.to_path_buf()));
        }

        if let Ok(url) = Url::parse(s) {
            return Ok(PackageSource::Url(url));
        }

        if let Ok(pkg) = Package::from_str(s) {
            return Ok(PackageSource::Package(pkg));
        }

        Err(anyhow::anyhow!(
            "Unable to resolve \"{s}\" as a URL, package name, or file on disk"
        ))
    }

    /// Try to resolve the [`PackageSource`] to an artifact on disk.
    ///
    /// This will try to automatically download and cache any resources from the
    /// internet.
    fn resolve_target(&self, home: &impl DownloadCached) -> Result<TargetOnDisk, Error> {
        match self {
            PackageSource::File(path) => TargetOnDisk::from_file(path.clone()),
            PackageSource::Dir(d) => Ok(TargetOnDisk::Directory(d.clone())),
            PackageSource::Package(pkg) => {
                let cached = home.download_package(pkg)?;
                Ok(TargetOnDisk::Webc(cached))
            }
            PackageSource::Url(url) => {
                let cached = home.download_url(url)?;
                Ok(TargetOnDisk::Webc(cached))
            }
        }
    }
}

impl Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::File(path) | PackageSource::Dir(path) => write!(f, "{}", path.display()),
            PackageSource::Package(p) => write!(f, "{p}"),
            PackageSource::Url(u) => write!(f, "{u}"),
        }
    }
}

/// A file/directory on disk that will be executed.
///
/// Depending on the type of target and the command-line arguments, this might
/// be something the user passed in manually or something that was automatically
/// saved to `$WASMER_HOME` for caching purposes.
#[derive(Debug, Clone)]
enum TargetOnDisk {
    WebAssemblyBinary(PathBuf),
    Wat(PathBuf),
    Webc(PathBuf),
    Directory(PathBuf),
    Artifact(PathBuf),
}

impl TargetOnDisk {
    fn from_file(path: PathBuf) -> Result<TargetOnDisk, Error> {
        // Normally the first couple hundred bytes is enough to figure
        // out what type of file this is.
        let mut buffer = [0_u8; 512];

        let mut f = File::open(&path)
            .with_context(|| format!("Unable to open \"{}\" for reading", path.display(),))?;
        let bytes_read = f.read(&mut buffer)?;

        let leading_bytes = &buffer[..bytes_read];

        if wasmer::is_wasm(leading_bytes) {
            Ok(TargetOnDisk::WebAssemblyBinary(path))
        } else if webc::detect(leading_bytes).is_ok() {
            Ok(TargetOnDisk::Webc(path))
        } else if path.extension() == Some("wat".as_ref()) {
            Ok(TargetOnDisk::Wat(path))
        } else {
            #[cfg(feature = "compiler")]
            if ArtifactBuild::is_deserializable(leading_bytes) {
                return Ok(TargetOnDisk::Artifact(path));
            }
            anyhow::bail!("Unable to determine how to execute \"{}\"", path.display());
        }
    }

    fn path(&self) -> &Path {
        match self {
            TargetOnDisk::WebAssemblyBinary(p)
            | TargetOnDisk::Webc(p)
            | TargetOnDisk::Wat(p)
            | TargetOnDisk::Directory(p)
            | TargetOnDisk::Artifact(p) => p,
        }
    }

    fn load(&self, cache: &mut ModuleCache, store: &Store) -> Result<ExecutableTarget, Error> {
        match self {
            TargetOnDisk::Webc(webc) => {
                // As an optimisation, try to use the mmapped version first.
                if let Ok(container) = Container::from_disk(webc.clone()) {
                    return Ok(ExecutableTarget::Webc(container));
                }

                // Otherwise, fall back to the version that reads everything
                // into memory.
                let bytes = std::fs::read(webc)
                    .with_context(|| format!("Unable to read \"{}\"", webc.display()))?;
                let container = Container::from_bytes(bytes)?;

                Ok(ExecutableTarget::Webc(container))
            }
            TargetOnDisk::Directory(dir) => {
                // FIXME: Runners should be able to load directories directly
                // instead of needing to compile to a WEBC file.
                let webc = compile_directory_to_webc(dir).with_context(|| {
                    format!("Unable to bundle \"{}\" as a WEBC package", dir.display())
                })?;
                let container = Container::from_bytes(webc)
                    .context("Unable to parse the generated WEBC file")?;

                Ok(ExecutableTarget::Webc(container))
            }
            TargetOnDisk::WebAssemblyBinary(path) => {
                let wasm = std::fs::read(path)
                    .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
                let module =
                    compile_wasm_cached(path.display().to_string(), &wasm, cache, store.engine())?;
                Ok(ExecutableTarget::WebAssembly(module))
            }
            TargetOnDisk::Wat(path) => {
                let wat = std::fs::read(path)
                    .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
                let wasm =
                    wasmer::wat2wasm(&wat).context("Unable to convert the WAT to WebAssembly")?;

                let module =
                    compile_wasm_cached(path.display().to_string(), &wasm, cache, store.engine())?;
                Ok(ExecutableTarget::WebAssembly(module))
            }
            TargetOnDisk::Artifact(artifact) => {
                let module = unsafe {
                    Module::deserialize_from_file(store, artifact)
                        .context("Unable to deserialize the pre-compiled module")?
                };
                Ok(ExecutableTarget::WebAssembly(module))
            }
        }
    }
}

fn compile_wasm_cached(
    name: String,
    wasm: &[u8],
    cache: &mut ModuleCache,
    engine: &Engine,
) -> Result<Module, Error> {
    tracing::debug!("Trying to retrieve module from cache");

    let hash = wasmer_cache::Hash::generate(wasm);
    tracing::debug!("Generated hash: {}", hash);

    unsafe {
        match cache.load(engine, hash) {
            Ok(m) => {
                tracing::debug!(%hash, "Module loaded from cache");
                return Ok(m);
            }
            Err(DeserializeError::Io(e)) if e.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    %hash,
                    error=&error as &dyn std::error::Error,
                    name=%name,
                    "Unable to deserialize the cached module",
                );
            }
        }
    }

    let mut module = Module::new(engine, wasm).context("Unable to load the module from a file")?;
    module.set_name(&name);

    if let Err(e) = cache.store(hash, &module) {
        tracing::warn!(
            error=&e as &dyn std::error::Error,
            wat=%name,
            key=%hash,
            "Unable to cache the compiled module",
        );
    }

    Ok(module)
}

#[derive(Debug, Clone)]
enum ExecutableTarget {
    WebAssembly(Module),
    Webc(Container),
}

#[cfg(feature = "coredump")]
fn generate_coredump(err: &Error, source: &Path, coredump_path: &Path) -> Result<(), Error> {
    let err: &wasmer::RuntimeError = match err.downcast_ref() {
        Some(e) => e,
        None => {
            log::warn!("no runtime error found to generate coredump with");
            return Ok(());
        }
    };

    let source_name = source.display().to_string();
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
