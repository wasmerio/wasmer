use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{mpsc::Sender, Arc},
    time::Duration,
};

use anyhow::{Context, Result};
use bytes::Bytes;
use clap::Parser;
use tokio::runtime::Handle;
use url::Url;
use virtual_fs::{DeviceFile, FileSystem, PassthruFileSystem, RootFileSystemBuilder};
use wasmer::{Engine, Function, Instance, Memory32, Memory64, Module, RuntimeError, Store, Value};
use wasmer_registry::wasmer_env::WasmerEnv;
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    default_fs_backing, get_wasi_versions,
    http::HttpClient,
    os::{tty_sys::SysTty, TtyBridge},
    rewind_ext,
    runners::MappedDirectory,
    runtime::{
        module_cache::{FileSystemCache, ModuleCache},
        package_loader::{BuiltinPackageLoader, PackageLoader},
        resolver::{
            FileSystemSource, InMemorySource, MultiSource, PackageSpecifier, Source, WapmSource,
            WebSource,
        },
        task_manager::{
            tokio::{RuntimeOrHandle, TokioTaskManager},
            VirtualTaskManagerExt,
        },
    },
    types::__WASI_STDIN_FILENO,
    wasmer_wasix_types::wasi::Errno,
    PluggableRuntime, RewindState, Runtime, WasiEnv, WasiEnvBuilder, WasiError, WasiFunctionEnv,
    WasiVersion,
};

use crate::utils::{parse_envvar, parse_mapdir};

const WAPM_SOURCE_CACHE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Parser, Clone, Default)]
/// WASI Options
pub struct Wasi {
    /// WASI pre-opened directory
    #[clap(long = "dir", name = "DIR", group = "wasi")]
    pub(crate) pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the Wasm module
    #[clap(
        long = "mapdir",
        name = "GUEST_DIR:HOST_DIR",
        value_parser=parse_mapdir,
    )]
    pub(crate) mapped_dirs: Vec<MappedDirectory>,

    /// Pass custom environment variables
    #[clap(
        long = "env",
        name = "KEY=VALUE",
        value_parser=parse_envvar,
    )]
    pub(crate) env_vars: Vec<(String, String)>,

    /// Forward all host env variables to guest
    #[clap(long, env)]
    pub(crate) forward_host_env: bool,

    /// List of other containers this module depends on
    #[clap(long = "use", name = "USE")]
    pub(crate) uses: Vec<String>,

    /// List of webc packages that are explicitly included for execution
    /// Note: these packages will be used instead of those in the registry
    #[clap(long = "include-webc", name = "WEBC")]
    include_webcs: Vec<PathBuf>,

    /// List of injected atoms
    #[clap(long = "map-command", name = "MAPCMD")]
    map_commands: Vec<String>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[cfg_attr(
        feature = "experimental-io-devices",
        clap(long = "enable-experimental-io-devices")
    )]
    enable_experimental_io_devices: bool,

    /// Enable networking with the host network.
    ///
    /// Allows WASI modules to open TCP and UDP connections, create sockets, ...
    #[clap(long = "net")]
    pub networking: bool,

    /// Disables the TTY bridge
    #[clap(long = "no-tty")]
    pub no_tty: bool,

    /// Enables asynchronous threading
    #[clap(long = "enable-async-threads")]
    pub enable_async_threads: bool,

    /// Allow instances to send http requests.
    ///
    /// Access to domains is granted by default.
    #[clap(long)]
    pub http_client: bool,

    /// Require WASI modules to only import 1 version of WASI.
    #[clap(long = "deny-multiple-wasi-versions")]
    pub deny_multiple_wasi_versions: bool,
}

pub struct RunProperties {
    pub ctx: WasiFunctionEnv,
    pub path: PathBuf,
    pub invoke: Option<String>,
    pub args: Vec<String>,
}

