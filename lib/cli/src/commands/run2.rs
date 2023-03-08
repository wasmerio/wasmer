#![allow(missing_docs, unused)]

use std::{
    fmt::Display,
    fs::File,
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Error};
use clap::Parser;
use tempfile::NamedTempFile;
use url::Url;
use wasmer::{Module, Store};
use wasmer_compiler::ArtifactBuild;
use wasmer_registry::Package;
use wasmer_wasix::runners::{Runner, WapmContainer};
use webc::metadata::Manifest;

use crate::store::StoreOptions;

/// The `wasmer run` subcommand.
#[derive(Debug, Parser)]
pub struct Run2 {
    #[clap(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,
    #[clap(flatten)]
    wasmer_home: WasmerHome,
    #[clap(flatten)]
    store: StoreOptions,
    #[clap(flatten)]
    wasi: crate::commands::run::Wasi,
    #[clap(flatten)]
    wcgi: WcgiOptions,
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

impl Run2 {
    pub fn execute(&self) -> Result<(), Error> {
        crate::logging::set_up_logging(self.verbosity.log_level_filter());
        tracing::info!("Started!");

        let target = self
            .input
            .resolve_target(&self.wasmer_home)
            .with_context(|| format!("Unable to resolve \"{}\"", self.input))?;

        let (mut store, _) = self.store.get_store()?;

        let result = match target.load(&self.wasmer_home, &store)? {
            ExecutableTarget::WebAssembly(wasm) => self.execute_wasm(&target, &wasm, &mut store),
            ExecutableTarget::Webc(container) => self.execute_webc(&target, &container, &mut store),
        };

        if let Err(e) = &result {
            if let Some(coredump) = &self.coredump_on_trap {
                generate_coredump(e, target.path(), coredump).context("Unable")?
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
        if wasmer_emscripten::is_emscripten_module(module) {
            execute_emscripten_module()
        } else if wasmer_wasix::is_wasi_module(module) || wasmer_wasix::is_wasix_module(module) {
            execute_wasi_module()
        } else {
            execute_pure_wasm_module()
        }
    }

    fn execute_webc(
        &self,
        target: &TargetOnDisk,
        container: &WapmContainer,
        store: &mut Store,
    ) -> Result<(), Error> {
        let id = match self.entrypoint.as_deref() {
            Some(cmd) => cmd,
            None => infer_webc_entrypoint(container.manifest())
                .context("Unable to infer the entrypoint. Please specify it manually")?,
        };
        let command = container
            .manifest()
            .commands
            .get(id)
            .with_context(|| format!("Unable to get metadata for the \"{id}\" command"))?;

        let (store, _compiler_type) = self.store.get_store()?;
        let mut runner = wasmer_wasix::runners::wasi::WasiRunner::new(store);
        runner.set_args(self.args.clone());
        if runner.can_run_command(id, command).unwrap_or(false) {
            return runner.run_cmd(&container, id).context("WASI runner failed");
        }

        let (store, _compiler_type) = self.store.get_store()?;
        let mut runner = wasmer_wasix::runners::emscripten::EmscriptenRunner::new(store);
        runner.set_args(self.args.clone());
        if runner.can_run_command(id, command).unwrap_or(false) {
            return runner
                .run_cmd(&container, id)
                .context("Emscripten runner failed");
        }

        let mut runner = wasmer_wasix::runners::wcgi::WcgiRunner::new(id);
        let (store, _compiler_type) = self.store.get_store()?;
        runner
            .config()
            .args(self.args.clone())
            .store(store)
            .addr(self.wcgi.addr)
            .envs(self.wasi.env_vars.clone())
            .map_directories(self.wasi.mapped_dirs.iter().map(|(g, h)| (h, g)));
        if self.wcgi.forward_host_env {
            runner.config().forward_host_env();
        }
        if runner.can_run_command(id, command).unwrap_or(false) {
            return runner.run_cmd(&container, id).context("WCGI runner failed");
        }

        anyhow::bail!(
            "Unable to find a runner that supports \"{}\"",
            command.runner
        );
    }
}

fn execute_pure_wasm_module() -> Result<(), Error> {
    todo!()
}

fn execute_wasi_module() -> Result<(), Error> {
    todo!()
}

fn execute_emscripten_module() -> Result<(), Error> {
    todo!()
}

fn infer_webc_entrypoint(manifest: &Manifest) -> Result<&str, Error> {
    if let Some(entrypoint) = manifest.entrypoint.as_deref() {
        return Ok(entrypoint);
    }

    let commands: Vec<_> = manifest.commands.keys().collect();

    match commands.as_slice() {
        [] => anyhow::bail!("The WEBC file doesn't contain any executable commands",),
        [one] => Ok(one.as_str()),
        [..] => {
            anyhow::bail!(
                "Unable to determine the WEBC file's entrypoint. Please choose one of {commands:?}"
            );
        }
    }
}

fn compile_directory_to_webc<W: Write>(dir: &Path, dest: W) -> Result<(), Error> {
    todo!()
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
        if let Ok(url) = Url::parse(s) {
            return Ok(PackageSource::Url(url));
        }

        let path = Path::new(s);
        if path.exists() {
            return Ok(PackageSource::File(path.to_path_buf()));
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

fn get_cached_package(pkg: &Package, home: &WasmerHome) -> Result<Vec<u8>, Error> {
    todo!();
}

/// Something which can fetch resources from the internet and will cache them
/// locally.
trait DownloadCached {
    fn download_package(&self, pkg: &Package) -> Result<PathBuf, Error>;
    fn download_url(&self, url: &url::Url) -> Result<PathBuf, Error>;
}

#[derive(Debug, Parser)]
struct WasmerHome {
    /// The Wasmer home directory.
    #[clap(long = "wasmer-dir", env = "WASMER_DIR")]
    home: Option<PathBuf>,
    /// Override the registry packages are downloaded from.
    #[clap(long, env = "WASMER_REGISTRY")]
    registry: Option<String>,
    /// Skip all caching.
    #[clap(long)]
    no_cache: bool,
}

impl WasmerHome {
    fn wasmer_home(&self) -> Result<PathBuf, Error> {
        if let Some(wasmer_home) = &self.home {
            return Ok(wasmer_home.clone());
        }

        if let Some(user_home) = dirs::home_dir() {
            return Ok(user_home.join(".wasmer"));
        }

        anyhow::bail!("Unable to determine the Wasmer directory");
    }
}

impl DownloadCached for WasmerHome {
    fn download_package(&self, pkg: &Package) -> Result<PathBuf, Error> {
        let home = self.wasmer_home()?;
        let checkouts = wasmer_registry::get_checkouts_dir(&home);
        todo!();
    }

    fn download_url(&self, url: &url::Url) -> Result<PathBuf, Error> {
        let home = self.wasmer_home()?;
        let checkouts = wasmer_registry::get_checkouts_dir(&home);
        let temp = NamedTempFile::new()?;
        todo!();
    }
}

impl wasmer_cache::Cache for WasmerHome {
    type SerializeError = wasmer::SerializeError;
    type DeserializeError = wasmer::DeserializeError;

    unsafe fn load(
        &self,
        engine: &impl wasmer::AsEngineRef,
        key: wasmer_cache::Hash,
    ) -> Result<wasmer::Module, Self::DeserializeError> {
        todo!()
    }

    fn store(
        &mut self,
        key: wasmer_cache::Hash,
        module: &wasmer::Module,
    ) -> Result<(), Self::SerializeError> {
        todo!()
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
        } else if ArtifactBuild::is_deserializable(leading_bytes) {
            Ok(TargetOnDisk::Artifact(path))
        } else if path.extension() == Some("wat".as_ref()) {
            Ok(TargetOnDisk::Wat(path))
        } else {
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

    fn load(
        &self,
        cache: &impl wasmer_cache::Cache,
        store: &Store,
    ) -> Result<ExecutableTarget, Error> {
        match self {
            TargetOnDisk::Webc(webc) => {
                // As an optimisation, try to use the mmapped version first.
                if let Ok(container) = WapmContainer::from_path(webc.clone()) {
                    return Ok(ExecutableTarget::Webc(container));
                }

                // Otherwise, fall back to the version that reads everything
                // into memory.
                let bytes = std::fs::read(webc)
                    .with_context(|| format!("Unable to read \"{}\"", webc.display()))?;
                let container = WapmContainer::from_bytes(bytes.into())?;

                Ok(ExecutableTarget::Webc(container))
            }
            TargetOnDisk::Directory(dir) => {
                let mut temp = NamedTempFile::new()?;
                compile_directory_to_webc(&dir, &mut temp).with_context(|| {
                    format!("Unable to bundle \"{}\" was a WEBC package", dir.display())
                })?;

                todo!("Figure out where to put the compiled WEBC in a way that won't fill the disk over time");
            }
            TargetOnDisk::WebAssemblyBinary(wasm) => {
                let module = Module::from_file(store, wasm)
                    .context("Unable to load the module from a file")?;
                Ok(ExecutableTarget::WebAssembly(module))
            }
            TargetOnDisk::Wat(wat) => {
                let wat = std::fs::read(wat)
                    .with_context(|| format!("Unable to read \"{}\"", wat.display()))?;
                let wasm =
                    wasmer::wat2wasm(&wat).context("Unable to convert the WAT to WebAssembly")?;
                let module =
                    Module::new(store, wasm).context("Unable to load the module from a file")?;
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

#[derive(Debug, Clone)]
enum ExecutableTarget {
    WebAssembly(Module),
    Webc(WapmContainer),
}

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
        .map_err(|e| Error::msg(e))
        .context("Coredump serializing failed")?;

    std::fs::write(&coredump_path, &coredump).with_context(|| {
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
    /// Forward all host env variables to the wcgi task.
    #[clap(long)]
    pub(crate) forward_host_env: bool,
}

impl Default for WcgiOptions {
    fn default() -> Self {
        Self {
            addr: ([127, 0, 0, 1], 8000).into(),
            forward_host_env: false,
        }
    }
}
