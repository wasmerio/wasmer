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
use virtual_fs::host_fs::FileSystem as HostFileSystem;
use virtual_fs::{DeviceFile, FileSystem, RootFileSystemBuilder};
use wasmer::{Engine, Function, Instance, Memory32, Memory64, Module, RuntimeError, Store, Value};
use wasmer_registry::wasmer_env::WasmerEnv;
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    http::HttpClient,
    os::{tty_sys::SysTty, TtyBridge},
    rewind_ext,
    runners::{MappedCommand, MappedDirectory},
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
    pub(super) include_webcs: Vec<PathBuf>,

    /// List of injected atoms
    #[clap(long = "map-command", name = "MAPCMD")]
    pub(super) map_commands: Vec<String>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[cfg_attr(
        feature = "experimental-io-devices",
        clap(long = "enable-experimental-io-devices")
    )]
    enable_experimental_io_devices: bool,

    /// Mount a new host's /tmp directory into the guest
    #[clap(long = "host-tmp")]
    pub(crate) mount_host_tmp: bool,

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
    pub const MAPPED_CURRENT_DIR_DEFAULT_PATH: &'static str = "/home";

    pub fn map_dir(&mut self, alias: &str, target_on_disk: PathBuf) {
        self.mapped_dirs.push(MappedDirectory {
            guest: alias.to_string(),
            host: target_on_disk,
        });
    }

    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    pub fn get_fs(&self) -> Result<virtual_fs::TmpFileSystem> {
        let root_fs = RootFileSystemBuilder::new()
            .with_tty(Box::new(DeviceFile::new(__WASI_STDIN_FILENO)))
            .build();
        let mut mapped_dirs = self.build_mapped_directories()?;
        let has_mapped_tmp = mapped_dirs
            .iter()
            .any(|dir| dir.guest == "/tmp" || dir.guest.starts_with("/tmp/"));
        if !has_mapped_tmp && self.mount_host_tmp {
            let tmp_folder_path = tempfile::Builder::new()
                .prefix("wasmer-")
                .tempdir()?
                .into_path();
            mapped_dirs.push(MappedDirectory {
                host: tmp_folder_path,
                guest: "/tmp".to_string(),
            });
        }
        if !mapped_dirs.is_empty() {
            let has_root_tmp = false;
            for MappedDirectory { host, guest } in mapped_dirs {
                tracing::debug!("Mounting host directory {} in {}", host.display(), guest);
                let native_fs = HostFileSystem::new(host.canonicalize()?)?;
                // Create the parent dirs
                if let Some(parent) = PathBuf::from(guest.clone()).parent() {
                    virtual_fs::ops::create_dir_all(&root_fs, parent)?;
                }
                let fs: Arc<dyn virtual_fs::FileSystem + Send + Sync + 'static> =
                    Arc::new(native_fs);
                root_fs
                    .mount(guest.clone().into(), &fs, PathBuf::new())
                    .map_err(|_e| {
                        anyhow!(
                            "There has been a collision. \nA folder might be already mounted in {} or it's parent.",
                            guest
                        )
                        .context(format!(
                            "Could not mount {} in {}",
                            host.display(),
                            guest
                        ))
                    })?;
            }
        }
        if let Err(e) = root_fs.create_dir(Path::new(Self::MAPPED_CURRENT_DIR_DEFAULT_PATH)) {
            tracing::debug!(
                "Could not create /home directory, probably the path is already mounted"
            );
        }
        Ok(root_fs)
    }

    fn build_mapped_directories(&self) -> Result<Vec<MappedDirectory>> {
        let mut mapped_dirs = Vec::new();

        // Process the --dirs flag and merge it with --mapdir flag.
        for dir in &self.pre_opened_directories {
            if !dir.is_relative() {
                bail!(
                    "Invalid argument '--dir {}': path must be relative",
                    dir.display(),
                );
            }
            let guest = PathBuf::from(Self::MAPPED_CURRENT_DIR_DEFAULT_PATH)
                .join(dir)
                .display()
                .to_string();
            let host = dir.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize path for argument '--dir {}'",
                    dir.display()
                )
            })?;
            mapped_dirs.push(MappedDirectory { host, guest });
        }

        // Process the --mapdir flag.
        for MappedDirectory { host, guest } in &self.mapped_dirs {
            let host = host.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize path for argument '--mapdir {}:{}'",
                    host.display(),
                    guest,
                )
            })?;
            let guest = PathBuf::from(Self::MAPPED_CURRENT_DIR_DEFAULT_PATH)
                .join(guest)
                .display()
                .to_string();

            mapped_dirs.push(MappedDirectory { host, guest });
        }

        Ok(mapped_dirs)
    }

    pub fn build_mapped_commands(&self) -> Result<Vec<MappedCommand>, anyhow::Error> {
        self.map_commands
            .iter()
            .map(|item| {
                let (a, b) = item.split_once('=').with_context(|| {
                    format!(
                        "Invalid --map-command flag: expected <ALIAS>=<HOST_PATH>, got '{item}'"
                    )
                })?;

                let a = a.trim();
                let b = b.trim();

                if a.is_empty() {
                    bail!("Invalid --map-command flag - alias cannot be empty: '{item}'");
                }
                // TODO(theduke): check if host command exists, and canonicalize PathBuf.
                if b.is_empty() {
                    bail!("Invalid --map-command flag - host path cannot be empty: '{item}'");
                }

                Ok(MappedCommand {
                    alias: a.to_string(),
                    target: b.to_string(),
                })
            })
            .collect::<Result<Vec<_>, anyhow::Error>>()
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

    pub fn for_binfmt_interpreter() -> Result<Self> {
        let dir = std::env::var_os("WASMER_BINFMT_MISC_PREOPEN")
            .map(Into::into)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            deny_multiple_wasi_versions: true,
            env_vars: std::env::vars().collect(),
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
        let tokens = tokens_by_authority(env)?;

        let loader = BuiltinPackageLoader::new()
            .with_cache_dir(checkout_dir)
            .with_shared_http_client(client)
            .with_tokens(tokens);

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
        let mut wapm_source = WapmSource::new(graphql_endpoint, Arc::clone(&client))
            .with_local_cache(cache_dir, WAPM_SOURCE_CACHE_TIMEOUT);
        if let Some(token) = env
            .config()?
            .registry
            .get_login_token_for_registry(wapm_source.registry_endpoint().as_str())
        {
            wapm_source = wapm_source.with_auth_token(token);
        }
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

fn tokens_by_authority(env: &WasmerEnv) -> Result<HashMap<String, String>> {
    let mut tokens = HashMap::new();
    let config = env.config()?;

    for credentials in config.registry.tokens {
        if let Ok(url) = Url::parse(&credentials.registry) {
            if url.has_authority() {
                tokens.insert(url.authority().to_string(), credentials.token);
            }
        }
    }

    if let (Ok(current_registry), Some(token)) = (env.registry_endpoint(), env.token()) {
        if current_registry.has_authority() {
            tokens.insert(current_registry.authority().to_string(), token);
        }
    }

    // Note: The global wasmer.toml config file stores URLs for the GraphQL
    // endpoint, however that's often on the backend (i.e.
    // https://registry.wasmer.io/graphql) and we also want to use the same API
    // token when sending requests to the frontend (e.g. downloading a package
    // using the `Accept: application/webc` header).
    //
    // As a workaround to avoid needing to query *all* backends to find out
    // their frontend URL every time the `wasmer` CLI runs, we'll assume that
    // when a backend is called something like `registry.wasmer.io`, the
    // frontend will be at `wasmer.io`. This works everywhere except for people
    // developing the backend locally... Sorry, Ayush.

    let mut frontend_tokens = HashMap::new();
    for (hostname, token) in &tokens {
        if let Some(frontend_url) = hostname.strip_prefix("registry.") {
            if !tokens.contains_key(frontend_url) {
                frontend_tokens.insert(frontend_url.to_string(), token.clone());
            }
        }
    }
    tokens.extend(frontend_tokens);

    Ok(tokens)
}