#[allow(dead_code)]
impl Wasi {
    pub fn map_dir(&mut self, alias: &str, target_on_disk: PathBuf) {
        self.mapped_dirs.push(MappedDirectory {
            guest: alias.to_string(),
            host: target_on_disk,
        });
    }

    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    /// Gets the WASI version (if any) for the provided module
    pub fn get_versions(module: &Module) -> Option<BTreeSet<WasiVersion>> {
        // Get the wasi version in non-strict mode, so multiple wasi versions
        // are potentially allowed.
        //
        // Checking for multiple wasi versions is handled outside this function.
        get_wasi_versions(module, false)
    }

    /// Checks if a given module has any WASI imports at all.
    pub fn has_wasi_imports(module: &Module) -> bool {
        // Get the wasi version in non-strict mode, so no other imports
        // are allowed
        get_wasi_versions(module, false).is_some()
    }

    pub fn prepare(
        &self,
        module: &Module,
        program_name: String,
        args: Vec<String>,
        rt: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<WasiEnvBuilder> {
        let args = args.into_iter().map(|arg| arg.into_bytes());

        let map_commands = self
            .map_commands
            .iter()
            .map(|map| map.split_once('=').unwrap())
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<HashMap<_, _>>();

        let mut uses = Vec::new();
        for name in &self.uses {
            let specifier = PackageSpecifier::parse(name)
                .with_context(|| format!("Unable to parse \"{name}\" as a package specifier"))?;
            let pkg = {
                let inner_rt = rt.clone();
                rt.task_manager()
                    .spawn_and_block_on(async move {
                        BinaryPackage::from_registry(&specifier, &*inner_rt).await
                    })
                    .with_context(|| format!("Unable to load \"{name}\""))?
            };
            uses.push(pkg);
        }

        let builder = WasiEnv::builder(program_name)
            .runtime(Arc::clone(&rt))
            .args(args)
            .envs(self.env_vars.clone())
            .uses(uses)
            .map_commands(map_commands);

        let mut builder = if wasmer_wasix::is_wasix_module(module) {
            // If we preopen anything from the host then shallow copy it over
            let root_fs = RootFileSystemBuilder::new()
                .with_tty(Box::new(DeviceFile::new(__WASI_STDIN_FILENO)))
                .build();
            if !self.mapped_dirs.is_empty() {
                let fs_backing: Arc<dyn FileSystem + Send + Sync> =
                    Arc::new(PassthruFileSystem::new(default_fs_backing()));
                for MappedDirectory { host, guest } in self.mapped_dirs.clone() {
                    let host = if !host.is_absolute() {
                        Path::new("/").join(host)
                    } else {
                        host
                    };
                    root_fs.mount(guest.into(), &fs_backing, host)?;
                }
            }

            // Open the root of the new filesystem
            builder
                .sandbox_fs(root_fs)
                .preopen_dir(Path::new("/"))
                .unwrap()
                .map_dir(".", "/")?
        } else {
            builder
                .fs(default_fs_backing())
                .preopen_dirs(self.pre_opened_directories.clone())?
                .map_dirs(
                    self.mapped_dirs
                        .iter()
                        .map(|d| (d.guest.clone(), d.host.clone())),
                )?
        };

        *builder.capabilities_mut() = self.capabilities();

        #[cfg(feature = "experimental-io-devices")]
        {
            if self.enable_experimental_io_devices {
                wasi_state_builder
                    .setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
            }
        }

        Ok(builder)
    }

    pub fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::default();

        if self.http_client {
            caps.http_client = wasmer_wasix::http::HttpClientCapabilityV1::new_allow_all();
        }

        caps.threading.enable_asynchronous_threading = self.enable_async_threads;

        caps
    }

