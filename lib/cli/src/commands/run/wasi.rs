use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{mpsc::Sender, Arc},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use clap::Parser;
use tokio::runtime::Handle;
use url::Url;
use virtual_fs::{DeviceFile, FileSystem, PassthruFileSystem, RootFileSystemBuilder};
use wasmer::{Engine, Function, Instance, Memory32, Memory64, Module, RuntimeError, Store, Value};
use wasmer_config::package::PackageSource as PackageSpecifier;
use wasmer_registry::wasmer_env::WasmerEnv;
#[cfg(feature = "journal")]
use wasmer_wasix::journal::{LogFileJournal, SnapshotTrigger};
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    default_fs_backing, get_wasi_versions,
    http::HttpClient,
    journal::{CompactingLogFileJournal, DynJournal},
    os::{tty_sys::SysTty, TtyBridge},
    rewind_ext,
    runners::{MappedCommand, MappedDirectory},
    runtime::{
        module_cache::{FileSystemCache, ModuleCache, ModuleHash},
        package_loader::{BuiltinPackageLoader, PackageLoader},
        resolver::{FileSystemSource, InMemorySource, MultiSource, Source, WapmSource, WebSource},
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

    /// Enables an exponential backoff (measured in milli-seconds) of
    /// the process CPU usage when there are no active run tokens (when set
    /// holds the maximum amount of time that it will pause the CPU)
    /// (default = off)
    #[clap(long = "enable-cpu-backoff")]
    pub enable_cpu_backoff: Option<u64>,

    /// Specifies one or more journal files that Wasmer will use to restore
    /// and save the state of the WASM process as it executes.
    ///
    /// The state of the WASM process and its sandbox will be reapplied using
    /// the journals in the order that you specify here.
    ///
    /// The last journal file specified will be created if it does not exist
    /// and opened for read and write. New journal events will be written to this
    /// file
    #[cfg(feature = "journal")]
    #[clap(long = "journal")]
    pub journals: Vec<PathBuf>,

    /// Flag that indicates if the journal will be automatically compacted
    /// as it fills up and when the process exits
    #[cfg(feature = "journal")]
    #[clap(long = "enable-compaction")]
    pub enable_compaction: bool,

    /// Tells the compactor not to compact when the journal log file is closed
    #[cfg(feature = "journal")]
    #[clap(long = "without-compact-on-drop")]
    pub without_compact_on_drop: bool,

    /// Tells the compactor to compact when it grows by a certain factor of
    /// its original size. (i.e. '0.2' would be it compacts after the journal
    /// has grown by 20 percent)
    ///
    /// Default is to compact on growth that exceeds 15%
    #[cfg(feature = "journal")]
    #[clap(long = "with-compact-on-growth", default_value = "0.15")]
    pub with_compact_on_growth: f32,

    /// Indicates what events will cause a snapshot to be taken
    /// and written to the journal file.
    ///
    /// If not specified, the default is to snapshot when the process idles, when
    /// the process exits or periodically if an interval argument is also supplied.
    ///
    /// Additionally if the snapshot-on is not specified it will also take a snapshot
    /// on the first stdin, environ or socket listen - this can be used to accelerate
    /// the boot up time of WASM processes.
    #[cfg(feature = "journal")]
    #[clap(long = "snapshot-on")]
    pub snapshot_on: Vec<SnapshotTrigger>,

    /// Adds a periodic interval (measured in milli-seconds) that the runtime will automatically
    /// take snapshots of the running process and write them to the journal. When specifying
    /// this parameter it implies that `--snapshot-on interval` has also been specified.
    #[cfg(feature = "journal")]
    #[clap(long = "snapshot-period")]
    pub snapshot_interval: Option<u64>,

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
    const MAPPED_CURRENT_DIR_DEFAULT_PATH: &'static str = "/mnt/host";

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
            let specifier = PackageSpecifier::from_str(name)
                .with_context(|| format!("Unable to parse \"{name}\" as a package specifier"))?;
            let pkg = {
                let inner_rt = rt.clone();
                rt.task_manager()
                    .spawn_and_block_on(async move {
                        BinaryPackage::from_registry(&specifier, &*inner_rt).await
                    })
                    .with_context(|| format!("Unable to load \"{name}\""))??
            };
            uses.push(pkg);
        }

        let builder = WasiEnv::builder(program_name)
            .runtime(Arc::clone(&rt))
            .args(args)
            .envs(self.env_vars.clone())
            .uses(uses)
            .map_commands(map_commands);

        let mut builder = {
            // If we preopen anything from the host then shallow copy it over
            let root_fs = RootFileSystemBuilder::new()
                .with_tty(Box::new(DeviceFile::new(__WASI_STDIN_FILENO)))
                .build();

            let mut mapped_dirs = Vec::new();

            // Process the --dirs flag and merge it with --mapdir.
            let mut have_current_dir = false;
            for dir in &self.pre_opened_directories {
                let mapping = if dir == Path::new(".") {
                    if have_current_dir {
                        bail!("Cannot pre-open the current directory twice: --dir=. must only be specified once");
                    }
                    have_current_dir = true;

                    let current_dir =
                        std::env::current_dir().context("could not determine current directory")?;

                    MappedDirectory {
                        host: current_dir,
                        guest: Self::MAPPED_CURRENT_DIR_DEFAULT_PATH.to_string(),
                    }
                } else {
                    let resolved = dir.canonicalize().with_context(|| {
                        format!(
                            "could not canonicalize path for argument '--dir {}'",
                            dir.display()
                        )
                    })?;

                    if &resolved != dir {
                        bail!(
                            "Invalid argument '--dir {}': path must either be absolute, or '.'",
                            dir.display(),
                        );
                    }

                    let guest = resolved
                        .to_str()
                        .with_context(|| {
                            format!(
                                "invalid argument '--dir {}': path must be valid utf-8",
                                dir.display(),
                            )
                        })?
                        .to_string();

                    MappedDirectory {
                        host: resolved,
                        guest,
                    }
                };

                mapped_dirs.push(mapping);
            }

            for MappedDirectory { host, guest } in &self.mapped_dirs {
                let resolved_host = host.canonicalize().with_context(|| {
                    format!(
                        "could not canonicalize path for argument '--mapdir {}:{}'",
                        host.display(),
                        guest,
                    )
                })?;

                let mapping = if guest == "." {
                    if have_current_dir {
                        bail!("Cannot pre-open the current directory twice: '--mapdir=?:.' / '--dir=.' must only be specified once");
                    }
                    have_current_dir = true;

                    MappedDirectory {
                        host: resolved_host,
                        guest: Self::MAPPED_CURRENT_DIR_DEFAULT_PATH.to_string(),
                    }
                } else {
                    MappedDirectory {
                        host: resolved_host,
                        guest: guest.clone(),
                    }
                };
                mapped_dirs.push(mapping);
            }

            if !mapped_dirs.is_empty() {
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
            let b = builder
                .sandbox_fs(root_fs)
                .preopen_dir(Path::new("/"))
                .unwrap();

            if have_current_dir {
                b.map_dir(".", Self::MAPPED_CURRENT_DIR_DEFAULT_PATH)?
            } else {
                b.map_dir(".", "/")?
            }
        };

        *builder.capabilities_mut() = self.capabilities();

        #[cfg(feature = "journal")]
        {
            for trigger in self.snapshot_on.iter().cloned() {
                builder.add_snapshot_trigger(trigger);
            }
            if let Some(interval) = self.snapshot_interval {
                builder.with_snapshot_interval(std::time::Duration::from_millis(interval));
            }
            for journal in self.build_journals()? {
                builder.add_journal(journal);
            }
        }

        Ok(builder)
    }

    #[cfg(feature = "journal")]
    pub fn build_journals(&self) -> anyhow::Result<Vec<Arc<DynJournal>>> {
        let mut ret = Vec::new();
        for journal in self.journals.clone() {
            if self.enable_compaction {
                let mut journal = CompactingLogFileJournal::new(journal)?;
                if !self.without_compact_on_drop {
                    journal = journal.with_compact_on_drop()
                }
                if self.with_compact_on_growth.is_normal() && self.with_compact_on_growth != 0f32 {
                    journal = journal.with_compact_on_factor_size(self.with_compact_on_growth);
                }
                ret.push(Arc::new(journal) as Arc<DynJournal>);
            } else {
                ret.push(Arc::new(LogFileJournal::new(journal)?));
            }
        }
        Ok(ret)
    }

    #[cfg(not(feature = "journal"))]
    pub fn build_journals(&self) -> anyhow::Result<Vec<Arc<DynJournal>>> {
        Ok(Vec::new())
    }

    pub fn build_mapped_directories(&self) -> Result<Vec<MappedDirectory>, anyhow::Error> {
        let mut mapped_dirs = Vec::new();

        // Process the --dirs flag and merge it with --mapdir.
        let mut have_current_dir = false;
        for dir in &self.pre_opened_directories {
            let mapping = if dir == Path::new(".") {
                if have_current_dir {
                    bail!("Cannot pre-open the current directory twice: --dir=. must only be specified once");
                }
                have_current_dir = true;

                let current_dir =
                    std::env::current_dir().context("could not determine current directory")?;

                MappedDirectory {
                    host: current_dir,
                    guest: Self::MAPPED_CURRENT_DIR_DEFAULT_PATH.to_string(),
                }
            } else {
                let resolved = dir.canonicalize().with_context(|| {
                    format!(
                        "could not canonicalize path for argument '--dir {}'",
                        dir.display()
                    )
                })?;

                if &resolved != dir {
                    bail!(
                        "Invalid argument '--dir {}': path must either be absolute, or '.'",
                        dir.display(),
                    );
                }

                let guest = resolved
                    .to_str()
                    .with_context(|| {
                        format!(
                            "invalid argument '--dir {}': path must be valid utf-8",
                            dir.display(),
                        )
                    })?
                    .to_string();

                MappedDirectory {
                    host: resolved,
                    guest,
                }
            };

            mapped_dirs.push(mapping);
        }

        for MappedDirectory { host, guest } in &self.mapped_dirs {
            let resolved_host = host.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize path for argument '--mapdir {}:{}'",
                    host.display(),
                    guest,
                )
            })?;

            let mapping = if guest == "." {
                if have_current_dir {
                    bail!("Cannot pre-open the current directory twice: '--mapdir=?:.' / '--dir=.' must only be specified once");
                }
                have_current_dir = true;

                MappedDirectory {
                    host: resolved_host,
                    guest: Self::MAPPED_CURRENT_DIR_DEFAULT_PATH.to_string(),
                }
            } else {
                MappedDirectory {
                    host: resolved_host,
                    guest: guest.clone(),
                }
            };
            mapped_dirs.push(mapping);
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
        caps.threading.enable_exponential_cpu_backoff =
            self.enable_cpu_backoff.map(Duration::from_millis);

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
        let tokio_task_manager = Arc::new(TokioTaskManager::new(rt_or_handle.into()));
        let mut rt = PluggableRuntime::new(tokio_task_manager.clone());

        if self.networking {
            rt.set_networking_implementation(virtual_net::host::LocalNetworking::default());
        } else {
            rt.set_networking_implementation(virtual_net::UnsupportedVirtualNetworking::default());
        }

        #[cfg(feature = "journal")]
        for journal in self.build_journals()? {
            rt.add_journal(journal);
        }

        if !self.no_tty {
            let tty = Arc::new(SysTty);
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
            .with_fallback(FileSystemCache::new(cache_dir, tokio_task_manager));

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
        module_hash: ModuleHash,
        program_name: String,
        args: Vec<String>,
        runtime: Arc<dyn Runtime + Send + Sync>,
        store: &mut Store,
    ) -> Result<(WasiFunctionEnv, Instance)> {
        let builder = self.prepare(module, program_name, args, runtime)?;
        let (instance, wasi_env) = builder.instantiate_ext(module.clone(), module_hash, store)?;

        Ok((wasi_env, instance))
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