    pub fn prepare_runtime<I>(
        &self,
        engine: Engine,
        env: &WasmerEnv,
        rt_or_handle: I,
    ) -> Result<impl Runtime + Send + Sync>
    where
        I: Into<RuntimeOrHandle>,
    {
        let mut rt = PluggableRuntime::new(Arc::new(TokioTaskManager::new(rt_or_handle.into())));

        if self.networking {
            rt.set_networking_implementation(virtual_net::host::LocalNetworking::default());
        } else {
            rt.set_networking_implementation(virtual_net::UnsupportedVirtualNetworking::default());
        }

        if !self.no_tty {
            let tty = Arc::new(SysTty::default());
            tty.reset();
            rt.set_tty(tty);
        }

        let client =
            wasmer_wasix::http::default_http_client().context("No HTTP client available")?;
        let client = Arc::new(client);

        let package_loader = self
            .prepare_package_loader(env, client.clone())
            .context("Unable to prepare the package loader")?;

        let registry = self.prepare_source(env, client)?;

        let cache_dir = env.cache_dir().join("compiled");
        let module_cache = wasmer_wasix::runtime::module_cache::in_memory()
            .with_fallback(FileSystemCache::new(cache_dir));

        rt.set_package_loader(package_loader)
            .set_module_cache(module_cache)
            .set_source(registry)
            .set_engine(Some(engine));

        Ok(rt)
    }

    /// Helper function for instantiating a module with Wasi imports for the `Run` command.
    pub fn instantiate(
        &self,
        module: &Module,
        program_name: String,
        args: Vec<String>,
        runtime: Arc<dyn Runtime + Send + Sync>,
        store: &mut Store,
    ) -> Result<(WasiFunctionEnv, Instance)> {
        let builder = self.prepare(module, program_name, args, runtime)?;
        let (instance, wasi_env) = builder.instantiate(module.clone(), store)?;

        Ok((wasi_env, instance))
    }

    pub fn for_binfmt_interpreter() -> Result<Self> {
        use std::env;
        let dir = env::var_os("WASMER_BINFMT_MISC_PREOPEN")
            .map(Into::into)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            deny_multiple_wasi_versions: true,
            env_vars: env::vars().collect(),
            pre_opened_directories: vec![dir],
            ..Self::default()
        })
    }

    fn prepare_package_loader(
        &self,
        env: &WasmerEnv,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Result<impl PackageLoader + Send + Sync> {
        let checkout_dir = env.cache_dir().join("checkouts");
        let loader = BuiltinPackageLoader::new_with_client(checkout_dir, Arc::new(client));
        Ok(loader)
    }

    fn prepare_source(
        &self,
        env: &WasmerEnv,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Result<impl Source + Send + Sync> {
        let mut source = MultiSource::new();

        // Note: This should be first so our "preloaded" sources get a chance to
        // override the main registry.
        let mut preloaded = InMemorySource::new();
        for path in &self.include_webcs {
            preloaded
                .add_webc(path)
                .with_context(|| format!("Unable to load \"{}\"", path.display()))?;
        }
        source.add_source(preloaded);

        let graphql_endpoint = self.graphql_endpoint(env)?;
        let cache_dir = env.cache_dir().join("queries");
        let wapm_source = WapmSource::new(graphql_endpoint, Arc::clone(&client))
            .with_local_cache(cache_dir, WAPM_SOURCE_CACHE_TIMEOUT);
        source.add_source(wapm_source);

        let cache_dir = env.cache_dir().join("downloads");
        source.add_source(WebSource::new(cache_dir, client));

        source.add_source(FileSystemSource::default());

        Ok(source)
    }

    fn graphql_endpoint(&self, env: &WasmerEnv) -> Result<Url> {
        if let Ok(endpoint) = env.registry_endpoint() {
            return Ok(endpoint);
        }

        let config = env.config()?;
        let graphql_endpoint = config.registry.get_graphql_url();
        let graphql_endpoint = graphql_endpoint
            .parse()
            .with_context(|| format!("Unable to parse \"{graphql_endpoint}\" as a URL"))?;

        Ok(graphql_endpoint)
    }
}

fn parse_registry(r: &str) -> Result<Url> {
    let url = wasmer_registry::format_graphql(r).parse()?;
    Ok(url)
}
